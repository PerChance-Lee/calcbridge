//! NavNet services: device info (0x4020), screenshots (0x4024), file system (0x4060), OS install
//! (0x4080).
//!
//! Byte layouts follow libnspire (GPL-3.0); each one here was checked against a CX II on OS 5.4
//! except OS install, which is implemented from libnspire but deliberately not exercised on real
//! hardware.

use std::time::Duration;

use serde::Serialize;

use crate::navnet::{MAX_DATA, Session};
use crate::{Error, RemoteKind, Result};

pub const SID_DEVINFO: u16 = 0x4020;
pub const SID_SCREEN: u16 = 0x4024;
pub const SID_FILES: u16 = 0x4060;
pub const SID_OS: u16 = 0x4080;
pub const OK: u16 = 0xFF00;
/// File data per packet: one byte of each data packet is the opcode.
pub const CHUNK: usize = MAX_DATA - 1;

// ------------------------------------------------------------------ helpers
/// Path strings are NUL-terminated and padded with zeros to at least 9 bytes.
pub fn padded(path: &str) -> Vec<u8> {
    let mut v = path.as_bytes().to_vec();
    v.resize(v.len().max(8) + 1, 0);
    v
}

/// A padded string at `off`, and the offset just after its padding.
pub fn scan_padded(buf: &[u8], off: usize) -> Result<(String, usize)> {
    let rest = buf.get(off..).ok_or_else(short)?;
    let end = rest.iter().position(|&b| b == 0).ok_or_else(|| Error::Protocol("unterminated name".into()))?;
    Ok((String::from_utf8_lossy(&rest[..end]).into_owned(), off + end.max(8) + 1))
}

fn short() -> Error {
    Error::Protocol("short reply from the calculator".into())
}

fn bytes<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N]> {
    b.get(off..off + N).and_then(|s| s.try_into().ok()).ok_or_else(short)
}

fn u16_at(b: &[u8], off: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(bytes(b, off)?))
}

fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(b, off)?))
}

fn u64_at(b: &[u8], off: usize) -> Result<u64> {
    Ok(u64::from_be_bytes(bytes(b, off)?))
}

fn version(v: [u8; 4]) -> String {
    format!("{}.{}.{}", v[0], v[1], (u32::from(v[2]) << 8) | u32::from(v[3]))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| char::from(c)).collect() // latin-1
}

// ------------------------------------------------------------------ types
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub os_version: String,
    pub boot1_version: String,
    pub boot2_version: String,
    pub hw_type: u8,
    pub hw_version: u32,
    pub flash_free: u64,
    pub flash_total: u64,
    pub ram_free: u64,
    pub ram_total: u64,
    pub battery: String,
    pub charging: bool,
    pub clock_mhz: u8,
    pub lcd_width: u16,
    pub lcd_height: u16,
    pub lcd_bpp: u8,
    pub run_level: String,
    pub electronic_id: String,
    pub file_extension: String,
    pub os_extension: String,
}

