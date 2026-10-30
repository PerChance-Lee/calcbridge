import os
import random
import struct
import time

import pytest

from calcbridge.device import Calculator, Cancelled
from calcbridge.protocol import navnet, nnse
from calcbridge.protocol.errors import ExistsError, LinkError, NotFoundError, RemoteError
from calcbridge.protocol.services import rle_decode, rle_encode
from calcbridge.sim import SimCalculator, SimTransport


# ------------------------------------------------------------ encodings
def test_navnet_address_packet_matches_real_hardware_capture():
    # Logged from a real CX II: the host's first NavNet packet (address assignment).
    pkt = navnet.build(0x4003, 0x4003, bytes([0x64, 0x01, 0xFF, 0x00]))
    assert pkt.hex() == "54fd64004003640140031343040000fa6401ff00"


def test_navnet_device_info_request_matches_capture():
    assert navnet.build(0x8000, 0x4020, b"\x01").hex() == "54fd64008000640140200100010000fc01"


def test_navnet_roundtrip_small_and_big():
    for n in (0, 1, 254, 255, 1000, 1440):
        data = bytes(random.Random(n).randrange(256) for _ in range(n))
        p = navnet.parse(navnet.build(0x8001, 0x4060, data))
        assert p["data"] == data and p["src_sid"] == 0x8001 and p["dst_sid"] == 0x4060
    with pytest.raises(ValueError):
        navnet.build(1, 2, bytes(1441))


def test_navnet_rejects_corruption():
    pkt = bytearray(navnet.build(0x8000, 0x4020, b"hello"))
    pkt[-1] ^= 1
    with pytest.raises(Exception):
        navnet.parse(bytes(pkt))


def test_nnse_checksum_and_roundtrip():
    msg = nnse.encode(nnse.SVC_STREAM, b"abc", req_ack=1, seqno=513)
    assert nnse.checksum(msg) == 0xFFFF
    m = nnse.decode(msg)
    assert m["seqno"] == 513 and m["data"] == b"abc" and m["req_ack"] == 1
    bad = bytearray(msg)
    bad[-1] ^= 0xFF
    assert nnse.decode(bytes(bad)) is None


@pytest.mark.parametrize("seed", range(5))
def test_rle_roundtrip(seed):
    rng = random.Random(seed)
    px = bytearray()
    while len(px) < 320 * 240 * 2:
        unit = struct.pack("<HH", rng.choice([0, 0xFFFF, rng.randrange(65536)]), rng.randrange(65536))
        px += unit * rng.choice([1, 1, 2, 50, 200])
    px = bytes(px[:320 * 240 * 2])
    assert rle_decode(16, rle_encode(16, px), len(px)) == px


# ------------------------------------------------------- link behaviour
def test_fresh_link_handshake_and_info(calc, sim):
    info = calc.info()
    assert info.name == "Simulated CX II"
    assert info.os_version == "5.40.259"
    assert info.lcd_width == 320 and info.flash_total == sim.flash_total
    assert info.file_extension == ".tns" and info.os_extension == ".tco2"


def test_rediscovers_sequence_on_silent_link(tmp_path):
    sim = SimCalculator(fresh_link=False, start_seq=37, idle_drop_s=0)
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
    assert [e.name for e in c.list_dir("/")]
    c.close()
    first_run = sim.stats["out_of_order"]
    assert first_run >= 36  # probed up from 0
    # the second run starts just behind the saved number instead of probing from zero
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
    assert sim.stats["out_of_order"] - first_run <= 4
    before = sim.stats["out_of_order"]
    assert c.info().name
    c.close()
    assert sim.stats["out_of_order"] == before


def test_survives_idle_drop(tmp_path):
    sim = SimCalculator(idle_drop_s=0.3)
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
    c.info()
    time.sleep(0.4)
    assert c.list_dir("/")
    time.sleep(0.4)
    assert c.screenshot().width == 320
    assert sim.stats["idle_drops"] >= 2
    c.close()


def test_survives_random_packet_loss(tmp_path):
    sim = SimCalculator(idle_drop_s=0, drop_rate=0.15, seed=7)
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
    data = os.urandom(20000)
    c.write_file("/lossy.tns", data)
    assert c.read_file("/lossy.tns") == data
    assert sim.stats["random_drops"] > 0
    c.close()


def test_reconnects_after_replug(calc, sim):
    calc.info()
    sim.replug()
    # the next call notices the new handshake, reopens the session and retries
    assert calc.list_dir("/")


def test_stray_packet_for_old_session_is_refused_once(calc, sim):
    # A leftover packet for a session that already ended must be nacked, not loop or break the next call.
    calc.info()
    stray = navnet.build(0x4024, 0x8000, b"\x00leftover", src_addr=navnet.CALC_ADDR, dst_addr=navnet.HOST_ADDR)
    sim._send(nnse.SVC_STREAM, stray, req_ack=1)
    before = sim.stats["request_4024"]
    assert calc.list_dir("/")
    assert sim.stats["request_4024"] == before


def test_unplugged_raises(calc, sim):
    calc.info()
    sim.unplug()
    with pytest.raises(Exception):
        calc.info()


# --------------------------------------------------------------- files
def test_list_root(calc):
    names = {e.name: e for e in calc.list_dir("/")}
    assert {"MyLib", "MyWidgets", "PyLib", "Examples", "themes.csv"} <= set(names)
    assert names["MyLib"].is_dir and not names["themes.csv"].is_dir
    assert names["themes.csv"].path == "/themes.csv"


