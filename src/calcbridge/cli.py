"""Command-line interface.  calcbridge-cli <command> ...   (or: python -m calcbridge cli <command> ...)

Run from PowerShell/cmd/Terminal; Git Bash on Windows rewrites a bare "/" argument into a Windows path.
"""
from __future__ import annotations

import argparse
import os
import re
import sys
import time

from . import __version__


def _calc(args):
    from .device import Calculator
    if args.sim:
        from .sim import SimCalculator, SimTransport
        return Calculator(transport=SimTransport(SimCalculator(idle_drop_s=0)))
    return Calculator()


def _size(n):
    return f"{n:,}"


def parse_key_arg(arg: str) -> tuple[str, int]:
    """"down*3" -> ("down", 3); a plain "*" (multiply) or "**2"-like text is just a key."""
    name, sep, n = arg.rpartition("*")
    if sep and name and n.isdigit():
        return name, int(n)
    return arg, 1


def remote(path: str) -> str:
    """Undo Git Bash's path conversion: on Windows it turns "/folder" into "C:/Program Files/Git/folder"."""
    m = re.match(r"^[A-Za-z]:/(?:.*/)?Git/(.*)$", path)
    if m and os.name == "nt":
        return "/" + m.group(1)
    return path


def cmd_info(c, a):
    i = c.info()
    rows = [("Name", i.name), ("Model", i.model), ("OS", i.os_version),
            ("Boot loaders", f"{i.boot1_version} / {i.boot2_version}"), ("Product ID", i.electronic_id),
            ("Storage", f"{_size(i.flash_total - i.flash_free)} of {_size(i.flash_total)} bytes used"),
            ("Memory", f"{_size(i.ram_total - i.ram_free)} of {_size(i.ram_total)} bytes used"),
            ("Battery", i.battery + (" (charging)" if i.charging else "")),
            ("Screen", f"{i.lcd_width}x{i.lcd_height} {i.lcd_bpp}-bit")]
    for k, v in rows:
        print(f"{k:13} {v}")


def cmd_ls(c, a):
    for e in (c.walk(a.path) if a.recursive else c.list_dir(a.path)):
        shown = e.path if a.recursive else e.name
        print(f"{'<DIR>' if e.is_dir else _size(e.size):>12}  {shown}")


def cmd_get(c, a):
    n = c.download(a.remote, a.local or os.path.basename(a.remote.rstrip("/")))
    print(f"received {_size(n)} bytes")


def cmd_put(c, a):
    for local in a.local:
        print("sent", c.upload(local, a.remote, overwrite=a.force))


def cmd_mkdir(c, a):
    c.mkdir(a.path, parents=a.parents)


def cmd_rm(c, a):
    for p in a.path:
        c.delete(p)
        print("deleted", p)


def cmd_mv(c, a):
    c.move(a.src, a.dst)


def cmd_cp(c, a):
    c.copy(a.src, a.dst)


def cmd_shot(c, a):
    s = c.screenshot()
    from .gui.screen_panel import screenshot_to_image
    img = screenshot_to_image(s)
    if not img.save(a.file):
        raise SystemExit(f"couldn't write {a.file}")
    print(f"saved {s.width}x{s.height} screenshot to {a.file}")


def cmd_backup(c, a):
    n = c.backup(a.folder, progress=lambda p, i, t: print(f"[{i}/{t}] {p}"))
    print(f"backed up {n} files to {a.folder}")


def cmd_restore(c, a):
    n = c.restore(a.folder, a.remote, overwrite=a.force, progress=lambda p, i, t: print(f"[{i}/{t}] {p}"))
    print(f"restored {n} files")


def cmd_install_os(c, a):
    want = c.info().os_extension
    if not a.file.lower().endswith(want.lower()) and not a.force:
        raise SystemExit(f"this handheld needs a {want} file (use --force to send anyway)")
    with open(a.file, "rb") as f:
        image = f.read()
    c.install_os(image, progress=lambda stage, d, t: print(f"{stage}: {d}/{t}", end="\r"))
    print("\ndone; the handheld restarts")


def cmd_key(c, a):
    from .protocol import keys
    for arg in a.keys:
        k, n = parse_key_arg(arg)
        for _ in range(n):
            c._call(keys.press, keys.key_code(k))
            time.sleep(a.delay)


def cmd_type(c, a):
    from .protocol import keys
    for code in keys.text_codes(a.text):
        c._call(keys.press, code)
        time.sleep(a.delay)


