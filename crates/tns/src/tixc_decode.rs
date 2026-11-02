//! TIXC token-stream decoder.
//!
//! Ported from TnsTools' `tixc_decode.py` (https://github.com/MaksimirKurtov/TnsTools),
//! MIT License, Copyright (c) 2026 tnstools contributors (full text in THIRD_PARTY.md), with one change:
//! literal closing tags (TI writes `</name>` once its 256-entry tag dictionary is full) are passed through.

const ETA: &[u8; 32] = b" etaionsrhAlcduFmfpgybwvkxqjz,.'";
const DIGIT: &[u8; 32] = b"0123456789AN,.EFx; (){}[]^+-/*PB";
const CDATA_OPEN: &[u8] = b"<![CDATA[";
const CDATA_CLOSE: &[u8] = b"]]>";

fn special_digit(b: u8, out: &mut Vec<u8>) {
    match b {
        b'N' => out.extend_from_slice("\u{2212}".as_bytes()),
        b'E' => out.extend_from_slice(&[0xef, 0x80, 0x80]),
        b'P' => out.extend_from_slice(b"))"),
        b'B' => out.extend_from_slice(b"]["),
        _ => out.push(b),
    }
}

#[derive(Default)]
struct TextDecoder {
    state: u32,
    count: i32,
    tmp: u8,
}

impl TextDecoder {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn decode_byte(&mut self, b: u8, out: &mut Vec<u8>) -> Result<(), String> {
        match self.state {
            0 => {
                if b < 0xA0 {
                    let needs_utf = (b <= 0x1F && (((-9729i64 as u32 as u64) >> b) & 1) == 1) || b >= 0x7F;
                    if needs_utf {
                        if b & 0xF0 == 0x80 {
                            self.count = (b & 0x0F) as i32 + 1;
                            self.state = 28;
                            return Ok(());
                        }
                        if b >= 8 {
                            match b {
                                8 => self.state = 30,
                                11 => self.state = 33,
                                _ => return Err(format!("unsupported TIXC text control byte 0x{b:02x}")),
                            }
                            return Ok(());
                        }
                        self.tmp = ((((b & 7) | 0xF0) as u32 * 4) & 0xFF) as u8;
                        self.count = 1;
                        self.state = 27;
                        return Ok(());
                    }
                    out.push(b);
                    return Ok(());
                }
                if b & 0xF0 == 0xA0 {
                    let ch = ETA[(b & 0x0F) as usize];
                    match ch {
                        b'F' => self.state = 24,
                        b'A' => self.state = 0,
                        _ => {
                            self.state = 23;
                            out.push(ch);
                        }
                    }
                    return Ok(());
                }
                if b & 0xF0 == 0xF0 {
                    let ch = DIGIT[(b & 0x0F) as usize];
                    match ch {
                        b'A' => self.state = 26,
                        b'F' => self.state = 0,
                        _ => {
                            self.state = 25;
                            special_digit(ch, out);
                        }
                    }
                }
                Ok(())
            }
            23 => {
                let ch1 = ETA[(b >> 4) as usize];
                if ch1 == b'F' {
                    out.push(ETA[(b & 0x0F) as usize + 16]);
                    return Ok(());
                }
                if ch1 == b'A' {
                    self.state = 0;
                    return Ok(());
                }
                out.push(ch1);
                let ch2 = ETA[(b & 0x0F) as usize];
                match ch2 {
                    b'F' => self.state = 24,
                    b'A' => self.state = 0,
                    _ => out.push(ch2),
                }
                Ok(())
            }
            24 => {
                out.push(ETA[(b >> 4) as usize + 16]);
                let ch2 = ETA[(b & 0x0F) as usize];
                match ch2 {
                    b'F' => {}
                    b'A' => self.state = 0,
                    _ => {
                        out.push(ch2);
                        self.state = 23;
                    }
                }
                Ok(())
            }
            25 => {
                let ch1 = DIGIT[(b >> 4) as usize];
                if ch1 == b'A' {
                    special_digit(DIGIT[(b & 0x0F) as usize + 16], out);
                    return Ok(());
                }
                if ch1 == b'F' {
                    self.state = 0;
                    return Ok(());
                }
                special_digit(ch1, out);
                let ch2 = DIGIT[(b & 0x0F) as usize];
                match ch2 {
                    b'A' => self.state = 26,
                    b'F' => self.state = 0,
                    _ => special_digit(ch2, out),
                }
                Ok(())
            }
            26 => {
                special_digit(DIGIT[(b >> 4) as usize + 16], out);
                let ch2 = DIGIT[(b & 0x0F) as usize];
                match ch2 {
                    b'A' => {}
                    b'F' => self.state = 0,
                    _ => {
                        self.state = 25;
                        special_digit(ch2, out);
                    }
                }
                Ok(())
            }
            27 => {
                self.state = 0;
                self.count = 0;
                out.extend_from_slice(&[self.tmp | (b >> 6), (b & 0x3F) | 0x80]);
                Ok(())
            }
            28 => {
                self.state = 29;
                self.tmp = b;
                out.push((b >> 4) | 0xE0);
                Ok(())
            }
            29 => {
                self.count -= 1;
                self.state = if self.count <= 0 { 0 } else { 28 };
                out.extend_from_slice(&[
                    ((b >> 6) as u32 | ((((self.tmp & 0x0F) | 0xE0) as u32) << 2)) as u8,
                    (b & 0x3F) | 0x80,
                ]);
                Ok(())
            }
            30 => {
                self.tmp = b;
                self.state = 31;
                if b >= 0x20 {
                    self.count = 2;
                    out.extend_from_slice(&[0xF8, (b >> 2) | 0x80]);
                } else {
                    self.count = 1;
                    out.push(((b >> 2) & 7) | 0xF0);
                }
                Ok(())
            }
            31 | 35 => {
                self.count = 1;
                self.state = 32;
                out.push(((b >> 4) as u32 | ((((self.tmp & 3) | 0xF8) as u32) << 4)) as u8);
                self.tmp = b;
                Ok(())
            }
            32 | 36 => {
                self.state = 0;
                self.count = 0;
                out.extend_from_slice(&[
                    ((b >> 6) as u32 | ((((self.tmp & 0x0F) | 0xE0) as u32) << 2)) as u8,
                    (b & 0x3F) | 0x80,
                ]);
                Ok(())
            }
            33 => {
                self.state = 34;
                if b >= 4 {
                    out.extend_from_slice(&[(((b & 0x40) != 0) as u8) | 0xFC, (b & 0x3F) | 0x80]);
                } else {
                    out.push((b & 3) | 0xF8);
                }
                Ok(())
            }
            34 => {
                self.state = 35;
                self.tmp = b;
                self.count = 1;
                out.push((b >> 2) | 0x80);
                Ok(())
            }
            s => Err(format!("unsupported TIXC text state {s}")),
        }
    }
}

