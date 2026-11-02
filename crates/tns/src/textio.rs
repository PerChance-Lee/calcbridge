//! Small helpers for reading user files and writing extracted ones safely.

use std::path::{Path, PathBuf};

/// Decode a text file whatever its encoding: UTF-8 (with or without BOM) or UTF-16 (Notepad's
/// "Unicode", Windows PowerShell 5.1's `>` redirection). Falls back to UTF-8 with replacement.
pub fn decode_text(raw: &[u8]) -> String {
    if let Some(rest) = raw.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |bytes: &[u8], big: bool| {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|p| if big { u16::from_be_bytes([p[0], p[1]]) } else { u16::from_le_bytes([p[0], p[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = raw.strip_prefix(b"\xFF\xFE") {
        return utf16(rest, false);
    }
    if let Some(rest) = raw.strip_prefix(b"\xFE\xFF") {
        return utf16(rest, true);
    }
    // BOM-less UTF-16 LE: ASCII text with every second byte zero
    if raw.len() >= 4 && raw[1] == 0 && raw[3] == 0 && (raw[0] != 0 || raw[2] != 0) {
        return utf16(raw, false);
    }
    String::from_utf8_lossy(raw).into_owned()
}

pub fn read_text(path: &Path) -> std::io::Result<String> {
    Ok(decode_text(&std::fs::read(path)?))
}

/// A file name from an untrusted document, reduced to a bare name that can't escape the target
/// folder (no directories, drive letters or `..`). `None` if nothing usable is left.
pub fn safe_filename(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let base = base.replace(':', "_");
    let base = base.trim().trim_start_matches('.');
    let base: String = base.chars().filter(|&c| u32::from(c) >= 32 && !"<>\"|?*".contains(c)).collect();
    (!base.is_empty()).then_some(base)
}

/// `folder/name` for an untrusted `name`, or `None` if it would land anywhere else.
pub fn safe_join(folder: &Path, name: &str) -> Option<PathBuf> {
    let base = safe_filename(name)?;
    let path = folder.join(&base);
    (path.parent() == Some(folder) && path.file_name().is_some_and(|n| n == base.as_str())).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_boms() {
        let mut le = vec![0xFF, 0xFE];
        for u in "héllo\nx".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_text(&le), "héllo\nx");
        assert_eq!(decode_text(b"\xEF\xBB\xBFh\xC3\xA9"), "hé");
        assert_eq!(decode_text(b"h\0i\0"), "hi");
        assert_eq!(decode_text(b"plain"), "plain");
    }

    #[test]
    fn safe_join_stays_in_folder() {
        let dir = Path::new("out");
        for name in ["../../evil.py", "..\\..\\evil.py", "/etc/evil.py", "C:\\Windows\\evil.py", "C:evil.py", "..",
            "sub/dir/evil.py"]
        {
            if let Some(p) = safe_join(dir, name) {
                assert_eq!(p.parent(), Some(dir), "{name} -> {}", p.display());
            }
        }
        assert_eq!(safe_join(dir, "ok.py"), Some(dir.join("ok.py")));
        assert_eq!(safe_join(dir, ".."), None);
    }
}
