"""NavNet services: device info (0x4020), screenshots (0x4024), file system (0x4060), OS install (0x4080).

Byte layouts follow libnspire (GPL-3.0); each one here was checked against a CX II on OS 5.4 except
OS install, which is implemented from libnspire but deliberately not exercised on real hardware.
"""
from __future__ import annotations

import struct
from dataclasses import dataclass, field

from .errors import ExistsError, NotFoundError, ProtocolError, RemoteError
from .navnet import MAX_DATA, Session

SID_DEVINFO, SID_SCREEN, SID_FILES, SID_OS = 0x4020, 0x4024, 0x4060, 0x4080
OK = 0xFF00
CHUNK = MAX_DATA - 1  # one byte of each data packet is the opcode


# ------------------------------------------------------------------ helpers
def padded(path: str) -> bytes:
    """Path strings are NUL-terminated and padded with zeros to at least 9 bytes."""
    s = path.encode("utf-8")
    return s + b"\0" * (max(len(s), 8) - len(s) + 1)


def scan_padded(buf: bytes, off: int):
    end = buf.index(b"\0", off)
    return buf[off:end].decode("utf-8", "replace"), off + max(end - off, 8) + 1


def _version(v: bytes) -> str:
    return f"{v[0]}.{v[1]}.{(v[2] << 8) | v[3]}"


def _cstr(b: bytes) -> str:
    return b.split(b"\0")[0].decode("latin-1")


# ---------------------------------------------------------------- dataclasses
@dataclass
class DeviceInfo:
    name: str
    os_version: str
    boot1_version: str
    boot2_version: str
    hw_type: int
    hw_version: int
    flash_free: int
    flash_total: int
    ram_free: int
    ram_total: int
    battery: str
    charging: bool
    clock_mhz: int
    lcd_width: int
    lcd_height: int
    lcd_bpp: int
    run_level: str
    electronic_id: str
    file_extension: str
    os_extension: str

    @property
    def model(self) -> str:
        cas = "CAS" if self.os_extension.endswith("cc2") or self.hw_type in (0x0E, 0x0F, 0x1C) else ""
        return f"TI-Nspire CX II {cas}".strip()


@dataclass
class DirEntry:
    name: str
    size: int
    is_dir: bool
    date: int = 0
    path: str = field(default="", compare=False)

    @property
    def is_document(self) -> bool:
        return not self.is_dir and self.name.lower().endswith(".tns")


@dataclass
class Screenshot:
    width: int
    height: int
    bpp: int
    pixels: bytes  # 16 bpp: RGB565 little endian; 4 bpp: grey, two pixels per byte


# ------------------------------------------------------------------ services
def device_info(s: Session) -> DeviceInfo:
    with s.connect(SID_DEVINFO):
        s.write(b"\x01")
        b = s.read()
        if len(b) < 110:
            raise ProtocolError("short device-info reply")
        (flash_free, flash_total, ram_free, ram_total, batt, _pad, charging, clock, osv, b1, b2, hwv,
         runlevel, _lx, _ly, lw, lh, bpp, _sm, hw, _eid, full_eid) = struct.unpack_from(
            ">QQQQBBBB4s4s4sIHHHHHBBB17s27s", b, 1)
        s.write(b"\x02")
        name = _cstr(s.read()[1:])
        s.write(b"\x03")
        ext = s.read()[1:].split(b"\0")
    return DeviceInfo(
        name=name, os_version=_version(osv), boot1_version=_version(b1), boot2_version=_version(b2),
        hw_type=hw, hw_version=hwv, flash_free=flash_free, flash_total=flash_total, ram_free=ram_free,
        ram_total=ram_total,
        battery={0x00: "powered", 0xF1: "low", 0x7F: "ok", 0xFF: "unknown"}.get(batt, f"0x{batt:02x}"),
        charging=bool(charging), clock_mhz=clock, lcd_width=lw, lcd_height=lh, lcd_bpp=bpp,
        run_level={1: "recovery", 2: "OS"}.get(runlevel, f"{runlevel}"),
        electronic_id=_cstr(full_eid),
        file_extension=ext[0].decode("latin-1") if ext else ".tns",
        os_extension=ext[1].decode("latin-1") if len(ext) > 1 else "")


