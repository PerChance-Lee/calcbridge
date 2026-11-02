//! The `.tns` container: a ZIP whose first local header starts with `*TIMLP` + four hex digits (the
//! document version) instead of `PK\3\4`, and whose end-of-central-directory record starts with `TIPD`.

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;

use crate::{Result, TnsError, crypto, tixc};

pub const DEFAULT_VERSION: u16 = 0x0500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// TI-encrypted TIXC XML (ZIP method 13)
    Ti,
    Deflate,
    Store,
}

impl Method {
    fn code(self) -> u16 {
        match self {
            Method::Ti => 13,
            Method::Deflate => 8,
            Method::Store => 0,
        }
    }
}

/// A container entry. For `Method::Ti` the data is XML text (decoded on read, encoded on write).
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub method: Method,
}

impl Entry {
    pub fn xml(name: &str, xml: impl Into<String>) -> Self {
        Entry { name: name.into(), data: xml.into().into_bytes(), method: Method::Ti }
    }
    pub fn file(name: &str, data: impl Into<Vec<u8>>) -> Self {
        Entry { name: name.into(), data: data.into(), method: Method::Deflate }
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

fn u16_at(b: &[u8], off: usize) -> Result<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| TnsError::Damaged("truncated header".into()))
}

fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| TnsError::Damaged("truncated header".into()))
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The most a whole document may unpack to.
pub const MAX_DOCUMENT: usize = 256 << 20;

/// Parse a document. Returns (document version, entries).
pub fn read(blob: &[u8]) -> Result<(u16, Vec<Entry>)> {
    if !blob.starts_with(b"*TIMLP") {
        return Err(TnsError::NotTns);
    }
    let version = std::str::from_utf8(&blob[6..10.min(blob.len())])
        .ok()
        .and_then(|s| u16::from_str_radix(s, 16).ok())
        .unwrap_or(0);
    let eocd = rfind(blob, b"TIPD")
        .or_else(|| rfind(blob, b"PK\x05\x06"))
        .ok_or_else(|| TnsError::Damaged("no central directory".into()))?;
    let count = u16_at(blob, eocd + 10)? as usize;
    let cd_off = u32_at(blob, eocd + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    let mut total = 0usize;
    let mut p = cd_off;
    for _ in 0..count {
        if blob.get(p..p + 4) != Some(b"PK\x01\x02") {
            return Err(TnsError::Damaged("bad central directory".into()));
        }
        let method = u16_at(blob, p + 10)?;
        let csize = u32_at(blob, p + 20)? as usize;
        let nlen = u16_at(blob, p + 28)? as usize;
        let xlen = u16_at(blob, p + 30)? as usize;
        let clen = u16_at(blob, p + 32)? as usize;
        let lho = u32_at(blob, p + 42)? as usize;
        let name = String::from_utf8_lossy(
            blob.get(p + 46..p + 46 + nlen).ok_or_else(|| TnsError::Damaged("truncated name".into()))?,
        )
        .into_owned();
        p += 46 + nlen + xlen + clen;
        let hdr = lho + if blob.get(lho..lho + 6) == Some(b"*TIMLP") { 10 } else { 4 };
        let lnlen = u16_at(blob, hdr + 22)? as usize;
        let lxlen = u16_at(blob, hdr + 24)? as usize;
        let start = hdr + 26 + lnlen + lxlen;
        let raw = blob.get(start..start + csize).ok_or_else(|| TnsError::Damaged("truncated entry".into()))?;
        let (data, method) = match method {
            13 => (tixc::decode(&crypto::decrypt(raw)?)?.into_bytes(), Method::Ti),
            8 => (crypto::inflate(raw, &name)?, Method::Deflate),
            _ => (raw.to_vec(), Method::Store),
        };
        total += data.len();
        if total > MAX_DOCUMENT {
            return Err(TnsError::Damaged(format!("unpacks to more than {} MB", MAX_DOCUMENT >> 20)));
        }
        entries.push(Entry { name, data, method });
    }
    Ok((version, entries))
}

/// Write a document. `Document.xml` should be the first entry.
pub fn write(entries: &[Entry], version: u16) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (idx, e) in entries.iter().enumerate() {
        let (payload, crc, usize_) = match e.method {
            Method::Ti => {
                let xml = String::from_utf8_lossy(&e.data);
                let payload = crypto::encrypt(&tixc::encode(&xml)?);
                let crc = crc32(&payload);
                let n = payload.len();
                (payload, crc, n)
            }
            Method::Deflate => {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
                enc.write_all(&e.data).expect("in-memory write");
                (enc.finish().expect("in-memory deflate"), crc32(&e.data), e.data.len())
            }
            Method::Store => (e.data.clone(), crc32(&e.data), e.data.len()),
        };
        let name = e.name.as_bytes();
        let offset = out.len() as u32;
        if idx == 0 {
            out.extend_from_slice(b"*TIMLP");
            out.extend_from_slice(format!("{version:04X}").as_bytes());
        } else {
            out.extend_from_slice(b"PK\x03\x04");
        }
        for v in [20u16, 0, e.method.code()] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // DOS time/date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(usize_ as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&payload);

        central.extend_from_slice(&0x02014B50u32.to_le_bytes());
        for v in [20u16, 20, 0, e.method.code()] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(usize_ as u32).to_le_bytes());
        for v in [name.len() as u16, 0, 0, 0, 0] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"TIPD");
    for v in [0u16, 0, entries.len() as u16, entries.len() as u16] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// CRC-32 (IEEE), as ZIP uses.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let entries = vec![
            Entry::xml("Document.xml", "<doc ver=\"1.0\"><nps>1</nps></doc>"),
            Entry::xml("Problem1.xml", "<prob><sym></sym></prob>"),
            Entry::file("a.py", b"print(1)\n".to_vec()),
        ];
        let blob = write(&entries, DEFAULT_VERSION).unwrap();
        assert!(blob.starts_with(b"*TIMLP0500"));
        let (version, back) = read(&blob).unwrap();
        assert_eq!(version, 0x500);
        assert_eq!(back.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Document.xml", "Problem1.xml", "a.py"]);
        assert!(back[0].text().contains("<nps>1</nps>"));
        assert_eq!(back[2].data, b"print(1)\n");
    }

    #[test]
    fn refuses_a_part_that_unpacks_too_far() {
        // 65 MB of zeros deflates to a few dozen KB: a "zip bomb" in miniature
        let entries = vec![Entry::file("huge.bin", vec![0; crypto::MAX_INFLATED + (1 << 20)])];
        let blob = write(&entries, DEFAULT_VERSION).unwrap();
        assert!(blob.len() < 1 << 20);
        assert!(matches!(read(&blob), Err(TnsError::Damaged(_))));
    }

    #[test]
    fn rejects_non_tns() {
        assert!(matches!(read(b"PK\x03\x04garbage"), Err(TnsError::NotTns)));
    }
}
