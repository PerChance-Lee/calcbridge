//! NNSE ("NavNet SE"), the link layer the CX II wraps NavNet packets in.
//!
//! Wire format (big endian), 12-byte header then payload:
//!     misc  service  src  dest  unknown  reqAck  length(2)  seqno(2)  checksum(2)
//! The checksum makes the 16-bit ones'-complement sum of the whole message 0xFFFF.
//!
//! Observed on a real CX II (OS 5.4):
//!   * The calc accepts our stream packets only with seqno == last+1. seqno == last is acked again
//!     as a duplicate (and not processed); anything else is silently dropped.
//!   * It runs the address/time handshake once per USB connection. A later program finds a silent
//!     link and has to rediscover the sequence number, so we remember it per device between runs.
//!   * After about 1.5 s of idle it drops the first packet it receives. Retransmitting the same
//!     seqno gets it through, so every send retries on a short ack timeout.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::transport::Transport;
use crate::{Error, Result};

pub const ADDR_ALL: u8 = 0xFF;
pub const ADDR_ME: u8 = 0xFE;
pub const ADDR_CALC: u8 = 0x01;
pub const SVC_ADDRREQ: u8 = 0x01;
pub const SVC_TIME: u8 = 0x02;
pub const SVC_STREAM: u8 = 0x04;
pub const SVC_UNKNOWN: u8 = 0x08;
pub const ACK_FLAG: u8 = 0x80;
pub const REQ_ACK: u8 = 0x01;
pub const REQ_ACK_RETRY: u8 = 0x09;

pub const HEADER_LEN: usize = 12;
pub const MAX_PAYLOAD: usize = 1472;
pub const MAX_MESSAGE: usize = HEADER_LEN + MAX_PAYLOAD;

const SAVE_EVERY: u32 = 8;
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const MIN_WAIT: Duration = Duration::from_millis(1);

pub fn checksum(data: &[u8]) -> u16 {
    let mut acc: u32 = 0;
    let mut words = data.chunks_exact(2);
    for w in &mut words {
        acc += (u32::from(w[0]) << 8) | u32::from(w[1]);
    }
    if let [last] = words.remainder() {
        acc += u32::from(*last) << 8;
    }
    while acc >> 16 != 0 {
        acc = (acc >> 16) + (acc & 0xFFFF);
    }
    acc as u16
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub misc: u8,
    pub service: u8,
    pub src: u8,
    pub dest: u8,
    pub unknown: u8,
    pub req_ack: u8,
    pub seqno: u16,
    pub data: Vec<u8>,
}

impl Message {
    /// A message from us to the calculator.
    pub fn new(service: u8, data: impl Into<Vec<u8>>) -> Self {
        Message { misc: 0, service, src: ADDR_ME, dest: ADDR_CALC, unknown: 0, req_ack: 0, seqno: 0, data: data.into() }
    }

    pub fn seq(mut self, seqno: u16) -> Self {
        self.seqno = seqno;
        self
    }

    pub fn ack(mut self, req_ack: u8) -> Self {
        self.req_ack = req_ack;
        self
    }

    pub fn route(mut self, src: u8, dest: u8) -> Self {
        self.src = src;
        self.dest = dest;
        self
    }

    pub fn encode(&self) -> Vec<u8> {
        let len = HEADER_LEN + self.data.len();
        let mut m = Vec::with_capacity(len);
        m.extend_from_slice(&[self.misc, self.service, self.src, self.dest, self.unknown, self.req_ack]);
        m.extend_from_slice(&(len as u16).to_be_bytes());
        m.extend_from_slice(&self.seqno.to_be_bytes());
        m.extend_from_slice(&[0, 0]);
        m.extend_from_slice(&self.data);
        let sum = checksum(&m) ^ 0xFFFF;
        m[10..12].copy_from_slice(&sum.to_be_bytes());
        m
    }

    /// `None` if the bytes aren't a valid message.
    pub fn decode(buf: &[u8]) -> Option<Message> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([buf[6], buf[7]]));
        if length < HEADER_LEN || length > buf.len() || checksum(&buf[..length]) != 0xFFFF {
            return None;
        }
        Some(Message {
            misc: buf[0],
            service: buf[1],
            src: buf[2],
            dest: buf[3],
            unknown: buf[4],
            req_ack: buf[5],
            seqno: u16::from_be_bytes([buf[8], buf[9]]),
            data: buf[HEADER_LEN..length].to_vec(),
        })
    }
}

/// Remembers the next sequence number per device, across program runs.
pub struct SeqStore {
    path: Option<PathBuf>,
}

