"""Small helpers for reading user files and writing extracted ones safely."""
from __future__ import annotations

import os
import re


def read_text(path: str) -> str:
    """Read a text file whatever its encoding: UTF-8 (with or without BOM) or UTF-16 (Notepad's
    "Unicode", Windows PowerShell 5.1's ">" redirection). Falls back to UTF-8 with replacement."""
    with open(path, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xef\xbb\xbf"):
        return raw[3:].decode("utf-8", "replace")
    if raw.startswith((b"\xff\xfe", b"\xfe\xff")):
        return raw.decode("utf-16", "replace")
    if len(raw) >= 4 and raw[1:4:2] == b"\0\0" and raw[0:4:2] != b"\0\0":  # BOM-less UTF-16 LE
        return raw.decode("utf-16-le", "replace")
    return raw.decode("utf-8", "replace")


def safe_filename(name: str) -> str | None:
    """A file name from an untrusted document, reduced to a bare name that can't escape the target
    folder (no directories, drive letters or '..'). None if nothing usable is left."""
    base = re.split(r"[\\/]", name)[-1]
    base = base.replace(":", "_").strip().lstrip(".")
    base = "".join(c for c in base if ord(c) >= 32 and c not in '<>"|?*')
    return base or None


def safe_join(folder: str, name: str) -> str | None:
    base = safe_filename(name)
    if base is None:
        return None
    path = os.path.join(folder, base)
    if os.path.commonpath([os.path.abspath(folder), os.path.abspath(path)]) != os.path.abspath(folder):
        return None
    return path
