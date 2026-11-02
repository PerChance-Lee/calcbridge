//! Remote keypresses (NavNet service 0x4042), as implemented by tilibs/libticalcs (GPL-2.0+).
//!
//! A key is a 24-bit code: ASCII character << 16 | scan code << 8 | modifier flags. The table is the
//! subset of tilibs' keysnsp.h that maps to keys on the CX II keypad.

use crate::Result;
use crate::navnet::Session;

pub const SID_KEYS: u16 = 0x4042;

/// Named keys. Single letters (`a`..`z`, `A`..`Z`) come from [`LETTER_SCAN`] instead.
pub const KEYS: &[(&str, u32)] = &[
    ("esc", 0x1B9600),
    ("tab", 0x099500),
    ("shift_tab", 0x7C9500),
    ("home", 0x00FD00),
    ("menu", 0x003600),
    ("ctrl_menu", 0x00CE00),
    ("click", 0x00AD00),
    ("left", 0x000700),
    ("right", 0x002700),
    ("up", 0x001700),
    ("down", 0x003700),
    ("enter", 0x0D1000),
    ("ctrl_enter", 0x00A600),
    ("del", 0x081500),
    ("var", 0x00AF00),
    ("ctrl_var", 0x00A800),
    ("catalog", 0x009100),
    ("on", 0x000B00),
    ("ctrl_home", 0x00FE00),
    ("ctrl_tab", 0x00ED00),
    ("ctrl_left", 0x00DA00),
    ("ctrl_right", 0x00DB00),
    ("ctrl_up", 0x00DC00),
    ("ctrl_down", 0x00DD00),
    ("ctrl", 0x00AA04),
    ("shift", 0x00AB03),
    ("=", 0x3D7500),
    ("<", 0x3CA600),
    (">", 0x3E8600),
    ("(", 0x285500),
    (")", 0x293500),
    ("^", 0x5E9300),
    ("sin", 0x007400),
    ("cos", 0x005400),
    ("tan", 0x003400),
    ("/", 0x2F1400),
    ("*", 0x2A1300),
    ("-", 0x2D1200),
    ("+", 0x2B1100),
    ("square", 0x009300),
    ("10^", 0x00EC00),
    ("e^", 0x00CA00),
    ("neg", 0xB13000),
    (".", 0x2E7000),
    (",", 0x2CA000),
    (" ", 0x202000),
    ("?", 0x3F0300),
    (":", 0x3A0100),
    ("\"", 0x22A100),
    ("'", 0x27F500),
    ("pi", 0x00A300),
    ("theta", 0x880600),
    // the imaginary unit; the letter i is "i"
    ("imag", 0x00A200),
    ("flag", 0x00A700),
    ("EE", 0x00A400),
    ("0", 0x305000),
    ("1", 0x317300),
    ("2", 0x325300),
    ("3", 0x333300),
    ("4", 0x347200),
    ("5", 0x355200),
    ("6", 0x363200),
    ("7", 0x377100),
    ("8", 0x385100),
    ("9", 0x393100),
];

/// Scan codes of the letter keys, a..z.
pub const LETTER_SCAN: [u8; 26] = [
    0x66, 0x46, 0x26, 0x85, 0x65, 0x45, 0x25, 0x84, 0x64, 0x44, 0x24, 0x83, 0x63, 0x43, 0x23, 0x82, 0x62,
    0x42, 0x22, 0x81, 0x61, 0x41, 0x21, 0x80, 0x60, 0x40,
];

pub fn key_code(name: &str) -> Option<u32> {
    if let Some(&(_, code)) = KEYS.iter().find(|(n, _)| *n == name) {
        return Some(code);
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii_alphabetic() => {
            let scan = LETTER_SCAN[usize::from(ch.to_ascii_lowercase() as u8 - b'a')];
            Some((u32::from(ch) << 16) | (u32::from(scan) << 8))
        }
        _ => None,
    }
}

/// Every key name [`key_code`] accepts, for help texts.
pub fn names() -> impl Iterator<Item = String> {
    KEYS.iter().map(|(n, _)| (*n).to_owned()).chain(('a'..='z').map(String::from))
}

pub fn press(s: &mut Session, code: u32) -> Result<()> {
    s.with_service(SID_KEYS, |s| {
        s.write(&[0x01, 0x00, 0x00, 0x80])?;
        let mut ev = [0u8; 26];
        ev[4] = 0x08;
        ev[5] = 0x02;
        ev[6] = (code >> 16) as u8;
        ev[8] = (code >> 8) as u8;
        ev[24] = code as u8;
        s.write(&ev)
    })
}

/// Key codes to type a string (letters, digits, simple punctuation), or the first character the
/// handheld has no key for.
pub fn text_codes(text: &str) -> Result<Vec<u32>, char> {
    text.chars().map(|c| key_code(c.encode_utf8(&mut [0; 4])).ok_or(c)).collect()
}
