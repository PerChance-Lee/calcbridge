"""NNSE ("NavNet SE"), the link layer the CX II wraps NavNet packets in.

Wire format (big endian), 12-byte header then payload:
    misc  service  src  dest  unknown  reqAck  length(2)  seqno(2)  checksum(2)
The checksum makes the 16-bit ones'-complement sum of the whole message 0xFFFF.

Observed on a real CX II (OS 5.4):
  * The calc accepts our stream packets only with seqno == last+1. seqno == last is acked again as a
    duplicate (and not processed); anything else is silently dropped.
  * It runs the address/time handshake once per USB connection. A later program finds a silent link
    and has to rediscover the sequence number, so we remember it per device between runs.
  * After about 1.5 s of idle it drops the first packet it receives. Retransmitting the same seqno
    gets it through, so every send retries on a short ack timeout.
"""
from __future__ import annotations

import collections
import json
import logging
import os
import struct
import threading
import time

from .errors import LinkError
from .transport import Transport

log = logging.getLogger(__name__)

ADDR_ALL, ADDR_ME, ADDR_CALC = 0xFF, 0xFE, 0x01
SVC_ADDRREQ, SVC_TIME, SVC_STREAM, SVC_UNKNOWN = 0x01, 0x02, 0x04, 0x08
ACK_FLAG = 0x80
REQ_ACK, REQ_ACK_RETRY = 0x01, 0x09

HDR = struct.Struct(">BBBBBBHHH")
MAX_PAYLOAD = 1472
MAX_MESSAGE = HDR.size + MAX_PAYLOAD


class LinkResetError(LinkError):
    """The calculator restarted the link (it was re-plugged or woke up); higher layers must re-init."""


def checksum(data: bytes) -> int:
    acc = 0
    n = len(data)
    for i in range(0, n - 1, 2):
        acc += (data[i] << 8) | data[i + 1]
    if n & 1:
        acc += data[-1] << 8
    while acc >> 16:
        acc = (acc >> 16) + (acc & 0xFFFF)
    return acc


def encode(service, payload=b"", *, src=ADDR_ME, dest=ADDR_CALC, req_ack=0, seqno=0, misc=0, unknown=0) -> bytes:
    msg = bytearray(HDR.pack(misc, service, src, dest, unknown, req_ack, HDR.size + len(payload), seqno, 0))
    msg += payload
    msg[10:12] = struct.pack(">H", checksum(msg) ^ 0xFFFF)
    return bytes(msg)


def decode(buf: bytes):
    """-> dict, or None if the bytes aren't a valid message."""
    if len(buf) < HDR.size:
        return None
    misc, service, src, dest, unknown, req_ack, length, seqno, _ = HDR.unpack_from(buf)
    if length < HDR.size or length > len(buf) or checksum(buf[:length]) != 0xFFFF:
        return None
    return dict(misc=misc, service=service, src=src, dest=dest, unknown=unknown, req_ack=req_ack,
                seqno=seqno, data=bytes(buf[HDR.size:length]))


class SeqStore:
    """Remembers the next sequence number per device, across program runs."""

    def __init__(self, path: str | None):
        self.path = path
        self._lock = threading.Lock()

    def load(self, key: str) -> int | None:
        if not self.path:
            return None
        try:
            with open(self.path, encoding="utf-8") as f:
                return int(json.load(f)[key])
        except (OSError, ValueError, KeyError, TypeError):
            return None

    def save(self, key: str, seq: int) -> None:
        """Best effort: this only speeds up the next connection, so it must never fail an operation.
        (On Windows, antivirus/indexers briefly lock freshly written files and os.replace gets EACCES.)"""
        if not self.path:
            return
        with self._lock:
            try:
                with open(self.path, encoding="utf-8") as f:
                    data = json.load(f)
            except (OSError, ValueError):
                data = {}
            data[key] = seq
            for attempt in range(3):
                try:
                    os.makedirs(os.path.dirname(self.path) or ".", exist_ok=True)
                    tmp = f"{self.path}.{os.getpid()}.tmp"
                    with open(tmp, "w", encoding="utf-8") as f:
                        json.dump(data, f)
                    os.replace(tmp, self.path)
                    return
                except OSError as e:
                    log.debug("couldn't save link state (attempt %d): %s", attempt + 1, e)
                    time.sleep(0.05)


