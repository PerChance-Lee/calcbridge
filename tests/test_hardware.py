"""End-to-end checks against a real TI-Nspire CX II. Opt in with:  pytest --hardware

Everything happens inside a scratch folder (/calcbridge-selftest-<time>) that is removed afterwards;
nothing else on the handheld is written to. OS install is never exercised here.
"""
import os
import time

import pytest

from calcbridge.device import Calculator
from calcbridge.protocol.errors import ExistsError, NotFoundError, RemoteError

pytestmark = pytest.mark.hardware


@pytest.fixture(scope="module")
def hw():
    from calcbridge.protocol.nnse import Link

    # undo the short test timeouts from conftest for real hardware
    Link.ACK_TIMEOUT_MS, Link.PROBE_TIMEOUT_MS = 700, 120
    c = Calculator()
    root = f"/calcbridge-selftest-{int(time.time())}"
    c.mkdir(root)
    yield c, root
    try:
        if c.exists(root):
            c.delete(root)
    finally:
        c.close()


@pytest.fixture(autouse=True)
def real_timeouts(monkeypatch):
    from calcbridge.protocol.nnse import Link

    monkeypatch.setattr(Link, "ACK_TIMEOUT_MS", 700)
    monkeypatch.setattr(Link, "PROBE_TIMEOUT_MS", 120)


def test_info(hw):
    c, _ = hw
    i = c.info()
    assert i.name and i.os_version.count(".") == 2
    assert 0 < i.flash_free <= i.flash_total
    assert (i.lcd_width, i.lcd_height) == (320, 240)
    assert i.file_extension == ".tns"


def test_root_listing(hw):
    c, root = hw
    names = {e.name for e in c.list_dir("/")}
    assert root.strip("/") in names
    assert "MyLib" in names and "PyLib" in names


@pytest.mark.parametrize("size", [1, 1438, 1439, 1440, 5000, 100_000])
def test_roundtrip(hw, size):
    c, root = hw
    data = os.urandom(size)
    path = f"{root}/rt{size}.tns"
    c.write_file(path, data)
    assert c.stat(path).size == size
    assert c.read_file(path) == data


def test_zero_byte_file(hw):
    c, root = hw
    try:
        c.write_file(f"{root}/empty.tns", b"")
    except RemoteError:
        pytest.skip("this OS refuses empty files")
    assert c.read_file(f"{root}/empty.tns") == b""


def test_overwrite_and_missing(hw):
    c, root = hw
    c.write_file(f"{root}/o.tns", b"one")
    with pytest.raises(ExistsError):
        c.write_file(f"{root}/o.tns", b"two")
    c.write_file(f"{root}/o.tns", b"two!", overwrite=True)
    assert c.read_file(f"{root}/o.tns") == b"two!"
    with pytest.raises(NotFoundError):
        c.read_file(f"{root}/missing.tns")
    assert c.stat(f"{root}/missing.tns") is None


def test_folder_ops_copy_move_rename(hw):
    c, root = hw
    c.mkdir(f"{root}/A/B", parents=True)
    c.write_file(f"{root}/A/B/f.tns", b"f" * 3000)
    c.write_file(f"{root}/A/g.tns", b"g")
    c.copy(f"{root}/A/g.tns", f"{root}/A/g copy.tns")  # on-device copy (0x0C01)
    assert c.read_file(f"{root}/A/g copy.tns") == b"g"
    c.copy(f"{root}/A", f"{root}/A2")  # recursive copy
    assert c.read_file(f"{root}/A2/B/f.tns") == b"f" * 3000
    assert c.rename(f"{root}/A2/g.tns", "h.tns") == f"{root}/A2/h.tns"
    c.move(f"{root}/A2/h.tns", f"{root}/A/B/h.tns")
    assert {e.name for e in c.list_dir(f"{root}/A/B")} == {"f.tns", "h.tns"}
    c.delete(f"{root}/A2")
    assert c.stat(f"{root}/A2") is None


def test_names_with_spaces_and_unicode(hw):
    c, root = hw
    for name in ("with space.tns", "Ünïcødé ε.tns"):
        c.write_file(f"{root}/{name}", b"n")
        assert name in {e.name for e in c.list_dir(root)}


def test_survives_idle(hw):
    c, root = hw
    c.info()
    time.sleep(2.5)  # the calc drops the first packet after ~1.5 s idle
    assert c.list_dir(root) is not None
    time.sleep(2.5)
    assert c.screenshot().width == 320


def test_screenshot(hw):
    c, _ = hw
    s = c.screenshot()
    assert (s.width, s.height, s.bpp) == (320, 240, 16) and len(s.pixels) == 320 * 240 * 2


def test_backup_everything_and_restore_into_scratch(hw, tmp_path):
    c, root = hw
    bk = tmp_path / "backup"
    n = c.backup(str(bk))  # read-only on the handheld
    assert n >= 1
    files = [e for e in c.walk("/") if not e.is_dir and not e.path.startswith(root)]
    sample = files[0]
    local = bk.joinpath(*sample.path.strip("/").split("/"))
    assert local.read_bytes() == c.read_file(sample.path)
    # restore one backed-up folder into the scratch area only
    src = bk / "MyLib" if (bk / "MyLib").is_dir() else local.parent
    restored = c.restore(str(src), f"{root}/restored")
    assert restored == len([p for p in src.rglob("*") if p.is_file()])
    assert {e.name for e in c.list_dir(f"{root}/restored")} >= {p.name for p in src.iterdir()}


def test_download_upload_tree(hw, tmp_path):
    c, root = hw
    src = tmp_path / "tree"
    (src / "sub").mkdir(parents=True)
    (src / "a.tns").write_bytes(os.urandom(2000))
    (src / "sub" / "b.tns").write_bytes(os.urandom(7000))
    c.upload(str(src), root)
    out = tmp_path / "back"
    c.download(f"{root}/tree", str(out))
    assert (out / "a.tns").read_bytes() == (src / "a.tns").read_bytes()
    assert (out / "sub" / "b.tns").read_bytes() == (src / "sub" / "b.tns").read_bytes()