def screenshot(s: Session) -> Screenshot:
    with s.connect(SID_SCREEN):
        s.write(b"\x00")
        b = s.read()
        size, _, _, w, h, bpp, _ = struct.unpack_from(">IHHHHBB", b, 1)
        raw = bytearray()
        while len(raw) < size:
            raw += s.read()[1:]
        del raw[size:]
    return Screenshot(w, h, bpp, rle_decode(bpp, bytes(raw), w * h * bpp // 8))


def rle_decode(bpp: int, data: bytes, out_size: int) -> bytes:
    unit = bpp * 2 // 8  # runs are counted in pairs of pixels
    out = bytearray()
    i = 0
    while i < len(data) - 1 and len(out) < out_size:
        n = struct.unpack_from("b", data, i)[0]
        i += 1
        if n < 0:
            k = (-n + 1) * unit
            out += data[i:i + k]
            i += k
        else:
            out += data[i:i + unit] * (n + 1)
            i += unit
    return bytes(out[:out_size]) + bytes(max(0, out_size - len(out)))


def rle_encode(bpp: int, pixels: bytes) -> bytes:
    """Inverse of rle_decode (used by the simulator and tests)."""
    unit = bpp * 2 // 8
    units = [pixels[i:i + unit] for i in range(0, len(pixels), unit)]
    out, i = bytearray(), 0
    while i < len(units):
        j = i
        while j + 1 < len(units) and units[j + 1] == units[i] and j - i < 127:
            j += 1
        if j > i:
            out += struct.pack("b", j - i) + units[i]
            i = j + 1
            continue
        j = i
        while j + 1 < len(units) and units[j + 1] != units[j] and j - i < 127:
            j += 1
        out += struct.pack("b", -(j - i + 1) + 1 if j > i else 0)
        if j > i:
            out += b"".join(units[i:j + 1])
        else:
            out += units[i]
        i = j + 1
    return bytes(out)


def _result(b: bytes) -> int:
    return struct.unpack_from(">H", b, 0)[0] if len(b) >= 2 else -1


# FF xx result codes (hackspire / tilibs; 0x15 confirmed on a CX II: non-.tns uploads are refused).
RESULT_MESSAGES = {
    0x02: "the handheld is out of memory", 0x04: "the file is damaged", 0x07: "the path is too long",
    0x09: "the folder doesn't exist", 0x0A: "it doesn't exist", 0x0F: "that folder name isn't allowed",
    0x12: "a folder with that name already exists", 0x13: "the path is too long",
    0x14: "that file name isn't allowed", 0x15: "the handheld only accepts TI-Nspire documents (.tns)",
    0x16: "the name contains characters the handheld doesn't allow",
    0x20: "that OS file isn't for this model", 0x80: "the handheld refuses to install an older OS",
}


def _why(code: int) -> str:
    if code is not None and code >= 0 and (code >> 8) == 0xFF and (code & 0xFF) in RESULT_MESSAGES:
        return RESULT_MESSAGES[code & 0xFF]
    return f"code {code:04x}" if isinstance(code, int) and code >= 0 else "no reply"


def list_dir(s: Session, path: str) -> list[DirEntry]:
    items = []
    with s.connect(SID_FILES):
        s.write(b"\x0D" + padded(path) + b"\0")
        code = s.read()
        if len(code) > 1 and code[1] == 0x0A:
            raise NotFoundError(f"{path} doesn't exist", 0x0A)
        if len(code) > 1 and code[1] == 0x0F:
            raise RemoteError(f"{path} is not a folder", 0x0F)
        while True:
            s.write(b"\x0E")
            b = s.read()
            if b[0] == 0xFF:
                break
            name, off = scan_padded(b, 3)
            size, date, is_dir = struct.unpack_from(">IIB", b, off)
            items.append(DirEntry(name, size, bool(is_dir), date, join(path, name)))
        s.write(b"\x0F")
        s.read()
    return items


def stat(s: Session, path: str) -> DirEntry | None:
    with s.connect(SID_FILES):
        s.write(struct.pack(">H", 0x2001) + padded(path) + b"\0")
        b = s.read()
    if not b or b[0] != 0x20:
        return None
    size, date, is_dir = struct.unpack_from(">IIB", b, 1)
    return DirEntry(basename(path), size, bool(is_dir), date, path)


def read_file(s: Session, path: str, progress=None) -> bytes:
    with s.connect(SID_FILES):
        s.write(struct.pack(">H", 0x0701) + padded(path))
        b = s.read()
        if _result(b) != 0x0301:
            raise NotFoundError(f"{path} doesn't exist", _result(b))
        length = struct.unpack_from(">I", b, 11)[0]
        s.write(b"\x04")
        out = bytearray()
        while len(out) < length:
            out += s.read()[1:]
            if progress:
                progress(min(len(out), length), length)
        s.write(struct.pack(">H", OK))
    return bytes(out[:length])


def write_file(s: Session, path: str, data: bytes, progress=None) -> None:
    with s.connect(SID_FILES):
        s.write(struct.pack(">H", 0x0301) + padded(path) + struct.pack(">I", len(data)))
        b = s.read()
        if not b or b[0] != 0x04:
            code = _result(b)
            raise RemoteError(f"The calculator refused {path}: {_why(code)}", code)
        for i in range(0, len(data), CHUNK):
            s.write(b"\x05" + data[i:i + CHUNK])
            if progress:
                progress(min(i + CHUNK, len(data)), len(data))
        result = _result(s.read(30000))
    if result != OK:
        raise RemoteError(f"Writing {path} failed: {_why(result)}", result)
    if progress and not data:
        progress(0, 0)


def _simple(s: Session, opcode: int, *paths: str, trailing_zero: bool) -> int:
    with s.connect(SID_FILES):
        s.write(struct.pack(">H", opcode) + b"".join(padded(p) for p in paths) + (b"\0" if trailing_zero else b""))
        return _result(s.read())


def make_dir(s: Session, path: str) -> None:
    r = _simple(s, 0x0A03, path, trailing_zero=False)
    if r != OK:
        raise ExistsError(f"Couldn't create {path}: {_why(r)}", r)


def remove_dir(s: Session, path: str) -> None:
    r = _simple(s, 0x0B03, path, trailing_zero=False)
    if r != OK:
        raise RemoteError(f"Couldn't remove the folder {path} (it must exist and be empty): {_why(r)}", r)


def delete_file(s: Session, path: str) -> None:
    r = _simple(s, 0x0901, path, trailing_zero=True)
    if r != OK:
        raise NotFoundError(f"Couldn't delete {path}: {_why(r)}", r)


def move(s: Session, src: str, dst: str) -> None:
    r = _simple(s, 0x2101, src, dst, trailing_zero=True)
    if r != OK:
        raise RemoteError(f"Couldn't move {src} to {dst}: {_why(r)}", r)


def copy_file(s: Session, src: str, dst: str) -> None:
    r = _simple(s, 0x0C01, src, dst, trailing_zero=True)
    if r != OK:
        raise RemoteError(f"Couldn't copy {src} to {dst}: {_why(r)}", r)


def install_os(s: Session, image: bytes, progress=None) -> None:
    """Send an OS image (.tco2/.tcc2/.tct2). The calc verifies the signature itself and reboots."""
    with s.connect(SID_OS):
        s.write(struct.pack(">BI", 0x03, len(image)))
        b = s.read()
        if not b or b[0] != 0x04:
            code = _result(b) if len(b) >= 2 else -1
            raise RemoteError(f"The calculator refused the OS image: {_why(code)}", code)
        # After the first chunk the calc answers FF 00 (header accepted) or 04 ("keep sending"); in the
        # latter case it sends FF 00 later, so keep reading a status per chunk until it does.
        awaiting_header = True
        for i in range(0, len(image), CHUNK):
            s.write(b"\x05" + image[i:i + CHUNK])
            if awaiting_header:
                st = s.read()
                code = _result(st) if len(st) >= 2 else (0x0400 if st[:1] == b"\x04" else -1)
                if code == OK:
                    awaiting_header = False
                elif code != 0x0400 and st[:1] != b"\x04":
                    raise RemoteError(f"The calculator rejected the OS image: {_why(code)}", code)
            if progress:
                progress("upload", min(i + CHUNK, len(image)), len(image))
        while True:
            b = s.read(120000)
            if b[:1] == b"\xff":
                code = _result(b)
                raise RemoteError(f"The calculator stopped the OS install: {_why(code)}", code)
            if progress:
                progress("install", min(b[1], 100), 100)
            if b[1] >= 100:  # libnspire notes it can report over 100%
                break


# --------------------------------------------------------------------- paths
def join(folder: str, name: str) -> str:
    folder = folder.rstrip("/")
    return f"{folder}/{name}" if folder else f"/{name}"


def basename(path: str) -> str:
    return path.rstrip("/").rsplit("/", 1)[-1]


def dirname(path: str) -> str:
    head = path.rstrip("/").rsplit("/", 1)[0]
    return head or "/"