class Link:
    """Reliable stream of NavNet packets over NNSE."""

    ACK_TIMEOUT_MS = 700
    RETRIES = 8
    PROBE_TIMEOUT_MS = 120
    PROBE_LIMIT = 1024
    SAVE_EVERY = 8

    def __init__(self, transport: Transport, store: SeqStore | None = None, trace=None):
        self.t = transport
        self.store = store or SeqStore(None)
        self.trace = trace  # optional callable(str) for packet logs
        self.seqno = 0
        self.ready = False
        self.reset_seen = False
        self._inbox = collections.deque()
        self._last_rx_seq = None
        self._sent_since_save = 0
        self.last_activity = 0.0

    # ------------------------------------------------------------- raw I/O
    def _write(self, service, payload=b"", **kw):
        msg = encode(service, payload, **kw)
        if self.trace:
            self.trace(f"-> svc={service:02x} seq={kw.get('seqno', 0)} ack={kw.get('req_ack', 0)} "
                       f"{payload[:20].hex()}")
        self.t.write(msg)
        self.last_activity = time.monotonic()

    def _read(self, timeout_ms):
        buf = self.t.read(MAX_MESSAGE, timeout_ms)
        if not buf:
            return None
        if len(buf) >= HDR.size:
            length = struct.unpack_from(">H", buf, 6)[0]
            while HDR.size <= length <= MAX_MESSAGE and len(buf) < length:
                more = self.t.read(length - len(buf), 1000)
                if not more:
                    break
                buf += more
        m = decode(buf)
        if m and self.trace:
            self.trace(f"<- svc={m['service']:02x} seq={m['seqno']} ack={m['req_ack']} {m['data'][:20].hex()}")
        return m

    # -------------------------------------------------------- housekeeping
    def _handle(self, m):
        """Ack and answer link-level traffic. Stream payloads go to the inbox."""
        if m["dest"] not in (ADDR_ME, ADDR_ALL) or m["service"] & ACK_FLAG:
            return
        if m["req_ack"] & 1:
            self._write(m["service"] | ACK_FLAG, req_ack=m["req_ack"] & ~1, seqno=m["seqno"],
                        src=m["dest"], dest=m["src"], misc=m["misc"], unknown=m["unknown"])
        svc, data = m["service"] & ~ACK_FLAG, m["data"]
        if svc == SVC_ADDRREQ and data[:1] == b"\x00":
            # A fresh link: the calc's sequence expectations start over. It sometimes drops the first
            # reply after reconnecting, so answer twice like libnspire does.
            if self.ready:
                self.reset_seen = True
            self.seqno = 0
            self._last_rx_seq = None
            self._write(SVC_ADDRREQ, b"\x80", seqno=self._next())
            self._write(SVC_ADDRREQ, b"\x80", seqno=self._next())
        elif svc == SVC_TIME and data[:1] == b"\x00":
            self._write(SVC_TIME, struct.pack(">BIQI", 0x80, int(time.time()), 0, 0), seqno=self._next())
            self.ready = True
        elif svc == SVC_UNKNOWN and data == b"\x01":
            self._write(SVC_UNKNOWN, b"\x81\x03", seqno=self._next())
        elif svc == SVC_STREAM:
            if m["seqno"] == self._last_rx_seq:
                return  # a retransmit of something we already have (our ack got lost)
            self._last_rx_seq = m["seqno"]
            self._inbox.append(data)

    def _next(self):
        s = self.seqno
        self.seqno = (self.seqno + 1) & 0xFFFF
        return s

    def _check_reset(self):
        if self.reset_seen:
            self.reset_seen = False
            raise LinkResetError("The calculator restarted the connection")

    # ---------------------------------------------------------------- open
    def open(self, first_payload: bytes, progress=None) -> None:
        """Bring the link up and deliver the first stream payload (NavNet's address assignment)."""
        # A freshly plugged-in calc repeats address requests; an established link stays silent.
        deadline = time.monotonic() + 1.2
        while time.monotonic() < deadline and not self.ready:
            m = self._read(300)
            if m is None:
                continue
            if m["service"] == SVC_ADDRREQ:
                deadline = max(deadline, time.monotonic() + 3)  # handshake under way: wait for its end
            self._handle(m)
        if self.ready:  # we just did the handshake: sequence numbers start from what it set up
            self.send(first_payload)
            return
        saved = self.store.load(self.t.device_key)
        self.seqno = max(0, (saved or 0) - 4)
        log.info("link silent; probing for the sequence number from %d", self.seqno)
        for attempt in range(self.PROBE_LIMIT):
            if progress and attempt and attempt % 16 == 0:
                progress(attempt, self.PROBE_LIMIT)
            seq = self._next()
            # the first packet after an idle period may be dropped, so the first candidate gets a retry
            for flag in ((REQ_ACK, REQ_ACK_RETRY) if attempt == 0 else (REQ_ACK,)):
                self._write(SVC_STREAM, first_payload, req_ack=flag, seqno=seq)
                if self._wait_ack(seq, self.PROBE_TIMEOUT_MS if attempt else self.ACK_TIMEOUT_MS):
                    self.ready = True
                    self.reset_seen = False
                    self._save(force=True)
                    return
                if self.ready and self.seqno < seq:  # a handshake happened while probing
                    self.send(first_payload)
                    return
        raise LinkError("The calculator isn't answering. Unplug it, plug it back in, and try again.")

    # ---------------------------------------------------------- send/recv
    def _wait_ack(self, seq, timeout_ms) -> bool:
        deadline = time.monotonic() + timeout_ms / 1000
        while True:
            left = int((deadline - time.monotonic()) * 1000)
            if left <= 0:
                return False
            m = self._read(left)
            if m is None:
                return False
            self._handle(m)
            if m["dest"] == ADDR_ME and m["service"] == (SVC_STREAM | ACK_FLAG) and m["seqno"] == seq:
                return True

    def send(self, payload: bytes) -> None:
        if len(payload) > MAX_PAYLOAD:
            raise ValueError("payload too large for one NNSE message")
        self._check_reset()
        seq = self._next()
        for attempt in range(self.RETRIES):
            self._write(SVC_STREAM, payload, req_ack=REQ_ACK if attempt == 0 else REQ_ACK_RETRY, seqno=seq)
            if self._wait_ack(seq, self.ACK_TIMEOUT_MS):
                self._save()
                return
            self._check_reset()
        self.seqno = seq  # never acked, so the calc still expects this number
        self._save(force=True)
        raise LinkError("The calculator stopped answering")

    def recv(self, timeout_ms: int = 10000) -> bytes:
        deadline = time.monotonic() + timeout_ms / 1000
        while not self._inbox:
            self._check_reset()
            left = int((deadline - time.monotonic()) * 1000)
            if left <= 0:
                raise LinkError("Timed out waiting for the calculator")
            m = self._read(min(left, 1000))
            if m is not None:
                self._handle(m)
        self._check_reset()
        return self._inbox.popleft()

    def _save(self, force=False):
        self._sent_since_save += 1
        if force or self._sent_since_save >= self.SAVE_EVERY:
            self._sent_since_save = 0
            self.store.save(self.t.device_key, self.seqno)

    def close(self) -> None:
        try:
            self.store.save(self.t.device_key, self.seqno)
        finally:
            self.t.close()