static STORE_LOCK: Mutex<()> = Mutex::new(());

impl SeqStore {
    pub fn new(path: Option<PathBuf>) -> Self {
        SeqStore { path }
    }

    fn read_all(&self) -> serde_json::Map<String, serde_json::Value> {
        self.path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn load(&self, key: &str) -> Option<u16> {
        self.path.as_ref()?;
        self.read_all().get(key)?.as_u64().and_then(|n| u16::try_from(n).ok())
    }

    /// Best effort: this only speeds up the next connection, so it must never fail an operation.
    /// (On Windows, antivirus and indexers briefly lock freshly written files.)
    pub fn save(&self, key: &str, seq: u16) {
        let Some(path) = &self.path else { return };
        let _guard = STORE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut data = self.read_all();
        data.insert(key.to_owned(), seq.into());
        let text = serde_json::Value::Object(data).to_string();
        for attempt in 1..=3 {
            let written = (|| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
                std::fs::write(&tmp, &text)?;
                std::fs::rename(&tmp, path)
            })();
            match written {
                Ok(()) => return,
                Err(e) => {
                    log::debug!("couldn't save link state (attempt {attempt}): {e}");
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkConfig {
    /// How long to wait for the ack of a stream packet before sending it again.
    pub ack_timeout: Duration,
    /// Attempts per stream packet.
    pub retries: u32,
    /// Ack wait per candidate while rediscovering the sequence number of a silent link.
    pub probe_timeout: Duration,
    /// How far above the remembered sequence number to search.
    pub probe_limit: u32,
}

impl Default for LinkConfig {
    fn default() -> Self {
        LinkConfig {
            ack_timeout: Duration::from_millis(700),
            retries: 8,
            probe_timeout: Duration::from_millis(120),
            probe_limit: 1024,
        }
    }
}

/// Packet log sink.
pub type Trace = Arc<dyn Fn(&str) + Send + Sync>;

fn hex(data: &[u8]) -> String {
    data.iter().take(20).map(|b| format!("{b:02x}")).collect()
}

/// Reliable stream of NavNet packets over NNSE.
pub struct Link {
    t: Box<dyn Transport>,
    store: SeqStore,
    trace: Option<Trace>,
    cfg: LinkConfig,
    seqno: u16,
    ready: bool,
    ever_ready: bool,
    reset_seen: bool,
    inbox: VecDeque<Vec<u8>>,
    last_rx_seq: Option<u16>,
    sent_since_save: u32,
    closed: bool,
}

impl Link {
    pub fn new(transport: Box<dyn Transport>, store: SeqStore, trace: Option<Trace>, cfg: LinkConfig) -> Self {
        Link {
            t: transport,
            store,
            trace,
            cfg,
            seqno: 0,
            ready: false,
            ever_ready: false,
            reset_seen: false,
            inbox: VecDeque::new(),
            last_rx_seq: None,
            sent_since_save: 0,
            closed: false,
        }
    }

    pub fn seqno(&self) -> u16 {
        self.seqno
    }

    pub fn device_key(&self) -> String {
        self.t.device_key()
    }

    // ------------------------------------------------------------ raw I/O
    fn write_msg(&mut self, m: &Message) -> Result<()> {
        if let Some(trace) = &self.trace {
            trace(&format!("-> svc={:02x} seq={} ack={} {}", m.service, m.seqno, m.req_ack, hex(&m.data)));
        }
        self.t.write(&m.encode(), WRITE_TIMEOUT)
    }

    /// One message, or `None` on timeout.
    ///
    /// Each USB transfer carries one message. Bytes after the header's length are dropped: the CX II
    /// sends a 1472-byte message (23 x 64) as a 1473-byte transfer, presumably so the transfer still
    /// ends in a short packet on a full-speed link.
    fn read_msg(&mut self, timeout: Duration) -> Result<Option<Message>> {
        let deadline = Instant::now() + timeout.max(MIN_WAIT);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left < MIN_WAIT {
                return Ok(None);
            }
            let Some(mut buf) = self.t.read(MAX_MESSAGE, left)? else { return Ok(None) };
            if buf.len() >= HEADER_LEN {
                let length = usize::from(u16::from_be_bytes([buf[6], buf[7]]));
                // a message split over transfers: collect the rest
                while (HEADER_LEN..=MAX_MESSAGE).contains(&length) && buf.len() < length {
                    match self.t.read(length - buf.len(), Duration::from_secs(1))? {
                        Some(more) if !more.is_empty() => buf.extend_from_slice(&more),
                        _ => break,
                    }
                }
            }
            match Message::decode(&buf) {
                Some(m) => {
                    if let Some(trace) = &self.trace {
                        trace(&format!("<- svc={:02x} seq={} ack={} {}", m.service, m.seqno, m.req_ack, hex(&m.data)));
                    }
                    return Ok(Some(m));
                }
                None => log::debug!("dropping {} bytes that aren't a message", buf.len()),
            }
        }
    }

    // -------------------------------------------------------- housekeeping
    /// Ack and answer link-level traffic. Stream payloads go to the inbox.
    fn handle(&mut self, m: &Message) -> Result<()> {
        if !(m.dest == ADDR_ME || m.dest == ADDR_ALL) || m.service & ACK_FLAG != 0 {
            return Ok(());
        }
        if m.req_ack & 1 != 0 {
            let ack = Message {
                misc: m.misc,
                service: m.service | ACK_FLAG,
                src: m.dest,
                dest: m.src,
                unknown: m.unknown,
                req_ack: m.req_ack & !1,
                seqno: m.seqno,
                data: Vec::new(),
            };
            match self.write_msg(&ack) {
                // a lost ack just makes the calc send that packet again; we de-duplicate it
                Ok(()) | Err(Error::Link(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let svc = m.service & !ACK_FLAG;
        let first = m.data.first().copied();
        if svc == SVC_ADDRREQ && first == Some(0) {
            // A fresh link: the calc's sequence expectations start over, and nothing is usable until
            // its time request completes the handshake. It sometimes drops the first reply after
            // reconnecting, so answer twice like libnspire does.
            if self.ready {
                self.reset_seen = true;
            }
            self.ready = false;
            self.seqno = 0;
            self.last_rx_seq = None;
            self.inbox.clear(); // anything queued belongs to the old link
            for _ in 0..2 {
                let seq = self.next_seq();
                self.write_msg(&Message::new(SVC_ADDRREQ, [0x80]).seq(seq))?;
            }
        } else if svc == SVC_TIME && first == Some(0) {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32);
            let mut reply = vec![0x80];
            reply.extend_from_slice(&now.to_be_bytes());
            reply.extend_from_slice(&[0; 12]);
            let seq = self.next_seq();
            self.write_msg(&Message::new(SVC_TIME, reply).seq(seq))?;
            self.ready = true;
            self.ever_ready = true;
        } else if svc == SVC_UNKNOWN && m.data == [0x01] {
            let seq = self.next_seq();
            self.write_msg(&Message::new(SVC_UNKNOWN, [0x81, 0x03]).seq(seq))?;
        } else if svc == SVC_STREAM {
            if Some(m.seqno) == self.last_rx_seq {
                return Ok(()); // a retransmit of something we already have (our ack got lost)
            }
            self.last_rx_seq = Some(m.seqno);
            self.inbox.push_back(m.data.clone());
        }
        Ok(())
    }

    fn next_seq(&mut self) -> u16 {
        let s = self.seqno;
        self.seqno = self.seqno.wrapping_add(1);
        s
    }

    fn check_reset(&mut self) -> Result<()> {
        if self.reset_seen {
            self.reset_seen = false;
            return Err(Error::LinkReset);
        }
        Ok(())
    }

    // ---------------------------------------------------------------- open
    /// Bring the link up and deliver the first stream payload (NavNet's address assignment).
    pub fn open(
        &mut self,
        first_payload: &[u8],
        mut progress: Option<&mut (dyn FnMut(usize, usize) + '_)>,
    ) -> Result<()> {
        self.inbox.clear();
        self.reset_seen = false;
        if self.ready {
            // Reopening after a reset whose new handshake already finished (usually inside the very
            // wait that noticed the reset): sequence numbers continue from it, nothing to look for.
            return self.send(first_payload);
        }
        // A freshly plugged-in calc repeats address requests; an established link stays silent.
        // (After a mid-session reset the address request was already answered; the time request
        // that completes the handshake follows shortly.)
        let mut deadline = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < deadline && !self.ready {
            let Some(m) = self.read_msg(Duration::from_millis(300))? else { continue };
            if m.service == SVC_ADDRREQ || m.service == SVC_UNKNOWN {
                deadline = deadline.max(Instant::now() + Duration::from_secs(3)); // handshake under way
            }
            self.handle(&m)?;
        }
        if self.ready {
            // we just did the handshake: sequence numbers continue from it
            self.reset_seen = false;
            return self.send(first_payload);
        }
        // A calculator seen before under another key (CalcBridge 1.x keyed devices without a readable
        // serial number as "default") is most likely the same one.
        let key = self.t.device_key();
        let saved = u32::from(self.store.load(&key).or_else(|| self.store.load("default")).unwrap_or(0));
        let start = saved.saturating_sub(4);
        // Most likely just behind the saved number; then everything below it; then further above.
        let order: Vec<u32> =
            (start..start + 64).chain(0..start).chain(start + 64..start + self.cfg.probe_limit).collect();
        log::info!("link silent; probing for the sequence number near {start}");
        for (attempt, &candidate) in order.iter().enumerate() {
            if attempt > 0 && attempt % 16 == 0 {
                if let Some(p) = progress.as_deref_mut() {
                    p(attempt, order.len());
                }
            }
            let seq = candidate as u16; // wraps like the calc's counter
            // the first packet after an idle period may be dropped, so the first candidate gets a retry
            let flags: &[u8] = if attempt == 0 { &[REQ_ACK, REQ_ACK_RETRY] } else { &[REQ_ACK] };
            let wait = if attempt == 0 { self.cfg.ack_timeout } else { self.cfg.probe_timeout };
            for &flag in flags {
                match self.write_msg(&Message::new(SVC_STREAM, first_payload).ack(flag).seq(seq)) {
                    Ok(()) => {}
                    Err(Error::Link(_)) => continue,
                    Err(e) => return Err(e),
                }
                if self.wait_ack(seq, wait)? {
                    self.seqno = seq.wrapping_add(1);
                    self.ready = true;
                    self.ever_ready = true;
                    self.reset_seen = false;
                    self.save(true);
                    return Ok(());
                }
                if self.ready {
                    // the calc was re-plugged while we probed and just did a handshake
                    self.reset_seen = false;
                    return self.send(first_payload);
                }
            }
        }
        Err(Error::Link("The calculator isn't answering. Unplug it, plug it back in, and try again.".into()))
    }

    // ---------------------------------------------------------- send/recv
    fn wait_ack(&mut self, seq: u16, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left < MIN_WAIT {
                return Ok(false);
            }
            let Some(m) = self.read_msg(left)? else { return Ok(false) };
            self.handle(&m)?;
            if m.dest == ADDR_ME && m.service == SVC_STREAM | ACK_FLAG && m.seqno == seq {
                return Ok(true);
            }
        }
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_PAYLOAD {
            return Err(Error::Invalid("payload too large for one NNSE message".into()));
        }
        self.check_reset()?;
        let seq = self.next_seq();
        match self.send_attempts(seq, payload) {
            Ok(true) => {
                self.save(false);
                Ok(())
            }
            Ok(false) => {
                self.seqno = seq; // never acked, so the calc still expects this number
                self.save(true);
                Err(Error::Link("The calculator stopped answering".into()))
            }
            Err(Error::LinkReset) => Err(Error::LinkReset), // the new handshake set the seqno; keep it
            Err(e) => {
                self.seqno = seq;
                Err(e)
            }
        }
    }

    fn send_attempts(&mut self, seq: u16, payload: &[u8]) -> Result<bool> {
        for attempt in 0..self.cfg.retries {
            let flag = if attempt == 0 { REQ_ACK } else { REQ_ACK_RETRY };
            match self.write_msg(&Message::new(SVC_STREAM, payload).ack(flag).seq(seq)) {
                Ok(()) => {}
                Err(Error::Link(_)) => continue, // e.g. a USB write timeout: same as a lost packet
                Err(e) => return Err(e),
            }
            if self.wait_ack(seq, self.cfg.ack_timeout)? {
                return Ok(true);
            }
            self.check_reset()?;
        }
        Ok(false)
    }

    pub fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        loop {
            self.check_reset()?;
            if let Some(data) = self.inbox.pop_front() {
                return Ok(data);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left < MIN_WAIT {
                return Err(Error::Timeout("Timed out waiting for the calculator".into()));
            }
            if let Some(m) = self.read_msg(left.min(Duration::from_secs(1)))? {
                self.handle(&m)?;
            }
        }
    }

    fn save(&mut self, force: bool) {
        self.sent_since_save += 1;
        if force || self.sent_since_save >= SAVE_EVERY {
            self.sent_since_save = 0;
            self.store.save(&self.t.device_key(), self.seqno);
        }
    }

    pub fn close(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        if self.ever_ready {
            // A link that never came up knows nothing; keep the number remembered from last time.
            self.store.save(&self.t.device_key(), self.seqno);
        }
        self.t.close();
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}