def cmd_build(_c, a):
    from .textio import read_text
    from .tns import build_lua, build_notes, build_python, build_text
    src = read_text(a.source)
    ext = os.path.splitext(a.source)[1].lower()
    if a.kind == "notes" or (a.kind == "auto" and ext in (".md", ".notes")):
        data = build_notes([src.split("\n")])
    elif a.kind == "python" or (a.kind == "auto" and ext == ".py"):
        from .gui.create_panel import safe_identifier
        data = build_python(safe_identifier(os.path.splitext(os.path.basename(a.source))[0]) + ".py", src)
    elif a.kind == "lua" or (a.kind == "auto" and ext == ".lua"):
        data = build_lua(src)
    else:
        data = build_text(src)
    out = a.output or os.path.splitext(a.source)[0] + ".tns"
    with open(out, "wb") as f:
        f.write(data)
    print("wrote", out)


def cmd_view(_c, a):
    from .tns import TnsDocument
    doc = TnsDocument.from_file(a.file)
    print(doc.summary())
    if a.extract:
        from .textio import safe_join
        os.makedirs(a.extract, exist_ok=True)
        for name, code in doc.python_files().items():
            path = safe_join(a.extract, name)  # names come from the document; keep them in the folder
            if path is None:
                print("skipped unsafe name", repr(name))
                continue
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(code)
            print("extracted", os.path.basename(path))


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="calcbridge-cli", description=f"CalcBridge {__version__} command line")
    p.add_argument("--sim", action="store_true", help="talk to a simulated calculator")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("info", help="handheld details").set_defaults(fn=cmd_info)
    s = sub.add_parser("ls", help="list a folder")
    s.add_argument("path", nargs="?", default="/")
    s.add_argument("-r", "--recursive", action="store_true")
    s.set_defaults(fn=cmd_ls)
    s = sub.add_parser("get", help="download a file or folder")
    s.add_argument("remote")
    s.add_argument("local", nargs="?")
    s.set_defaults(fn=cmd_get)
    s = sub.add_parser("put", help="upload files/folders into a handheld folder")
    s.add_argument("local", nargs="+")
    s.add_argument("remote")
    s.add_argument("-f", "--force", action="store_true", help="replace existing files")
    s.set_defaults(fn=cmd_put)
    s = sub.add_parser("mkdir")
    s.add_argument("path")
    s.add_argument("-p", "--parents", action="store_true")
    s.set_defaults(fn=cmd_mkdir)
    s = sub.add_parser("rm", help="delete files or folders (recursive)")
    s.add_argument("path", nargs="+")
    s.set_defaults(fn=cmd_rm)
    for name, fn in (("mv", cmd_mv), ("cp", cmd_cp)):
        s = sub.add_parser(name)
        s.add_argument("src")
        s.add_argument("dst")
        s.set_defaults(fn=fn)
    s = sub.add_parser("shot", help="save a screenshot (png/jpg/bmp)")
    s.add_argument("file", nargs="?", default="screen.png")
    s.set_defaults(fn=cmd_shot)
    s = sub.add_parser("backup", help="copy everything on the handheld to a folder")
    s.add_argument("folder")
    s.set_defaults(fn=cmd_backup)
    s = sub.add_parser("restore", help="upload a backup folder")
    s.add_argument("folder")
    s.add_argument("remote", nargs="?", default="/")
    s.add_argument("-f", "--force", action="store_true")
    s.set_defaults(fn=cmd_restore)
    s = sub.add_parser("install-os", help="install an OS image (.tco2/.tcc2/.tct2)")
    s.add_argument("file")
    s.add_argument("--force", action="store_true")
    s.set_defaults(fn=cmd_install_os)
    s = sub.add_parser("key", help="press keys (names like esc home enter up down a 1 ...; key*3 repeats)")
    s.add_argument("keys", nargs="+")
    s.add_argument("--delay", type=float, default=0.3)
    s.set_defaults(fn=cmd_key)
    s = sub.add_parser("type", help="type text on the handheld")
    s.add_argument("text")
    s.add_argument("--delay", type=float, default=0.1)
    s.set_defaults(fn=cmd_type)
    s = sub.add_parser("build", help="make a .tns from a .py/.lua/.txt/.md file (no calculator needed)")
    s.add_argument("source")
    s.add_argument("-o", "--output")
    s.add_argument("--kind", choices=["auto", "notes", "python", "lua", "text"], default="auto")
    s.set_defaults(fn=cmd_build, offline=True)
    s = sub.add_parser("view", help="describe a .tns file (no calculator needed)")
    s.add_argument("file")
    s.add_argument("--extract", metavar="FOLDER", help="save its Python files here")
    s.set_defaults(fn=cmd_view, offline=True)
    a = p.parse_args(argv)
    for attr in ("path", "remote", "src", "dst"):  # handheld paths only
        if a.cmd in ("put", "restore") and attr != "remote":
            continue
        v = getattr(a, attr, None)
        if isinstance(v, str):
            setattr(a, attr, remote(v))
        elif isinstance(v, list):
            setattr(a, attr, [remote(x) for x in v])
    if getattr(a, "offline", False):
        a.fn(None, a)
        return 0
    from .protocol.errors import CalcError
    try:
        with _calc(a) as c:
            a.fn(c, a)
    except CalcError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
