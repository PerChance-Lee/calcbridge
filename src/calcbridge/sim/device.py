"""Device side of NNSE + NavNet with an in-memory file system.

It reproduces the behaviour seen on a real CX II (OS 5.4): the one-time address/time handshake, the
"only last+1" sequence rule with duplicate re-acks, dropping the first packet after ~1.5 s idle, and
the reply formats of each service. Pass it to Calculator(transport=SimTransport(sim)).
"""
from __future__ import annotations

import collections
import random
import struct
import threading
import time

from ..protocol import navnet, nnse
from ..protocol.services import padded, rle_encode, scan_padded

OK = b"\xff\x00"
MISSING = b"\xff\x0a"


class _Node:
    def __init__(self, is_dir, data=b""):
        self.is_dir = is_dir
        self.data = data
        self.children = {} if is_dir else None


class SimCalculator:
    def __init__(self, *, fresh_link=True, start_seq=0, idle_drop_s=1.5, drop_rate=0.0, name="Simulated CX II",
                 seed=1, os_version=(5, 40, 1, 3)):
        self.name = name
        self.os_version = os_version
        self.fresh_link = fresh_link
        self.handshake_done = not fresh_link
        self.expected = start_seq  # next host seqno we accept
        self.idle_drop_s = idle_drop_s
        self.drop_rate = drop_rate
        self.rng = random.Random(seed)
        self.calc_seq = 1000
        self.last_rx = time.monotonic()
        self.out = collections.deque()
        self.lock = threading.Lock()
        self.root = _Node(True)
        self.sessions = {}  # host_sid -> state dict
        self.flash_total = 96_862_208
        self.os_installed = None
        self.plugged = True
        self.screen = self._default_screen()
        self.stats = collections.Counter()
        self.keys_pressed = []
        self.replug_after_ack = False
        for folder in ("MyLib", "MyWidgets", "PyLib", "Examples"):
            self.root.children[folder] = _Node(True)
        self.put("/Examples/Getting Started.tns", b"*TIMLP0500" + bytes(200))
        self.put("/themes.csv", b"eWindowTitleBarBackground,0xFFFFFF,0xFF0000\n")

    # --------------------------------------------------------------- fs
    def _walk(self, path, create_parents=False):
        parts = [p for p in path.split("/") if p]
        node = self.root
        for p in parts[:-1]:
            nxt = node.children.get(p) if node.is_dir else None
            if nxt is None:
                if not create_parents:
                    return None, None
                nxt = node.children[p] = _Node(True)
            node = nxt
        return node, (parts[-1] if parts else "")

    def get(self, path):
        if path.strip("/") == "":
            return self.root
        parent, name = self._walk(path)
        if parent is None or not parent.is_dir:
            return None
        return parent.children.get(name)

    def put(self, path, data):
        parent, name = self._walk(path, create_parents=True)
        parent.children[name] = _Node(False, data)

    def used(self):
        def size(n):
            return len(n.data) if not n.is_dir else sum(size(c) for c in n.children.values())
        return size(self.root)

    def _default_screen(self):
        w, h = 320, 240
        px = bytearray()
        for y in range(h):
            for x in range(w):
                r, g, b = (x * 31 // w), (y * 63 // h), 20 if (x // 40 + y // 40) % 2 else 8
                px += struct.pack("<H", (r << 11) | (g << 5) | b)  # little-endian RGB565, like the CX II
        return bytes(px)

    # -------------------------------------------------------- transport
    def unplug(self):
        self.plugged = False

    def replug(self):
        """Simulate re-plugging: the calc restarts the link and asks for an address again."""
        self.plugged = True
        self.handshake_done = False
        self.fresh_link = True
        self.sessions.clear()

    def host_write(self, data: bytes):
        with self.lock:
            if not self.plugged:
                raise OSError("unplugged")
            m = nnse.decode(data)
            if m is None:
                return
            now = time.monotonic()
            idle = now - self.last_rx
            self.last_rx = now
            if m["service"] & nnse.ACK_FLAG:
                return  # host acking our packets; we don't retransmit, so nothing to do
            svc = m["service"]
            if svc == nnse.SVC_ADDRREQ and m["data"][:1] == b"\x80":
                if not self.handshake_done and self.fresh_link:
                    self.fresh_link = False
                    self._send(nnse.SVC_UNKNOWN, b"\x01", req_ack=1)
                    self._send(nnse.SVC_TIME, b"\x00")
                return
            if svc == nnse.SVC_TIME:
                self.handshake_done = True
                self.expected = (m["seqno"] + 1) & 0xFFFF
                return
            if svc == nnse.SVC_UNKNOWN:
                return
            if svc != nnse.SVC_STREAM or not self.handshake_done:
                return
            if self.idle_drop_s and idle > self.idle_drop_s:
                self.stats["idle_drops"] += 1
                return
            if self.drop_rate and self.rng.random() < self.drop_rate:
                self.stats["random_drops"] += 1
                return
            seq = m["seqno"]
            if seq == ((self.expected - 1) & 0xFFFF):
                self._ack(m)  # duplicate: ack again, don't process
                return
            if seq != self.expected:
                self.stats["out_of_order"] += 1
                return
            self.expected = (self.expected + 1) & 0xFFFF
            self._ack(m)
            if self.replug_after_ack:  # test hook: restart the link while the host waits for a reply
                self.replug_after_ack = False
                self.replug()
                return
            self._navnet(m["data"])

    def host_read(self):
        with self.lock:
            if not self.plugged:
                raise OSError("unplugged")
            if not self.out and self.fresh_link and not self.handshake_done:
                self._send(nnse.SVC_ADDRREQ, b"\x00" + bytes(64), dest=nnse.ADDR_ALL, src=nnse.ADDR_ALL)
            return self.out.popleft() if self.out else None

    def _ack(self, m):
        self.out.append(nnse.encode(m["service"] | nnse.ACK_FLAG, src=nnse.ADDR_CALC, dest=nnse.ADDR_ME,
                                    seqno=m["seqno"]))

    def _send(self, service, payload, req_ack=0, src=nnse.ADDR_CALC, dest=nnse.ADDR_ME):
        self.calc_seq = (self.calc_seq + 1) & 0xFFFF
        self.out.append(nnse.encode(service, payload, src=src, dest=dest, req_ack=req_ack, seqno=self.calc_seq))

    # ------------------------------------------------------------ navnet
    def _reply(self, p, data):
        pkt = navnet.build(p["dst_sid"], p["src_sid"], data, src_addr=navnet.CALC_ADDR, dst_addr=navnet.HOST_ADDR)
        self._send(nnse.SVC_STREAM, pkt, req_ack=1)

    def _navnet(self, raw):
        p = navnet.parse(raw)
        dst, src = p["dst_sid"], p["src_sid"]
        if dst == navnet.SID_ADDR_ASSIGN or src in (navnet.SID_ACK, navnet.SID_ACK_ZERO, navnet.SID_NACK):
            return  # the host acking/refusing one of our packets isn't a request
        self.stats[f"request_{dst:04x}"] += 1
        if dst == navnet.SID_DISCONNECT:
            host = struct.unpack(">H", p["data"][:2])[0]
            self.sessions.pop(host, None)
            return
        st = self.sessions.setdefault(src, {"service": dst})
        handler = {0x4020: self._devinfo, 0x4024: self._screen, 0x4060: self._files, 0x4080: self._os,
                   0x4042: self._key}.get(dst)
        if handler is None:
            return
        handler(p, st)

    def _devinfo(self, p, st):
        cmd = p["data"][0]
        if cmd == 1:
            used = self.used()
            body = struct.pack(">QQQQBBBB4s4s4sIHHHHHBBB17s27s",
                               self.flash_total - used, self.flash_total, 32_000_000, 36_220_968,
                               0xFF, 0, 0, 32, bytes(self.os_version), bytes([5, 0, 0, 42]),
                               bytes([5, 13, 0, 110]), 4, 4, 0, 0, 320, 240, 16, 1, 0x1D,
                               b"SIM0000000000000", b"SIM000000000000000000000000")
            self._reply(p, b"\x01" + body)
        elif cmd == 2:
            self._reply(p, b"\x02" + self.name.encode() + b"\0")
        elif cmd == 3:
            self._reply(p, b"\x03.tns\0.tco2\0")

    def _screen(self, p, st):
        if getattr(self, "_rle_cache", (None,))[0] is not self.screen:
            self._rle_cache = (self.screen, rle_encode(16, self.screen))
        data = self._rle_cache[1]
        self._reply(p, b"\x00" + struct.pack(">IHHHHBB", len(data), 0, 0, 320, 240, 16, 0))
        for i in range(0, len(data), 1439):
            self._reply(p, b"\x00" + data[i:i + 1439])

    def _files(self, p, st):
        d = p["data"]
        mode = st.get("mode")
        if mode == "write":
            if d[:1] == b"\x05":
                st["buf"] += d[1:]
                if len(st["buf"]) >= st["size"]:
                    self.put(st["path"], bytes(st["buf"][:st["size"]]))
                    st["mode"] = None
                    self._reply(p, OK)
                return
        if mode == "read" and d[:1] == b"\x04":
            data = st["data"]
            for i in range(0, len(data), 1439):
                self._reply(p, b"\x05" + data[i:i + 1439])
            st["mode"] = None
            return
        if mode == "read" and d[:2] == OK:
            st["mode"] = None
            return
        if mode == "list":
            if d[:1] == b"\x0E":
                if st["items"]:
                    name, node = st["items"].pop(0)
                    rest = padded(name) + struct.pack(">IIB", 0 if node.is_dir else len(node.data), 2,
                                                      int(node.is_dir)) + b"\0"
                    self._reply(p, b"\x10\x00" + bytes([len(rest) & 0xFF]) + rest)
                else:
                    self._reply(p, b"\xff\x00")
                return
            if d[:1] == b"\x0F":
                st["mode"] = None
                self._reply(p, OK)
                return
        op = d[0]
        if op == 0x0D:
            path, _ = scan_padded(d, 1)
            node = self.get(path)
            if node is None:
                self._reply(p, MISSING)
            elif not node.is_dir:
                self._reply(p, b"\xff\x0f")
            else:
                st.update(mode="list", items=list(node.children.items()))
                self._reply(p, OK)
            return
        code = struct.unpack(">H", d[:2])[0]
        if code == 0x2001:
            path, _ = scan_padded(d, 2)
            node = self.get(path)
            if node is None:
                self._reply(p, MISSING)
            else:
                self._reply(p, b"\x20" + struct.pack(">IIB", 0 if node.is_dir else len(node.data), 2,
                                                     int(node.is_dir)) + b"\0")
        elif code == 0x0701:
            path, _ = scan_padded(d, 2)
            node = self.get(path)
            if node is None or node.is_dir:
                self._reply(p, MISSING)
            else:
                st.update(mode="read", data=node.data)
                self._reply(p, struct.pack(">H", 0x0301) + bytes(9) + struct.pack(">I", len(node.data)))
        elif code == 0x0301:
            path, off = scan_padded(d, 2)
            size = struct.unpack_from(">I", d, off)[0]
            parent, name = self._walk(path)
            if parent is None or not parent.is_dir or not name or size > self.flash_total - self.used():
                self._reply(p, MISSING)
                return
            if not name.lower().endswith(".tns"):
                self._reply(p, b"\xff\x15")  # like the real CX II: only .tns documents are accepted
                return
            st.update(mode="write", path=path, size=size, buf=bytearray())
            self._reply(p, b"\x04")
            if size == 0:
                self.put(path, b"")
                st["mode"] = None
                self._reply(p, OK)
        elif code == 0x0A03:
            path, _ = scan_padded(d, 2)
            parent, name = self._walk(path)
            if parent is None or name in parent.children:
                self._reply(p, MISSING)
            else:
                parent.children[name] = _Node(True)
                self._reply(p, OK)
        elif code in (0x0B03, 0x0901):
            path, _ = scan_padded(d, 2)
            parent, name = self._walk(path)
            node = parent.children.get(name) if parent is not None else None
            want_dir = code == 0x0B03
            if node is None or node.is_dir != want_dir or (want_dir and node.children):
                self._reply(p, MISSING)
            else:
                del parent.children[name]
                self._reply(p, OK)
        elif code in (0x2101, 0x0C01):
            src, off = scan_padded(d, 2)
            dst, _ = scan_padded(d, off)
            sp, sn = self._walk(src)
            dp, dn = self._walk(dst)
            node = sp.children.get(sn) if sp is not None else None
            if node is None or dp is None or dn in dp.children or (code == 0x0C01 and node.is_dir):
                self._reply(p, MISSING)
                return
            if code == 0x2101:
                del sp.children[sn]
                dp.children[dn] = node
            else:
                dp.children[dn] = _Node(False, node.data)
            self._reply(p, OK)
        else:
            self._reply(p, b"\xff\x0f")

    def _key(self, p, st):
        d = p["data"]
        if len(d) == 26 and d[0] == 0:  # the key event (the 01 00 00 80 packet before it carries no key)
            self.keys_pressed.append((d[6] << 16) | (d[8] << 8) | d[24])

    def _os(self, p, st):
        d = p["data"]
        if d[0] == 0x03:
            st.update(mode="os", size=struct.unpack(">I", d[1:5])[0], buf=bytearray(), first=True)
            self._reply(p, b"\x04")
        elif d[0] == 0x05 and st.get("mode") == "os":
            st["buf"] += d[1:]
            if st["first"]:
                st["first"] = False
                # stand-in for the real signature check: accept images that start like our test images
                ok = bytes(st["buf"]).startswith(b"TI-Nspire")
                self._reply(p, OK if ok else b"\xff\x04")  # FF 04: damaged / wrong file
                if not ok:
                    st["mode"] = None
                    return
            if len(st["buf"]) >= st["size"]:
                for pct in (0, 25, 50, 75, 100):
                    self._reply(p, bytes([0x00, pct]))
                self.os_installed = bytes(st["buf"])
                st["mode"] = None


class SimTransport:
    """Transport that talks to a SimCalculator in-process."""

    def __init__(self, sim: SimCalculator, key="sim"):
        self.sim = sim
        self.key = key

    @property
    def device_key(self):
        return self.key

    def write(self, data, timeout_ms=1000):
        from ..protocol.errors import NoDeviceError
        try:
            self.sim.host_write(bytes(data))
        except OSError as e:
            raise NoDeviceError(str(e)) from e

    def read(self, size, timeout_ms):
        from ..protocol.errors import NoDeviceError
        deadline = time.monotonic() + timeout_ms / 1000
        while True:
            try:
                msg = self.sim.host_read()
            except OSError as e:
                raise NoDeviceError(str(e)) from e
            if msg is not None:
                return msg[:size]
            if time.monotonic() >= deadline:
                return None
            time.sleep(min(0.005, max(0.0, deadline - time.monotonic())))

    def close(self):
        pass
