"""The .tns container: a ZIP variant with TI's method-13 encrypted XML entries.

Container: a ZIP whose first local-header magic is "*TIMLP" + four hex digits (document version) and
whose end-of-central-directory magic is "TIPD". XML entries use method 13:

    40-byte header, 3DES-ECB encrypted under a fixed key: "TIEN0100", block count (LE u32 = 0x400),
    counter seed (LE u32), 21 bytes of key material (three 56-bit DES keys), 3 bytes padding
    body: raw deflate of a TIXC token stream, XORed with a keystream of 3DES-ECB(00000000 || LE u32
    (seed + block % blocks)) under the key built from the key material.

The format was worked out by the Luna (ndless-nspire) and TnsTools projects; this is an independent
implementation. Other entries (scripts, images) are plain deflate.
"""
from __future__ import annotations

import os
import struct
import zlib
from dataclasses import dataclass

from Crypto.Cipher import DES3

from . import tixc_decode

HEADER_KEY = bytes.fromhex("79c4e0f45eef7a5b70137a57c2fd3d2cc2707cc1ad2f1575")
TIXC_MAGIC = b"TIXC0100-1.0?>"
DEFAULT_VERSION = 0x0500
METHOD_TI, METHOD_DEFLATE, METHOD_STORE = 13, 8, 0


# ----------------------------------------------------------------- crypto
def _des_key(k7: bytes) -> bytes:
    b = list(k7)
    out = [b[0] & 0xFE, ((b[1] >> 1) & 0x7E) | (b[0] << 7 & 0xFF), ((b[2] >> 2) & 0x3E) | (b[1] << 6 & 0xFF),
           ((b[3] >> 3) & 0x1E) | (b[2] << 5 & 0xFF), ((b[4] >> 4) & 0x0E) | (b[3] << 4 & 0xFF),
           ((b[5] >> 5) & 0x06) | (b[4] << 3 & 0xFF), ((b[6] >> 6) & 0x02) | (b[5] << 2 & 0xFF), b[6] << 1 & 0xFF]
    return bytes(x | (bin(x).count("1") % 2 == 0) for x in out)  # odd parity


def _keystream_xor(data: bytes, seed: int, material: bytes, blocks: int = 0x400) -> bytes:
    key = b"".join(_des_key(material[i:i + 7]) for i in (0, 7, 14))
    ecb = DES3.new(key, DES3.MODE_ECB)
    n = (len(data) + 7) // 8
    stream = ecb.encrypt(b"".join(struct.pack("<II", 0, (seed + i % blocks) & 0xFFFFFFFF) for i in range(n)))
    return bytes(a ^ b for a, b in zip(data, stream))


def decrypt(payload: bytes) -> bytes:
    """Method-13 payload -> TIXC (or plain) bytes."""
    hdr = DES3.new(HEADER_KEY, DES3.MODE_ECB).decrypt(payload[:40])
    if not hdr.startswith(b"TIEN0100"):
        raise ValueError("not a TI-encrypted entry")
    blocks, seed = struct.unpack_from("<II", hdr, 8)
    return zlib.decompress(_keystream_xor(payload[40:], seed, hdr[16:37], blocks or 0x400), -15)


def encrypt(plain: bytes, seed: int | None = None, material: bytes | None = None) -> bytes:
    seed = int.from_bytes(os.urandom(4), "little") if seed is None else seed
    material = os.urandom(21) if material is None else material
    deflater = zlib.compressobj(9, zlib.DEFLATED, -15)
    packed = deflater.compress(plain) + deflater.flush()
    header = b"TIEN0100" + struct.pack("<II", 0x400, seed) + material + b"\0\0\0"
    return DES3.new(HEADER_KEY, DES3.MODE_ECB).encrypt(header) + _keystream_xor(packed, seed, material)


# -------------------------------------------------------------------- TIXC
def tixc_encode(xml: str) -> bytes:
    """XML -> TIXC. Uses only literal tags (always valid TIXC): every opening tag adds a dictionary
    entry; closing tags are 0x0E + entry index, or written out in full past 256 entries."""
    starts = [i for i in (xml.find("<prob"), xml.find("<doc")) if i >= 0]
    s = xml[min(starts):] if starts else xml
    out = bytearray(TIXC_MAGIC)
    stack, next_id, i = [], 0, 0
    while i < len(s):
        if s.startswith("<![CDATA[", i):
            end = s.index("]]>", i) + 3
            out += _escape(s[i:end])
            i = end
        elif s.startswith("</", i):
            tag_id = stack.pop()
            end = s.index(">", i)
            out += bytes([0x0E, tag_id]) if tag_id < 256 else s[i:end + 1].encode("ascii")
            i = end + 1
        elif s.startswith("<!--", i):
            i = s.index("-->", i) + 3  # comments aren't supported by TIXC; drop them
        else:
            if s[i] == "<":
                end = s.index(">", i)
                if s[end - 1] == "/":  # expand <x/> into <x></x>
                    name = s[i + 1:end - 1].split()[0]
                    s = s[:end - 1] + f"></{name}>" + s[end + 1:]
                stack.append(next_id)
                next_id += 1
            out += _escape(s[i])
            i += 1
    if stack:
        raise ValueError("unbalanced XML")
    return bytes(out)


