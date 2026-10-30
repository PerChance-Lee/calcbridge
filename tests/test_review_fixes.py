"""Regression tests for the issues found in the independent review."""
import os

import pytest

from calcbridge import cli
from calcbridge.device import Calculator, Cancelled, local_name
from calcbridge.gui.create_panel import safe_identifier
from calcbridge.protocol.errors import LinkError, RemoteError
from calcbridge.sim import SimCalculator, SimTransport
from calcbridge.textio import read_text, safe_join
from calcbridge.tns import TnsDocument, build_lua, build_python, build_text


class FlakyTransport(SimTransport):
    """Raises a USB write timeout on chosen write numbers."""

    def __init__(self, sim, fail_on):
        super().__init__(sim)
        self.n, self.fail_on = 0, set(fail_on)

    def write(self, data, timeout_ms=1000):
        self.n += 1
        if self.n in self.fail_on:
            raise LinkError("USB write timed out")
        super().write(data, timeout_ms)


# 1. a link reset while waiting for a reply used to wedge the connection
def test_reset_while_waiting_for_reply(calc, sim):
    calc.info()
    sim.replug_after_ack = True
    assert calc.list_dir("/")          # notices the reset, re-handshakes, retries
    assert calc.info().name            # and the link keeps working afterwards
    assert calc.list_dir("/Examples")


# 2. one USB write timeout used to leave the host a sequence number ahead for good
def test_write_timeout_does_not_desync(tmp_path):
    sim = SimCalculator(idle_drop_s=0)
    t = FlakyTransport(sim, fail_on={12, 30})
    c = Calculator(transport=t, state_file=str(tmp_path / "s.json"))
    for _ in range(4):
        assert c.list_dir("/")
    c.close()
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "s.json"))
    assert c.info().name  # the saved sequence number is right too
    c.close()


# 3. copying a folder into itself recursed until the path limit
def test_copy_or_move_folder_into_itself_is_refused(calc, sim):
    calc.mkdir("/A")
    calc.write_file("/A/x.tns", b"x")
    for op in (calc.copy, calc.move):
        with pytest.raises(ValueError):
            op("/A", "/A/A")
    assert [e.name for e in calc.list_dir("/A")] == ["x.tns"]


# 4. a stale Cancel used to abort the next job (the GUI clears it per job; the API raises once)
def test_cancel_is_one_shot(calc):
    calc.cancel_event.set()
    with pytest.raises(Cancelled):
        calc._check_cancel()
    calc._check_cancel()  # cleared


# 5. names inside an untrusted .tns must not escape the chosen folder
@pytest.mark.parametrize("name", ["../../evil.py", "..\\..\\evil.py", "/etc/evil.py", "C:\\Windows\\evil.py",
                                  "C:evil.py", "..", "sub/dir/evil.py"])
def test_safe_join_stays_in_folder(tmp_path, name):
    p = safe_join(str(tmp_path), name)
    assert p is None or os.path.dirname(os.path.abspath(p)) == os.path.abspath(str(tmp_path))


def test_cli_extract_is_safe(tmp_path, capsys):
    doc = tmp_path / "d.tns"
    doc.write_bytes(build_python("ok.py", "print(1)\n", extra={"../escape.py": "x = 1\n"}))
    assert cli.main(["view", str(doc), "--extract", str(tmp_path / "out")]) == 0
    assert not (tmp_path / "escape.py").exists()
    assert (tmp_path / "out" / "ok.py").exists() and (tmp_path / "out" / "escape.py").exists()


# 7. control characters (form feed, NULs from misread UTF-16) made unreadable documents
def test_control_characters_are_safe():
    doc = TnsDocument.from_bytes(build_text("a\x0cb\x00c\td\nsecond"))
    tree = doc.notes_trees()[0][1]
    assert "abc" in tree.replace("\t", "") or "ab" in tree
    lua = TnsDocument.from_bytes(build_lua("x = 1\n\ty = 2\x0c\n"))
    assert "x = 1\n\ty = 2\n" in lua.lua_script()


def test_read_text_handles_utf16_and_bom(tmp_path):
    (tmp_path / "a.txt").write_bytes("héllo\nx".encode("utf-16"))
    (tmp_path / "b.txt").write_bytes(b"\xef\xbb\xbfh\xc3\xa9")
    (tmp_path / "c.txt").write_bytes("hi".encode("utf-16-le"))
    assert read_text(str(tmp_path / "a.txt")) == "héllo\nx"
    assert read_text(str(tmp_path / "b.txt")) == "hé"
    assert read_text(str(tmp_path / "c.txt")) == "hi"


# 8. folder upload / restore sent files the handheld refuses
def test_upload_folder_sends_only_tns(calc, sim, tmp_path):
    d = tmp_path / "pack"
    d.mkdir()
    (d / "a.tns").write_bytes(b"a")
    (d / "notes.txt").write_text("n")
    (d / "desktop.ini").write_text("[x]")
    skipped = []
    calc.upload(str(d), "/", skipped=skipped)
    assert [e.name for e in calc.list_dir("/pack")] == ["a.tns"]
    assert [os.path.basename(p) for p in skipped] == ["notes.txt"]  # OS clutter isn't reported
    with pytest.raises(ValueError):
        calc.upload(str(d / "notes.txt"), "/")


def test_simulator_refuses_non_tns_like_the_handheld(calc):
    with pytest.raises(RemoteError) as e:
        calc.write_file("/x.txt", b"x")
    assert ".tns" in str(e.value)


def test_restore_counts_only_written(calc, sim, tmp_path):
    files = calc.backup(str(tmp_path / "bk"))
    assert files >= 2
    skipped = []
    assert calc.restore(str(tmp_path / "bk"), skipped=skipped) == 0  # everything already there
    assert any(p.endswith("themes.csv") for p in skipped)
    calc.delete("/Examples")
    assert calc.restore(str(tmp_path / "bk")) == 1


# 9. non-ASCII names broke Python conversion
def test_unicode_names_make_valid_python_modules():
    ident = safe_identifier("Résumé 2")
    assert ident.isascii() and ident.isidentifier()
    build_python(ident + ".py", "print(1)")
    assert safe_identifier("123") == "p_123"


# 12. top-level download names weren't sanitised
def test_local_names():
    assert local_name("a:b.tns") == "a_b.tns"
    assert local_name("nul.tns") == "_nul.tns"
    assert local_name("x?.tns") == "x_.tns"
    assert local_name("dots...") == "dots"


# 13. probing only searched upward from the saved number
def test_probe_finds_sequence_below_saved_number(tmp_path):
    import json
    state = tmp_path / "s.json"
    state.write_text(json.dumps({"sim": 900}))
    sim = SimCalculator(fresh_link=False, start_seq=20, idle_drop_s=0)
    c = Calculator(transport=SimTransport(sim), state_file=str(state))
    assert c.info().name
    c.close()


# minor: the CLI couldn't press the * key
def test_cli_key_parsing():
    assert cli.parse_key_arg("*") == ("*", 1)
    assert cli.parse_key_arg("down*3") == ("down", 3)
    assert cli.parse_key_arg("a") == ("a", 1)
