//! TIXC encoding: XML text → TI's tokenised XML stream.
//!
//! Only literal tags are written (always valid TIXC): every opening tag adds a dictionary entry;
//! closing tags become `0x0E` + entry index, or are written out in full past 256 entries. Outside
//! CDATA, characters from U+0080 use TI's escapes and control characters are never written raw,
//! because in TIXC 0x00–0x08 and 0x0B start multi-byte sequences and 0x0C, 0x0E–0x1F are tokens;
//! tab/newline/CR become character references. CDATA (Lua scripts) is plain UTF-8.

use crate::{Result, TnsError};

pub const MAGIC: &[u8] = b"TIXC0100-1.0?>";

fn escape(text: &str, cdata: bool, out: &mut Vec<u8>) {
    for c in text.chars() {
        let u = c as u32;
        if u < 0x20 {
            if matches!(c, '\t' | '\n' | '\r') {
                if cdata {
                    out.push(u as u8);
                } else {
                    out.extend_from_slice(format!("&#{u};").as_bytes());
                }
            }
            continue;
        }
        if u < 0x80 {
            out.push(u as u8);
        } else if cdata {
            // Observed: the handheld reads CDATA (Lua scripts) as plain UTF-8; TI's character escapes
            // there make it report "an error was found in the format of this document".
            out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        } else if u < 0x800 {
            out.extend_from_slice(&[(u >> 8) as u8, (u & 0xFF) as u8]);
        } else if u < 0x10000 {
            out.extend_from_slice(&[0x80, (u >> 8) as u8, (u & 0xFF) as u8]);
        } else {
            out.extend_from_slice(&[0x08, (u >> 16) as u8, (u >> 8) as u8, u as u8]);
        }
    }
}

/// XML (from the root `<prob>`/`<doc>` element on) → TIXC bytes.
pub fn encode(xml: &str) -> Result<Vec<u8>> {
    let start = [xml.find("<prob"), xml.find("<doc")].into_iter().flatten().min().unwrap_or(0);
    let mut s: String = xml[start..].to_string();
    let mut out = MAGIC.to_vec();
    let mut stack: Vec<usize> = Vec::new();
    let mut next_id = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("<![CDATA[") {
            let end = i + rest.find("]]>").ok_or_else(|| TnsError::Invalid("unclosed CDATA".into()))? + 3;
            escape(&s[i..end], true, &mut out);
            i = end;
        } else if rest.starts_with("</") {
            let tag_id = stack.pop().ok_or_else(|| TnsError::Invalid("unbalanced XML".into()))?;
            let end = i + rest.find('>').ok_or_else(|| TnsError::Invalid("unterminated tag".into()))?;
            if tag_id < 256 {
                out.extend_from_slice(&[0x0E, tag_id as u8]);
            } else {
                out.extend_from_slice(&s.as_bytes()[i..=end]);
            }
            i = end + 1;
        } else if rest.starts_with("<!--") {
            i += rest.find("-->").map(|p| p + 3).unwrap_or(rest.len()); // TIXC has no comments
        } else {
            let ch = rest.chars().next().unwrap();
            if ch == '<' {
                let end = i + rest.find('>').ok_or_else(|| TnsError::Invalid("unterminated tag".into()))?;
                if s.as_bytes()[end - 1] == b'/' {
                    // expand <x .../> into <x ...></x>
                    let name = s[i + 1..end - 1].split_whitespace().next().unwrap_or("").to_string();
                    s.replace_range(end - 1..=end, &format!("></{name}>"));
                }
                stack.push(next_id);
                next_id += 1;
            }
            let mut buf = [0u8; 4];
            escape(ch.encode_utf8(&mut buf), false, &mut out);
            i += ch.len_utf8();
        }
    }
    if !stack.is_empty() {
        return Err(TnsError::Invalid("unbalanced XML".into()));
    }
    Ok(out)
}

/// TIXC (or plain) bytes → XML text.
pub fn decode(data: &[u8]) -> Result<String> {
    if !data.starts_with(b"TIXC") {
        return Ok(String::from_utf8_lossy(data).into_owned());
    }
    let bytes = crate::tixc_decode::decode_tixc(data).map_err(TnsError::Tixc)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_many_tags_and_unicode() {
        let body: String = (0..400).map(|i| format!("<item n=\"{i}\">v{i} ε√≤</item>")).collect();
        let xml = format!("<prob xmlns=\"urn:TI.Problem\" ver=\"1.0\"><sym></sym><list>{body}</list><e/></prob>");
        let back = decode(&encode(&xml).unwrap()).unwrap();
        let doc = roxmltree::Document::parse(&back).unwrap();
        let items: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("item")).collect();
        assert_eq!(items.len(), 400);
        assert_eq!(items[399].text(), Some("v399 ε√≤"));
        assert!(doc.descendants().any(|n| n.has_tag_name("e")));
    }

    #[test]
    fn control_characters_never_raw() {
        let xml = "<prob><a>x\u{0c}y\u{0}z\tw</a><s><![CDATA[l1\nl2\u{0c}]]></s></prob>";
        let enc = encode(xml).unwrap();
        let back = decode(&enc).unwrap();
        let doc = roxmltree::Document::parse(&back).unwrap();
        assert_eq!(doc.descendants().find(|n| n.has_tag_name("a")).unwrap().text(), Some("xyz\tw"));
        assert!(back.contains("l1\nl2]]>"));
    }
}