impl DeviceInfo {
    pub fn model(&self) -> String {
        let cas = self.os_extension.ends_with("cc2") || matches!(self.hw_type, 0x0E | 0x0F | 0x1C);
        if cas { "TI-Nspire CX II CAS".into() } else { "TI-Nspire CX II".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// Seconds since 1970 as the handheld reports them.
    pub date: u32,
    pub path: String,
}

impl DirEntry {
    pub fn is_document(&self) -> bool {
        !self.is_dir && self.name.to_lowercase().ends_with(".tns")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u16,
    pub height: u16,
    pub bpp: u8,
    /// 16 bpp: RGB565 little endian; 4 bpp: grey, two pixels per byte.
    pub pixels: Vec<u8>,
}

impl Screenshot {
    /// The image as 8-bit RGBA.
    pub fn to_rgba(&self) -> Vec<u8> {
        let n = usize::from(self.width) * usize::from(self.height);
        let mut out = Vec::with_capacity(n * 4);
        if self.bpp == 16 {
            for px in self.pixels.chunks_exact(2).take(n) {
                let v = u16::from_le_bytes([px[0], px[1]]);
                let (r, g, b) = ((v >> 11) & 0x1F, (v >> 5) & 0x3F, v & 0x1F);
                out.extend_from_slice(&[(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]);
            }
        } else {
            for &pair in &self.pixels {
                for nibble in [pair >> 4, pair & 0xF] {
                    let g = nibble * 17;
                    out.extend_from_slice(&[g, g, g, 255]);
                }
            }
            out.truncate(n * 4);
        }
        out.resize(n * 4, 0);
        out
    }
}

#[cfg(feature = "image")]
impl Screenshot {
    fn rgba_image(&self) -> Result<image::RgbaImage> {
        image::RgbaImage::from_raw(u32::from(self.width), u32::from(self.height), self.to_rgba())
            .ok_or_else(|| Error::Protocol("screenshot size doesn't match its pixels".into()))
    }

    /// Save as PNG, JPEG or BMP, chosen by the file extension (PNG when there is none).
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let format = image::ImageFormat::from_path(path).unwrap_or(image::ImageFormat::Png);
        let img = image::DynamicImage::ImageRgba8(self.rgba_image()?);
        let img = if format == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
        img.save_with_format(path, format).map_err(|e| Error::Io(std::io::Error::other(e)))
    }

    pub fn png(&self) -> Result<Vec<u8>> {
        let mut out = std::io::Cursor::new(Vec::new());
        self.rgba_image()?
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        Ok(out.into_inner())
    }
}

/// What an OS install is doing when it reports progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OsStage {
    Upload,
    Install,
}

// ------------------------------------------------------------------ services
pub fn device_info(s: &mut Session) -> Result<DeviceInfo> {
    s.with_service(SID_DEVINFO, |s| {
        s.write(&[0x01])?;
        let b = s.read()?;
        if b.len() < 110 {
            return Err(Error::Protocol("short device-info reply".into()));
        }
        // the struct starts after the opcode: >QQQQBBBB4s4s4sIHHHHHBBB17s27s
        let o = 1;
        let (batt, charging, clock) = (b[o + 32], b[o + 34], b[o + 35]);
        let runlevel = u16_at(&b, o + 52)?;
        s.write(&[0x02])?;
        let name = cstr(s.read()?.get(1..).unwrap_or_default());
        s.write(&[0x03])?;
        let ext = s.read()?;
        let mut ext = ext.get(1..).unwrap_or_default().split(|&c| c == 0);
        let file_extension = ext.next().filter(|e| !e.is_empty()).map_or(".tns".into(), cstr);
        let os_extension = ext.next().map(cstr).unwrap_or_default();
        Ok(DeviceInfo {
            name,
            os_version: version(bytes(&b, o + 36)?),
            boot1_version: version(bytes(&b, o + 40)?),
            boot2_version: version(bytes(&b, o + 44)?),
            hw_type: b[o + 64],
            hw_version: u32_at(&b, o + 48)?,
            flash_free: u64_at(&b, o)?,
            flash_total: u64_at(&b, o + 8)?,
            ram_free: u64_at(&b, o + 16)?,
            ram_total: u64_at(&b, o + 24)?,
            battery: match batt {
                0x00 => "powered".into(),
                0xF1 => "low".into(),
                0x7F => "ok".into(),
                0xFF => "unknown".into(),
                v => format!("0x{v:02x}"),
            },
            charging: charging != 0,
            clock_mhz: clock,
            lcd_width: u16_at(&b, o + 58)?,
            lcd_height: u16_at(&b, o + 60)?,
            lcd_bpp: b[o + 62],
            run_level: match runlevel {
                1 => "recovery".into(),
                2 => "OS".into(),
                v => v.to_string(),
            },
            electronic_id: cstr(&b[o + 82..o + 109]),
            file_extension,
            os_extension,
        })
    })
}

pub fn screenshot(s: &mut Session) -> Result<Screenshot> {
    s.with_service(SID_SCREEN, |s| {
        s.write(&[0x00])?;
        let b = s.read()?;
        // >IHHHHBB after the opcode
        let size = u32_at(&b, 1)? as usize;
        let (w, h) = (u16_at(&b, 9)?, u16_at(&b, 11)?);
        let bpp = *b.get(13).ok_or_else(short)?;
        if !matches!(bpp, 4 | 16) || w > 4096 || h > 4096 || size > 64 << 20 {
            return Err(Error::Protocol(format!("unexpected screen format {w}x{h}x{bpp}")));
        }
        let mut raw = Vec::with_capacity(size);
        while raw.len() < size {
            raw.extend_from_slice(s.read()?.get(1..).unwrap_or_default());
        }
        raw.truncate(size);
        let out_size = usize::from(w) * usize::from(h) * usize::from(bpp) / 8;
        Ok(Screenshot { width: w, height: h, bpp, pixels: rle_decode(bpp, &raw, out_size) })
    })
}

pub fn rle_decode(bpp: u8, data: &[u8], out_size: usize) -> Vec<u8> {
    let unit = usize::from(bpp) * 2 / 8; // runs are counted in pairs of pixels
    let mut out = Vec::with_capacity(out_size);
    let mut i = 0;
    while i + 1 < data.len() && out.len() < out_size {
        let n = data[i] as i8;
        i += 1;
        if n < 0 {
            let k = (usize::from(n.unsigned_abs()) + 1) * unit;
            let end = (i + k).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i += k;
        } else {
            let end = (i + unit).min(data.len());
            for _ in 0..=n {
                out.extend_from_slice(&data[i..end]);
            }
            i += unit;
        }
    }
    out.resize(out_size, 0);
    out
}

/// Inverse of [`rle_decode`] (used by the simulator and tests).
pub fn rle_encode(bpp: u8, pixels: &[u8]) -> Vec<u8> {
    let unit = usize::from(bpp) * 2 / 8;
    let units: Vec<&[u8]> = pixels.chunks(unit).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let mut j = i;
        while j + 1 < units.len() && units[j + 1] == units[i] && j - i < 127 {
            j += 1;
        }
        if j > i {
            out.push((j - i) as u8);
            out.extend_from_slice(units[i]);
            i = j + 1;
            continue;
        }
        while j + 1 < units.len() && units[j + 1] != units[j] && j - i < 127 {
            j += 1;
        }
        out.push((-((j - i) as i8)) as u8);
        for u in &units[i..=j] {
            out.extend_from_slice(u);
        }
        i = j + 1;
    }
    out
}

fn result_code(b: &[u8]) -> Option<u16> {
    (b.len() >= 2).then(|| u16::from_be_bytes([b[0], b[1]]))
}

/// FF xx result codes (hackspire / tilibs; 0x15 confirmed on a CX II: non-.tns uploads are refused).
const RESULT_MESSAGES: &[(u8, &str)] = &[
    (0x02, "the handheld is out of memory"),
    (0x04, "the file is damaged"),
    (0x07, "the path is too long"),
    (0x09, "the folder doesn't exist"),
    (0x0A, "it doesn't exist"),
    (0x0F, "that folder name isn't allowed"),
    (0x12, "a folder with that name already exists"),
    (0x13, "the path is too long"),
    (0x14, "that file name isn't allowed"),
    (0x15, "the handheld only accepts TI-Nspire documents (.tns)"),
    (0x16, "the name contains characters the handheld doesn't allow"),
    (0x20, "that OS file isn't for this model"),
    (0x80, "the handheld refuses to install an older OS"),
];

pub fn why(code: Option<u16>) -> String {
    match code {
        Some(c) if c >> 8 == 0xFF => match RESULT_MESSAGES.iter().find(|(k, _)| u16::from(*k) == c & 0xFF) {
            Some((_, m)) => (*m).into(),
            None => format!("code {c:04x}"),
        },
        Some(c) => format!("code {c:04x}"),
        None => "no reply".into(),
    }
}

pub fn list_dir(s: &mut Session, path: &str) -> Result<Vec<DirEntry>> {
    s.with_service(SID_FILES, |s| {
        let mut req = vec![0x0D];
        req.extend(padded(path));
        req.push(0);
        s.write(&req)?;
        let code = s.read()?;
        match code.get(1) {
            Some(0x0A) => return Err(Error::remote(RemoteKind::NotFound, format!("{path} doesn't exist"), Some(0xFF0A))),
            Some(0x0F) => return Err(Error::remote(RemoteKind::Refused, format!("{path} is not a folder"), Some(0xFF0F))),
            _ => {}
        }
        let mut items = Vec::new();
        loop {
            s.write(&[0x0E])?;
            let b = s.read()?;
            if b.first().is_none_or(|&c| c == 0xFF) {
                break;
            }
            let (name, off) = scan_padded(&b, 3)?;
            let size = u32_at(&b, off)?;
            let date = u32_at(&b, off + 4)?;
            let is_dir = *b.get(off + 8).ok_or_else(short)? != 0;
            let full = join(path, &name);
            items.push(DirEntry { name, size: u64::from(size), is_dir, date, path: full });
        }
        s.write(&[0x0F])?;
        s.read()?;
        Ok(items)
    })
}

pub fn stat(s: &mut Session, path: &str) -> Result<Option<DirEntry>> {
    s.with_service(SID_FILES, |s| {
        let mut req = vec![0x20, 0x01];
        req.extend(padded(path));
        req.push(0);
        s.write(&req)?;
        let b = s.read()?;
        if b.first() != Some(&0x20) {
            return Ok(None);
        }
        Ok(Some(DirEntry {
            name: basename(path).into(),
            size: u64::from(u32_at(&b, 1)?),
            date: u32_at(&b, 5)?,
            is_dir: *b.get(9).ok_or_else(short)? != 0,
            path: path.into(),
        }))
    })
}

pub fn read_file(s: &mut Session, path: &str, mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>) -> Result<Vec<u8>> {
    s.with_service(SID_FILES, |s| {
        let mut req = vec![0x07, 0x01];
        req.extend(padded(path));
        s.write(&req)?;
        let b = s.read()?;
        if result_code(&b) != Some(0x0301) {
            return Err(Error::remote(RemoteKind::NotFound, format!("{path} doesn't exist"), result_code(&b)));
        }
        let length = u32_at(&b, 11)? as usize;
        s.write(&[0x04])?;
        let mut out = Vec::with_capacity(length);
        while out.len() < length {
            out.extend_from_slice(s.read()?.get(1..).unwrap_or_default());
            if let Some(p) = progress.as_deref_mut() {
                p(out.len().min(length) as u64, length as u64);
            }
        }
        s.write(&OK.to_be_bytes())?;
        out.truncate(length);
        Ok(out)
    })
}

pub fn write_file(
    s: &mut Session,
    path: &str,
    data: &[u8],
    mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>,
) -> Result<()> {
    let total = data.len() as u64;
    s.with_service(SID_FILES, |s| {
        let mut req = vec![0x03, 0x01];
        req.extend(padded(path));
        req.extend_from_slice(&(data.len() as u32).to_be_bytes());
        s.write(&req)?;
        let b = s.read()?;
        if b.first() != Some(&0x04) {
            let code = result_code(&b);
            return Err(Error::remote(RemoteKind::Refused, format!("The calculator refused {path}: {}", why(code)), code));
        }
        let mut sent = 0;
        for chunk in data.chunks(CHUNK) {
            let mut pkt = Vec::with_capacity(chunk.len() + 1);
            pkt.push(0x05);
            pkt.extend_from_slice(chunk);
            s.write(&pkt)?;
            sent += chunk.len() as u64;
            if let Some(p) = progress.as_deref_mut() {
                p(sent, total);
            }
        }
        let result = result_code(&s.read_timeout(Duration::from_secs(30))?);
        if result != Some(OK) {
            return Err(Error::remote(RemoteKind::Refused, format!("Writing {path} failed: {}", why(result)), result));
        }
        if data.is_empty() {
            if let Some(p) = progress.as_deref_mut() {
                p(0, 0);
            }
        }
        Ok(())
    })
}

fn simple(s: &mut Session, opcode: u16, paths: &[&str], trailing_zero: bool) -> Result<Option<u16>> {
    s.with_service(SID_FILES, |s| {
        let mut req = opcode.to_be_bytes().to_vec();
        for p in paths {
            req.extend(padded(p));
        }
        if trailing_zero {
            req.push(0);
        }
        s.write(&req)?;
        Ok(result_code(&s.read()?))
    })
}

pub fn make_dir(s: &mut Session, path: &str) -> Result<()> {
    match simple(s, 0x0A03, &[path], false)? {
        Some(OK) => Ok(()),
        r => Err(Error::remote(RemoteKind::Exists, format!("Couldn't create {path}: {}", why(r)), r)),
    }
}

pub fn remove_dir(s: &mut Session, path: &str) -> Result<()> {
    match simple(s, 0x0B03, &[path], false)? {
        Some(OK) => Ok(()),
        r => Err(Error::remote(
            RemoteKind::Refused,
            format!("Couldn't remove the folder {path} (it must exist and be empty): {}", why(r)),
            r,
        )),
    }
}

pub fn delete_file(s: &mut Session, path: &str) -> Result<()> {
    match simple(s, 0x0901, &[path], true)? {
        Some(OK) => Ok(()),
        r => Err(Error::remote(RemoteKind::NotFound, format!("Couldn't delete {path}: {}", why(r)), r)),
    }
}

pub fn move_path(s: &mut Session, src: &str, dst: &str) -> Result<()> {
    match simple(s, 0x2101, &[src, dst], true)? {
        Some(OK) => Ok(()),
        r => Err(Error::remote(RemoteKind::Refused, format!("Couldn't move {src} to {dst}: {}", why(r)), r)),
    }
}

pub fn copy_file(s: &mut Session, src: &str, dst: &str) -> Result<()> {
    match simple(s, 0x0C01, &[src, dst], true)? {
        Some(OK) => Ok(()),
        r => Err(Error::remote(RemoteKind::Refused, format!("Couldn't copy {src} to {dst}: {}", why(r)), r)),
    }
}

/// Send an OS image (.tco2/.tcc2/.tct2). The calc verifies the signature itself and reboots.
pub fn install_os(
    s: &mut Session,
    image: &[u8],
    mut progress: Option<&mut (dyn FnMut(OsStage, u64, u64) + '_)>,
) -> Result<()> {
    let refused = |what: &str, code: Option<u16>| {
        Error::remote(RemoteKind::Refused, format!("The calculator {what} the OS image: {}", why(code)), code)
    };
    s.with_service(SID_OS, |s| {
        let mut req = vec![0x03];
        req.extend_from_slice(&(image.len() as u32).to_be_bytes());
        s.write(&req)?;
        let b = s.read()?;
        if b.first() != Some(&0x04) {
            return Err(refused("refused", result_code(&b)));
        }
        // After the first chunk the calc answers FF 00 (header accepted) or 04 ("keep sending"); in
        // the latter case it sends FF 00 later, so keep reading a status per chunk until it does.
        let mut awaiting_header = true;
        let mut sent = 0;
        for chunk in image.chunks(CHUNK) {
            let mut pkt = vec![0x05];
            pkt.extend_from_slice(chunk);
            s.write(&pkt)?;
            if awaiting_header {
                let st = s.read()?;
                if result_code(&st) == Some(OK) {
                    awaiting_header = false;
                } else if st.first() != Some(&0x04) {
                    return Err(refused("rejected", result_code(&st)));
                }
            }
            sent += chunk.len() as u64;
            if let Some(p) = progress.as_deref_mut() {
                p(OsStage::Upload, sent, image.len() as u64);
            }
        }
        loop {
            let b = s.read_timeout(Duration::from_secs(120))?;
            if b.first() == Some(&0xFF) {
                let code = result_code(&b);
                return Err(Error::remote(
                    RemoteKind::Refused,
                    format!("The calculator stopped the OS install: {}", why(code)),
                    code,
                ));
            }
            let pct = *b.get(1).ok_or_else(short)?;
            if let Some(p) = progress.as_deref_mut() {
                p(OsStage::Install, u64::from(pct.min(100)), 100);
            }
            if pct >= 100 {
                return Ok(()); // libnspire notes it can report over 100%
            }
        }
    })
}

// --------------------------------------------------------------------- paths
pub fn join(folder: &str, name: &str) -> String {
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() { format!("/{name}") } else { format!("{folder}/{name}") }
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

pub fn dirname(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some((head, _)) if !head.is_empty() => head.into(),
        _ => "/".into(),
    }
}
