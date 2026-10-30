"""Remote keypresses (NavNet service 0x4042), as implemented by tilibs/libticalcs (GPL-2.0+).

A key is a 24-bit code: ASCII character << 16 | scan code << 8 | modifier flags. The table below is
the subset of tilibs' keysnsp.h that maps to keys on the CX II keypad.
"""
from __future__ import annotations

from .navnet import Session

SID_KEYS = 0x4042

KEYS = {
    "esc": 0x1B9600, "tab": 0x099500, "shift_tab": 0x7C9500, "home": 0x00FD00, "menu": 0x003600,
    "ctrl_menu": 0x00CE00, "click": 0x00AD00, "left": 0x000700, "right": 0x002700, "up": 0x001700,
    "down": 0x003700, "enter": 0x0D1000, "ctrl_enter": 0x00A600, "del": 0x081500, "var": 0x00AF00,
    "ctrl_var": 0x00A800, "catalog": 0x009100, "on": 0x000B00,
    "ctrl_home": 0x00FE00, "ctrl_tab": 0x00ED00, "ctrl_left": 0x00DA00, "ctrl_right": 0x00DB00,
    "ctrl_up": 0x00DC00, "ctrl_down": 0x00DD00, "ctrl": 0x00AA04, "shift": 0x00AB03,
    "=": 0x3D7500, "<": 0x3CA600, ">": 0x3E8600, "(": 0x285500, ")": 0x293500, "^": 0x5E9300,
    "sin": 0x007400, "cos": 0x005400, "tan": 0x003400, "/": 0x2F1400, "*": 0x2A1300, "-": 0x2D1200,
    "+": 0x2B1100, "square": 0x009300, "10^": 0x00EC00, "e^": 0x00CA00, "neg": 0xB13000, ".": 0x2E7000,
    ",": 0x2CA000, " ": 0x202000, "?": 0x3F0300, ":": 0x3A0100, '"': 0x22A100, "'": 0x27F500,
    "pi": 0x00A300, "theta": 0x880600, "i": 0x00A200, "flag": 0x00A700, "EE": 0x00A400,
    "0": 0x305000, "1": 0x317300, "2": 0x325300, "3": 0x333300, "4": 0x347200, "5": 0x355200,
    "6": 0x363200, "7": 0x377100, "8": 0x385100, "9": 0x393100,
}
_LETTER_SCAN = {"a": 0x66, "b": 0x46, "c": 0x26, "d": 0x85, "e": 0x65, "f": 0x45, "g": 0x25, "h": 0x84,
                "i": 0x64, "j": 0x44, "k": 0x24, "l": 0x83, "m": 0x63, "n": 0x43, "o": 0x23, "p": 0x82,
                "q": 0x62, "r": 0x42, "s": 0x22, "t": 0x81, "u": 0x61, "v": 0x41, "w": 0x21, "x": 0x80,
                "y": 0x60, "z": 0x40}
for _ch, _scan in _LETTER_SCAN.items():
    KEYS[_ch] = (ord(_ch) << 16) | (_scan << 8)
    KEYS[_ch.upper()] = (ord(_ch.upper()) << 16) | (_scan << 8)


def key_code(name: str) -> int:
    if name in KEYS:
        return KEYS[name]
    raise KeyError(f"unknown key {name!r}")


def press(s: Session, code: int) -> None:
    with s.connect(SID_KEYS):
        s.write(b"\x01\x00\x00\x80")
        ev = bytearray(26)
        ev[0] = 0x00  # command byte
        ev[1 + 3], ev[1 + 4] = 0x08, 0x02
        ev[1 + 5] = (code >> 16) & 0xFF
        ev[1 + 7] = (code >> 8) & 0xFF
        ev[1 + 23] = code & 0xFF
        s.write(bytes(ev))


def text_codes(text: str) -> list[int]:
    """Key codes to type a string (letters, digits, simple punctuation)."""
    out = []
    for ch in text:
        if ch in KEYS:
            out.append(KEYS[ch])
        else:
            raise KeyError(f"can't type {ch!r}")
    return out
