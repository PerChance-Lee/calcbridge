//! NavNet packets and service sessions (carried inside NNSE stream messages on the CX II).
//!
//! Packet (big endian): magic 0x54FD, src_addr, src_sid, dst_addr, dst_sid, data_checksum (2 bytes
//! each), then data_size, ack, seq, header_checksum (1 byte each), then data. data_size 0xFF means a
//! 4-byte length follows (packets up to 1440 data bytes on the CX II).

use std::time::Duration;

use crate::nnse::Link;
use crate::{Error, Result};

pub const MAGIC: u16 = 0x54FD;
pub const HEADER_LEN: usize = 16;
pub const HOST_ADDR: u16 = 0x6400;
pub const CALC_ADDR: u16 = 0x6401;
pub const SID_ADDR_ASSIGN: u16 = 0x4003;
pub const SID_DISCONNECT: u16 = 0x40DE;
pub const SID_ACK: u16 = 0xFF;
pub const SID_ACK_ZERO: u16 = 0xFE;
pub const SID_NACK: u16 = 0xD3;
pub const MAX_DATA: usize = 1440;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub fn data_checksum(data: &[u8]) -> u16 {
    let mut c: u32 = 0;
    for &b in data {
        let t1 = ((u32::from(b) << 8) | (c >> 8)) & 0xFFFF;
        c &= 0xFF;
        let t2 = ((((c & 0xF) << 4) ^ c) << 8) & 0xFFFF;
        let t3 = t2 >> 5;
        c = ((t3 >> 7) ^ t1 ^ t2 ^ t3) & 0xFFFF;
    }
    c as u16
}

/// Where this process starts numbering its service sessions (0x8000..0xBFFF).
///
/// Observed: a reply the handheld was still preparing when a program gave up (it builds
/// /NspireLogs.zip slowly) arrives later, addressed to that program's session. libnspire always starts
/// at 0x8000, so the next program to connect took those packets as answers to its own requests and
/// read garbage. Starting somewhere different each time makes them strays, which are refused.
fn first_host_sid() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() ^ (d.as_secs() as u32));
    let mixed = nanos.wrapping_mul(0x9E37_79B9) ^ std::process::id().rotate_left(16);
    0x8000 + (mixed % 0x4000) as u16
}

fn sum8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub src_addr: u16,
    pub src_sid: u16,
    pub dst_addr: u16,
    pub dst_sid: u16,
    pub ack: u8,
    pub seq: u8,
    pub data: Vec<u8>,
}

impl Packet {
    /// A packet from us to the calculator.
    pub fn new(src_sid: u16, dst_sid: u16, data: impl Into<Vec<u8>>) -> Self {
        Packet { src_addr: HOST_ADDR, src_sid, dst_addr: CALC_ADDR, dst_sid, ack: 0, seq: 0, data: data.into() }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let n = self.data.len();
        let (size, full) = if n < 0xFF {
            (n as u8, self.data.clone())
        } else if n <= MAX_DATA {
            let mut full = (n as u32).to_be_bytes().to_vec();
            full.extend_from_slice(&self.data);
            (0xFF, full)
        } else {
            return Err(Error::Invalid(format!("NavNet data too long ({n} > {MAX_DATA})")));
        };
        let mut p = Vec::with_capacity(HEADER_LEN + full.len());
        for v in [MAGIC, self.src_addr, self.src_sid, self.dst_addr, self.dst_sid, data_checksum(&full)] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        p.extend_from_slice(&[size, self.ack, self.seq, 0]);
        p[15] = sum8(&p[..15]);
        p.extend_from_slice(&full);
        Ok(p)
    }

    pub fn parse(raw: &[u8]) -> Result<Packet> {
        if raw.len() < HEADER_LEN {
            return Err(Error::Protocol("short NavNet packet".into()));
        }
        let be = |i: usize| u16::from_be_bytes([raw[i], raw[i + 1]]);
        if be(0) != MAGIC || sum8(&raw[..15]) != raw[15] {
            return Err(Error::Protocol("bad NavNet header".into()));
        }
        let size = raw[12];
        let (full, data) = if size == 0xFF {
            if raw.len() < HEADER_LEN + 4 {
                return Err(Error::Protocol("short NavNet packet".into()));
            }
            let n = u32::from_be_bytes([raw[16], raw[17], raw[18], raw[19]]) as usize;
            let end = (20 + n).min(raw.len());
            (&raw[16..end], &raw[20..end])
        } else {
            let end = (16 + usize::from(size)).min(raw.len());
            (&raw[16..end], &raw[16..end])
        };
        if data_checksum(full) != be(10) {
            return Err(Error::Protocol("bad NavNet data checksum".into()));
        }
        Ok(Packet {
            src_addr: be(2),
            src_sid: be(4),
            dst_addr: be(6),
            dst_sid: be(8),
            ack: raw[13],
            seq: raw[14],
            data: data.to_vec(),
        })
    }
}