@pytest.mark.parametrize("size", [0, 1, 1438, 1439, 1440, 2878, 5000, 150_000])
def test_write_read_roundtrip(calc, size):
    data = os.urandom(size)
    calc.write_file(f"/rt{size}.tns", data)
    assert calc.read_file(f"/rt{size}.tns") == data
    assert calc.stat(f"/rt{size}.tns").size == size


def test_progress_reports_reach_total(calc):
    seen = []
    data = os.urandom(10000)
    calc.write_file("/p.tns", data, progress=lambda done, total: seen.append((done, total)))
    assert seen[-1] == (10000, 10000)
    seen.clear()
    calc.read_file("/p.tns", progress=lambda done, total: seen.append((done, total)))
    assert seen[-1] == (10000, 10000)


def test_overwrite_protection(calc):
    calc.write_file("/a.tns", b"one")
    with pytest.raises(ExistsError):
        calc.write_file("/a.tns", b"two")
    calc.write_file("/a.tns", b"two", overwrite=True)
    assert calc.read_file("/a.tns") == b"two"


def test_missing_file_errors(calc):
    with pytest.raises(NotFoundError):
        calc.read_file("/nope.tns")
    with pytest.raises(NotFoundError):
        calc.list_dir("/nope")
    with pytest.raises(NotFoundError):
        calc.delete("/nope.tns")
    assert calc.stat("/nope.tns") is None


def test_folders_rename_move_copy_delete(calc):
    calc.mkdir("/T/sub/deep", parents=True)
    calc.write_file("/T/sub/deep/x.tns", b"x")
    calc.write_file("/T/y.tns", b"y")
    assert calc.rename("/T/y.tns", "z.tns") == "/T/z.tns"
    calc.move("/T/z.tns", "/T/sub/z.tns")
    calc.copy("/T/sub", "/T/sub2")
    assert calc.read_file("/T/sub2/deep/x.tns") == b"x"
    assert calc.read_file("/T/sub2/z.tns") == b"y"
    with pytest.raises(ExistsError):
        calc.copy("/T/sub", "/T/sub2")
    with pytest.raises(RemoteError):
        calc._call(__import__("calcbridge.protocol.services", fromlist=["x"]).remove_dir, "/T/sub")
    calc.delete("/T")
    assert calc.stat("/T") is None


def test_walk(calc):
    calc.mkdir("/W/a", parents=True)
    calc.write_file("/W/a/f.tns", b"1")
    paths = [e.path for e in calc.walk("/W")]
    assert paths == ["/W/a", "/W/a/f.tns"]


def test_bad_names_rejected(calc):
    for bad in ("a/b.tns", "a:b.tns", "", "x?.tns"):
        with pytest.raises(ValueError):
            calc.write_file("/" + bad if "/" not in bad else "/q/" + bad.split("/")[-1].replace("b", "b?"), b"")


def test_upload_download_trees(calc, tmp_path):
    src = tmp_path / "src"
    (src / "inner").mkdir(parents=True)
    (src / "one.tns").write_bytes(b"1" * 3000)
    (src / "inner" / "two.tns").write_bytes(b"2" * 10)
    remote = calc.upload(str(src), "/")
    assert remote == "/src"
    out = tmp_path / "out"
    n = calc.download("/src", str(out))
    assert n == 3010
    assert (out / "inner" / "two.tns").read_bytes() == b"2" * 10


def test_backup_and_restore(calc, sim, tmp_path):
    calc.mkdir("/B/c", parents=True)
    calc.write_file("/B/c/doc.tns", b"doc")
    files = calc.backup(str(tmp_path / "bk"))
    assert files >= 3 and (tmp_path / "bk" / "B" / "c" / "doc.tns").read_bytes() == b"doc"
    calc.delete("/B")
    # only the missing .tns comes back: themes.csv isn't a document and Getting Started is still there
    assert calc.restore(str(tmp_path / "bk")) == 1
    assert calc.read_file("/B/c/doc.tns") == b"doc"


def test_cancel_between_items(calc, tmp_path):
    for i in range(5):
        calc.write_file(f"/c{i}.tns", b"x")
    calc.cancel_event.set()
    with pytest.raises(Cancelled):
        calc.backup(str(tmp_path / "bk"))


def test_screenshot(calc):
    s = calc.screenshot()
    assert (s.width, s.height, s.bpp) == (320, 240, 16)
    assert len(s.pixels) == 320 * 240 * 2


def test_os_install_on_simulator(calc, sim):
    image = b"TI-Nspire.tco2" + os.urandom(5000)
    stages = []
    calc.install_os(image, progress=lambda stage, done, total: stages.append(stage))
    assert sim.os_installed == image
    assert "upload" in stages and "install" in stages


def test_os_install_rejects_tiny_file(calc):
    with pytest.raises(RemoteError):
        calc.install_os(b"short")


def test_link_error_when_calc_silent(tmp_path, monkeypatch):
    monkeypatch.setattr("calcbridge.protocol.nnse.Link.PROBE_LIMIT", 3)
    sim = SimCalculator(fresh_link=False, start_seq=500, idle_drop_s=0)
    with pytest.raises(LinkError):
        Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