def _escape(text: str) -> bytes:
    out = bytearray()
    for c in text:
        u = ord(c)
        if u < 0x80:
            out.append(u)
        elif u < 0x800:
            out += bytes([u >> 8, u & 0xFF])
        elif u < 0x10000:
            out += bytes([0x80, u >> 8, u & 0xFF])
        else:
            out += bytes([0x08, (u >> 16) & 0xFF, (u >> 8) & 0xFF, u & 0xFF])
    return bytes(out)


def tixc_decode_xml(data: bytes) -> str:
    if not data.startswith(b"TIXC"):
        return data.decode("utf-8", "replace")
    return tixc_decode.decode_tixc(data).decode("utf-8", "replace")


# --------------------------------------------------------------- container
@dataclass
class Entry:
    name: str
    data: bytes  # decoded: XML text bytes for method 13, raw bytes otherwise
    method: int = METHOD_DEFLATE


def read(blob: bytes) -> tuple[int, list[Entry]]:
    """-> (document version, entries)."""
    if blob[:6] != b"*TIMLP":
        raise ValueError("not a TI-Nspire document (.tns)")
    version = int(blob[6:10], 16) if blob[6:10].isalnum() else 0
    eocd = blob.rfind(b"TIPD")
    if eocd < 0:
        eocd = blob.rfind(b"PK\x05\x06")
    if eocd < 0:
        raise ValueError("damaged .tns: no central directory")
    count, _size, cd_off = struct.unpack_from("<HII", blob, eocd + 10)
    entries, p = [], cd_off
    for _ in range(count):
        if blob[p:p + 4] != b"PK\x01\x02":
            raise ValueError("damaged .tns: bad central directory")
        method, _crc, csize, _usize, nlen, xlen, clen, lho = struct.unpack_from("<H4xIIIHHH8xI", blob, p + 10)
        name = blob[p + 46:p + 46 + nlen].decode("utf-8", "replace")
        p += 46 + nlen + xlen + clen
        hdr = lho + (10 if blob[lho:lho + 6] == b"*TIMLP" else 4)
        lnlen, lxlen = struct.unpack_from("<HH", blob, hdr + 22)
        raw = blob[hdr + 26 + lnlen + lxlen:hdr + 26 + lnlen + lxlen + csize]
        if method == METHOD_TI:
            data = tixc_decode_xml(decrypt(raw)).encode("utf-8")
        elif method == METHOD_DEFLATE:
            data = zlib.decompress(raw, -15)
        else:
            data = raw
        entries.append(Entry(name, data, method))
    return version, entries


def write(entries: list[Entry], version: int = DEFAULT_VERSION) -> bytes:
    """Entries with method 13 hold XML text (str or UTF-8 bytes); Document.xml must come first."""
    out, central = bytearray(), bytearray()
    for idx, e in enumerate(entries):
        if e.method == METHOD_TI:
            xml = e.data.decode("utf-8") if isinstance(e.data, bytes) else e.data
            payload = encrypt(tixc_encode(xml))
            crc, usize = zlib.crc32(payload), len(payload)
        elif e.method == METHOD_DEFLATE:
            d = zlib.compressobj(9, zlib.DEFLATED, -15)
            payload = d.compress(e.data) + d.flush()
            crc, usize = zlib.crc32(e.data), len(e.data)
        else:
            payload, crc, usize = e.data, zlib.crc32(e.data), len(e.data)
        name = e.name.encode("utf-8")
        offset = len(out)
        magic = b"*TIMLP" + f"{version:04X}".encode() if idx == 0 else b"PK\x03\x04"
        out += magic + struct.pack("<HHHIIIIHH", 20, 0, e.method, 0, crc, len(payload), usize, len(name), 0)
        out += name + payload
        central += struct.pack("<IHHHHIIIIHHHHHII", 0x02014B50, 20, 20, 0, e.method, 0, crc, len(payload),
                               usize, len(name), 0, 0, 0, 0, 0, offset) + name
    cd_off = len(out)
    out += central + b"TIPD" + struct.pack("<HHHHIIH", 0, 0, len(entries), len(entries), len(central), cd_off, 0)
    return bytes(out)