fn is_tag_terminator(b: u8) -> bool {
    matches!(b, 0x0E | 0x0F | 0x20 | 0x2F | 0x3E)
}

/// TIXC0100 stream → XML bytes.
pub fn decode_tixc(data: &[u8]) -> Result<Vec<u8>, String> {
    if !data.starts_with(b"TIXC") {
        return Err("TIXC stream does not start with TIXC".into());
    }
    let dash = data[4..].iter().position(|&b| b == b'-').map(|p| p + 4).ok_or("invalid TIXC header")?;
    let qmark = data[dash + 1..].iter().position(|&b| b == b'?').map(|p| p + dash + 1).ok_or("invalid TIXC header")?;
    if data.get(qmark + 1) != Some(&b'>') {
        return Err("invalid TIXC header".into());
    }
    let version = &data[dash + 1..qmark];
    let mut pos = qmark + 2;
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 4);
    out.extend_from_slice(b"<?xml version=\"");
    out.extend_from_slice(version);
    out.extend_from_slice(b"\" encoding=\"UTF-8\" ?>");
    let mut tags: Vec<Vec<u8>> = Vec::new();
    let mut attrs: Vec<Vec<u8>> = Vec::new();
    let mut token: Vec<u8> = Vec::new();
    let mut text = TextDecoder::default();
    let mut state = 1u32;
    let mut cdata_match = 0usize;

    while pos < data.len() {
        let b = data[pos];
        match state {
            1 => {
                if b != b'<' {
                    return Err("expected root '<' after TIXC header".into());
                }
                out.push(b);
                pos += 1;
                state = 2;
            }
            2 => {
                if is_tag_terminator(b) {
                    match b {
                        0x0E => {
                            out.extend_from_slice(b"></");
                            state = 12;
                        }
                        0x0F => {
                            out.push(b' ');
                            state = 13;
                        }
                        0x20 => {
                            if token.is_empty() {
                                return Err("empty tag before attribute separator".into());
                            }
                            out.push(b' ');
                            state = 4;
                        }
                        0x2F => {
                            out.push(b'/');
                            state = if token.is_empty() { 10 } else { 9 };
                        }
                        _ => {
                            if token.is_empty() {
                                return Err("empty tag before '>'".into());
                            }
                            out.push(b'>');
                            state = 3;
                        }
                    }
                    if !token.is_empty() {
                        if tags.len() < 256 {
                            tags.push(token.clone());
                        }
                        token.clear();
                    }
                    pos += 1;
                } else {
                    if token.len() >= 64 {
                        return Err("tag token exceeds 64 bytes".into());
                    }
                    token.push(b);
                    out.push(b);
                    pos += 1;
                }
            }
            3 => {
                text.reset();
                token.clear();
                match b {
                    b'<' => {
                        out.push(b);
                        pos += 1;
                        state = 17;
                    }
                    0x0C => {
                        out.push(b'<');
                        pos += 1;
                        state = 11;
                    }
                    0x0E => {
                        out.extend_from_slice(b"</");
                        pos += 1;
                        state = 12;
                    }
                    _ => state = 8,
                }
            }
            4 => {
                if b == b'=' {
                    if attrs.len() < 256 {
                        attrs.push(token.clone());
                    }
                    token.clear();
                    out.push(b);
                    pos += 1;
                    state = 5;
                } else {
                    token.push(b);
                    out.push(b);
                    pos += 1;
                }
            }
            5 => {
                pos += 1;
                if b == b'"' {
                    out.push(b);
                    state = 6;
                } else if b != 0x20 {
                    return Err("expected quote or space after attribute '='".into());
                }
            }
            6 => {
                if b == b'"' && text.state == 0 {
                    out.push(b);
                    token.clear();
                    state = 7;
                } else {
                    text.decode_byte(b, &mut out)?;
                }
                pos += 1;
            }
            7 => {
                match b {
                    b'>' => {
                        out.push(b);
                        state = 3;
                    }
                    b'/' => {
                        out.push(b);
                        state = 10;
                    }
                    0x20 => {
                        out.push(b);
                        state = 4;
                    }
                    0x0F => {
                        out.push(b' ');
                        state = 13;
                    }
                    _ => return Err(format!("unexpected byte 0x{b:02x} after attribute value")),
                }
                pos += 1;
            }
            8 => {
                if b == b'<' && text.state == 0 {
                    out.push(b);
                    token.clear();
                    state = 2;
                } else if b == 0x0C && text.state == 0 {
                    out.push(b'<');
                    token.clear();
                    state = 11;
                } else if b == 0x0E && text.state == 0 {
                    out.extend_from_slice(b"</");
                    token.clear();
                    state = 12;
                } else {
                    text.decode_byte(b, &mut out)?;
                }
                pos += 1;
            }
            9 => {
                out.push(b);
                pos += 1;
                if b == b'>' {
                    state = 3;
                }
            }
            10 => {
                if b != b'>' {
                    // literal closing tag, used once the 256-entry tag dictionary is full
                    let end = data[pos..].iter().position(|&x| x == b'>').ok_or("unterminated closing tag")? + pos;
                    out.extend_from_slice(&data[pos..=end]);
                    pos = end + 1;
                    state = 3;
                    continue;
                }
                out.push(b);
                pos += 1;
                state = 3;
            }
            11 => {
                let tag = tags.get(b as usize).ok_or_else(|| format!("tag dictionary index {b} out of range"))?;
                out.extend_from_slice(tag);
                pos += 1;
                state = 14;
            }
            12 => {
                let tag = tags.get(b as usize).ok_or_else(|| format!("tag dictionary index {b} out of range"))?;
                out.extend_from_slice(tag);
                out.push(b'>');
                pos += 1;
                state = 3;
            }
            13 => {
                let attr =
                    attrs.get(b as usize).ok_or_else(|| format!("attribute dictionary index {b} out of range"))?;
                out.extend_from_slice(attr);
                out.extend_from_slice(b"=\"");
                pos += 1;
                text.reset();
                state = 6;
            }
            14 => match b {
                0x20 => {
                    out.push(b' ');
                    pos += 1;
                    state = 4;
                }
                b'/' => {
                    pos += 1;
                    state = 15;
                }
                0x0F => {
                    out.push(b' ');
                    pos += 1;
                    state = 13;
                }
                _ => {
                    out.push(b'>');
                    state = 3;
                }
            },
            15 => {
                if b == b'>' {
                    out.push(b'/');
                    state = 10;
                } else {
                    out.push(b'>');
                    state = 3;
                }
            }
            17 => {
                if b == b'!' {
                    out.push(b);
                    pos += 1;
                    cdata_match = 2;
                    state = 18;
                } else {
                    state = 2;
                }
            }
            18 => {
                if cdata_match >= CDATA_OPEN.len() {
                    state = 19;
                    token.clear();
                    continue;
                }
                if b != CDATA_OPEN[cdata_match] {
                    return Err("invalid CDATA opener".into());
                }
                out.push(b);
                pos += 1;
                cdata_match += 1;
            }
            19 => {
                out.push(b);
                pos += 1;
                if b == b']' {
                    cdata_match = 1;
                    state = 20;
                }
            }
            20 => {
                out.push(b);
                pos += 1;
                if cdata_match < CDATA_CLOSE.len() && b == CDATA_CLOSE[cdata_match] {
                    cdata_match += 1;
                } else {
                    state = 19;
                    cdata_match = 0;
                }
                if cdata_match == CDATA_CLOSE.len() {
                    state = 3;
                    cdata_match = 0;
                }
            }
            s => return Err(format!("unsupported TIXC parser state {s}")),
        }
    }
    if state != 3 {
        return Err(format!("TIXC stream ended in state {state}"));
    }
    Ok(out)
}