/// A NavNet session: at most one service connection at a time, like libnspire.
pub struct Session {
    link: Link,
    host_sid: u16,
    device_sid: u16,
    service: Option<u16>,
}

impl Session {
    pub fn new(link: Link) -> Self {
        Session { link, host_sid: SID_ADDR_ASSIGN, device_sid: SID_ADDR_ASSIGN, service: None }
    }

    pub fn link(&mut self) -> &mut Link {
        &mut self.link
    }

    pub fn open(&mut self, progress: Option<&mut (dyn FnMut(usize, usize) + '_)>) -> Result<()> {
        self.host_sid = SID_ADDR_ASSIGN;
        self.device_sid = SID_ADDR_ASSIGN;
        let first = Packet::new(self.host_sid, self.device_sid, [0x64, 0x01, 0xFF, 0x00]).encode()?;
        self.link.open(&first, progress)?;
        self.host_sid = first_host_sid();
        self.service = None;
        Ok(())
    }

    // ----------------------------------------------------------- packets
    fn send(&mut self, data: &[u8], src_sid: u16, dst_sid: u16, ack: u8, seq: u8) -> Result<()> {
        let p = Packet { ack, seq, ..Packet::new(src_sid, dst_sid, data) };
        self.link.send(&p.encode()?)
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.send(data, self.host_sid, self.device_sid, 0, 0)
    }

    pub fn read(&mut self) -> Result<Vec<u8>> {
        self.read_timeout(DEFAULT_READ_TIMEOUT)
    }

    pub fn read_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        loop {
            let p = Packet::parse(&self.link.recv(timeout)?)?;
            if p.dst_sid == self.host_sid {
                return Ok(p.data);
            }
            // Something for another service (e.g. a disconnect notice): ack it, nack the rest.
            let src = if p.dst_sid != SID_DISCONNECT {
                SID_NACK
            } else if p.seq != 0 {
                SID_ACK
            } else {
                SID_ACK_ZERO
            };
            self.send(&p.dst_sid.to_be_bytes(), src, p.src_sid, 0x0A, p.seq)?;
        }
    }

    // ---------------------------------------------------------- services
    /// Connect to a service, run `f`, and disconnect again.
    pub fn with_service<T>(&mut self, sid: u16, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        self.run_service(sid, f, true)
    }

    /// Like [`Session::with_service`], but once `f` has succeeded a failed disconnect doesn't undo it:
    /// the handheld restarts straight after finishing an OS install, before it can ack the disconnect.
    pub fn with_service_then_restart<T>(&mut self, sid: u16, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        self.run_service(sid, f, false)
    }

    fn run_service<T>(&mut self, sid: u16, f: impl FnOnce(&mut Session) -> Result<T>, strict: bool) -> Result<T> {
        if self.service.is_some() {
            return Err(Error::Protocol("a service is already connected".into()));
        }
        self.service = Some(sid);
        self.device_sid = sid;
        match f(self) {
            Ok(v) => {
                match self.disconnect() {
                    Err(e) if strict => return Err(e),
                    Err(e) => log::info!("no disconnect after the finished request (the handheld restarted?): {e}"),
                    Ok(()) => {}
                }
                Ok(v)
            }
            Err(Error::LinkReset) => {
                self.service = None; // the calc forgot everything; nothing to disconnect
                Err(Error::LinkReset)
            }
            Err(e) => {
                let _ = self.disconnect(); // the first error is the one worth reporting
                Err(e)
            }
        }
    }

    fn disconnect(&mut self) -> Result<()> {
        if self.service.is_none() {
            return Ok(());
        }
        let sent = self.send(&self.host_sid.to_be_bytes(), SID_DISCONNECT, self.device_sid, 0, 0);
        self.service = None;
        self.host_sid = 0x8000 + ((self.host_sid + 1 - 0x8000) % 0x4000);
        sent
    }
}
