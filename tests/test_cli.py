import os

import pytest

from calcbridge import cli


def run(capsys, *args):
    code = cli.main(list(args))
    out = capsys.readouterr().out
    return code, out


def test_info_and_ls(capsys):
    code, out = run(capsys, "--sim", "info")
    assert code == 0 and "Simulated CX II" in out and "5.40.259" in out
    code, out = run(capsys, "--sim", "ls", "-r")
    assert code == 0 and "/Examples/Getting Started.tns" in out


@pytest.mark.skipif(os.name != "nt", reason="Git Bash path mangling only happens on Windows")
def test_git_bash_mangled_paths_are_repaired(capsys):
    assert cli.remote("C:/Program Files/Git/Examples") == "/Examples"
    assert cli.remote("C:/Program Files/Git/") == "/"
    code, out = run(capsys, "--sim", "ls", "C:/Program Files/Git/Examples")
    assert code == 0 and "Getting Started.tns" in out


def test_missing_path_is_an_error(capsys):
    code = cli.main(["--sim", "get", "/nope.tns", "x.tns"])
    assert code == 1


def test_build_and_view_offline(tmp_path, capsys):
    src = tmp_path / "prog.py"
    src.write_text("print(6*7)\n")
    code, out = run(capsys, "build", str(src))
    assert code == 0 and (tmp_path / "prog.tns").exists()
    code, out = run(capsys, "view", str(tmp_path / "prog.tns"), "--extract", str(tmp_path / "x"))
    assert code == 0 and "Python editor" in out
    assert (tmp_path / "x" / "prog.py").read_text() == "print(6*7)\n"
    notes = tmp_path / "n.md"
    notes.write_text("$\\frac{1}{2}$\n")
    code, _ = run(capsys, "build", str(notes))
    assert code == 0 and (tmp_path / "n.tns").exists()


def test_screenshot(tmp_path, capsys, qapp):
    for name in ("s.png", "s.jpg"):
        out_img = tmp_path / name
        code, out = run(capsys, "--sim", "shot", str(out_img))
        assert code == 0 and out_img.stat().st_size > 1000
