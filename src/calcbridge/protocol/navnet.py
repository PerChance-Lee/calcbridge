"""NavNet packets and service sessions (carried inside NNSE stream messages on the CX II).

Packet (big endian): magic 0x54FD, src_addr, src_sid, dst_addr, dst_sid, data_checksum (2 bytes each),
then data_size, ack, seq, header_checksum (1 byte each), then data. data_size 0xFF means a 4-byte
length follows (packets up to 1440 data bytes on the CX II).
"""
from __future__ import annotations

import logging
import struct

from .errors import LinkError, ProtocolError
from .nnse import Link, LinkResetError

log = logging.getLogger(__name__)

MAGIC = 0x54FD
HDR = struct.Struct(">HHHHHHBBBB")
HOST_ADDR, CALC_ADDR = 0x6400, 0x6401
SID_ADDR_ASSIGN, SID_DISCONNECT = 0x4003, 0x40DE
SID_ACK, SID_ACK_ZERO, SID_NACK = 0xFF, 0xFE, 0xD3
MAX_DATA = 1440


def data_checksum(data: bytes) -> int:
    c = 0
    for b in data:
        t1 = ((b << 8) | (c >> 8)) & 0xFFFF
        c &= 0xFF
        t2 = ((((c & 0xF) << 4) ^ c) << 8) & 0xFFFF
        t3 = t2 >> 5
        c = ((t3 >> 7) ^ t1 ^ t2 ^ t3) & 0xFFFF
    return c


def build(src_sid, dst_sid, data=b"", *, src_addr=HOST_ADDR, dst_addr=CALC_ADDR, ack=0, seq=0) -> bytes:
    if len(data) < 0xFF:
        size, full = len(data), bytes(data)
    elif len(data) <= MAX_DATA:
        size, full = 0xFF, struct.pack(">I", len(data)) + bytes(data)
    else:
        raise ValueError(f"NavNet data too long ({len(data)} > {MAX_DATA})")
    hdr = bytearray(HDR.pack(MAGIC, src_addr, src_sid, dst_addr, dst_sid, data_checksum(full), size, ack, seq, 0))
    hdr[15] = sum(hdr[:15]) & 0xFF
    return bytes(hdr) + full


def parse(raw: bytes) -> dict:
    if len(raw) < HDR.size:
        raise ProtocolError("short NavNet packet")
    magic, sa, ssid, da, dsid, dcs, size, ack, seq, hcs = HDR.unpack_from(raw)
    if magic != MAGIC or (sum(raw[:15]) & 0xFF) != hcs:
        raise ProtocolError("bad NavNet header")
    if size == 0xFF:
        n = struct.unpack_from(">I", raw, 16)[0]
        full, data = raw[16:20 + n], raw[20:20 + n]
    else:
        full = data = raw[16:16 + size]
    if data_checksum(full) != dcs:
        raise ProtocolError("bad NavNet data checksum")
    return dict(src_addr=sa, src_sid=ssid, dst_addr=da, dst_sid=dsid, ack=ack, seq=seq, data=bytes(data))


class Session:
    """A NavNet session: at most one service connection at a time, like libnspire."""

    def __init__(self, link: Link):
        self.link = link
        self.host_sid = SID_ADDR_ASSIGN
        self.device_sid = SID_ADDR_ASSIGN
        self.service = None

    def open(self, progress=None):
        self.host_sid = self.device_sid = SID_ADDR_ASSIGN
        self.link.open(build(self.host_sid, self.device_sid, bytes([0x64, 0x01, 0xFF, 0x00])), progress)
        self.host_sid = 0x8000
        self.service = None

    # ----------------------------------------------------------- packets
    def _send(self, data, src_sid=None, dst_sid=None, ack=0, seq=0):
        self.link.send(build(self.host_sid if src_sid is None else src_sid,
                             self.device_sid if dst_sid is None else dst_sid, data, ack=ack, seq=seq))

    def write(self, data: bytes) -> None:
        self._send(data)

    def read(self, timeout_ms: int = 10000) -> bytes:
        while True:
            p = parse(self.link.recv(timeout_ms))
            if p["dst_sid"] == self.host_sid:
                return p["data"]
            # Something for another service (e.g. a disconnect notice): ack it, nack the rest.
            nack = p["dst_sid"] != SID_DISCONNECT
            self._send(bytes([p["dst_sid"] >> 8, p["dst_sid"] & 0xFF]),
                       src_sid=SID_NACK if nack else (SID_ACK if p["seq"] else SID_ACK_ZERO),
                       dst_sid=p["src_sid"], ack=0x0A, seq=p["seq"])

    # ---------------------------------------------------------- services
    def connect(self, sid: int):
        if self.service is not None:
            raise ProtocolError("a service is already connected")
        self.service = sid
        self.device_sid = sid
        return _ServiceContext(self)

    def disconnect(self):
        if self.service is None:
            return
        try:
            self._send(struct.pack(">H", self.host_sid), src_sid=SID_DISCONNECT)
        finally:
            self.service = None
            self.host_sid = 0x8000 + ((self.host_sid + 1 - 0x8000) % 0x4000)


class _ServiceContext:
    def __init__(self, session):
        self.session = session

    def __enter__(self):
        return self.session

    def __exit__(self, exc_type, exc, tb):
        if isinstance(exc, (LinkResetError,)):
            self.session.service = None  # the calc forgot everything; nothing to disconnect
            return False
        try:
            self.session.disconnect()
        except LinkError:
            if exc is None:
                raise
        return False
