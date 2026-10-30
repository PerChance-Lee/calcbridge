"""High-level calculator API used by the GUI and the command line."""
from __future__ import annotations

import logging
import os
import threading
from dataclasses import dataclass
from typing import Callable

from . import paths
from .protocol import services as svc
from .protocol.errors import CalcError, ExistsError, NotFoundError, RemoteError
from .protocol.navnet import Session
from .protocol.nnse import Link, LinkResetError, SeqStore
from .protocol.services import DeviceInfo, DirEntry, Screenshot
from .protocol.transport import Transport, UsbTransport

log = logging.getLogger(__name__)

Progress = Callable[[int, int], None]

# Names the handheld refuses or that break its file browser.
FORBIDDEN_CHARS = set('\\/:*?"<>|')
MAX_NAME = 255


def check_name(name: str) -> None:
    if not name or name in (".", ".."):
        raise ValueError("The name can't be empty")
    bad = sorted(c for c in set(name) if c in FORBIDDEN_CHARS or ord(c) < 32)
    if bad:
        raise ValueError(f"The name can't contain {' '.join(bad)}")
    if len(name.encode("utf-8")) > MAX_NAME:
        raise ValueError("The name is too long")


@dataclass
class TransferItem:
    local: str
    remote: str
    is_dir: bool


class Cancelled(CalcError):
    pass


class Calculator:
    """One open connection. All calls are serialised; safe to share between threads."""

    def __init__(self, transport: Transport | None = None, state_file: str | None = None, trace=None,
                 progress=None):
        self._lock = threading.RLock()
        self.transport = transport or UsbTransport()
        self.link = Link(self.transport, SeqStore(state_file if state_file is not None else paths.seq_state_file()),
                         trace=trace)
        self.session = Session(self.link)
        self.cancel_event = threading.Event()
        try:
            self.session.open(progress)
        except Exception:
            self.transport.close()
            raise

    # ---------------------------------------------------------- plumbing
    def _call(self, fn, *args, **kw):
        with self._lock:
            try:
                return fn(self.session, *args, **kw)
            except LinkResetError:
                log.info("link restarted by the calculator; reopening and retrying once")
                self.session.open()
                return fn(self.session, *args, **kw)

    def close(self):
        with self._lock:
            self.link.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def _check_cancel(self):
        if self.cancel_event.is_set():
            self.cancel_event.clear()
            raise Cancelled("Cancelled")

    # ------------------------------------------------------------- basics
    def info(self) -> DeviceInfo:
        return self._call(svc.device_info)

    def screenshot(self) -> Screenshot:
        return self._call(svc.screenshot)

    def list_dir(self, path: str = "/") -> list[DirEntry]:
        return self._call(svc.list_dir, path)

    def stat(self, path: str) -> DirEntry | None:
        return self._call(svc.stat, path)

    def exists(self, path: str) -> bool:
        return self.stat(path) is not None

    def read_file(self, path: str, progress: Progress | None = None) -> bytes:
        return self._call(svc.read_file, path, progress)

    def write_file(self, path: str, data: bytes, progress: Progress | None = None, overwrite: bool = False) -> None:
        check_name(svc.basename(path))
        if not overwrite and self.exists(path):
            raise ExistsError(f"{path} already exists")
        self._call(svc.write_file, path, data, progress)

    def mkdir(self, path: str, parents: bool = False) -> None:
        check_name(svc.basename(path))
        if parents:
            parent = svc.dirname(path)
            if parent != "/" and not self.exists(parent):
                self.mkdir(parent, parents=True)
            if self.exists(path):
                return
        self._call(svc.make_dir, path)

    def rename(self, path: str, new_name: str) -> str:
        check_name(new_name)
        dst = svc.join(svc.dirname(path), new_name)
        self.move(path, dst)
        return dst

    def move(self, src: str, dst: str) -> None:
        if _inside(dst, src):
            raise ValueError("A folder can't be moved into itself")
        if self.exists(dst):
            raise ExistsError(f"{dst} already exists")
        self._call(svc.move, src, dst)

    def copy(self, src: str, dst: str) -> None:
        """Copy a file or a whole folder on the handheld."""
        if _inside(dst, src):
            raise ValueError("A folder can't be copied into itself")
        if self.exists(dst):
            raise ExistsError(f"{dst} already exists")
        st = self.stat(src)
        if st is None:
            raise NotFoundError(f"{src} doesn't exist")
        if not st.is_dir:
            self._call(svc.copy_file, src, dst)
            return
        children = self.list_dir(src)  # list before creating dst, so dst can never show up in it
        self._call(svc.make_dir, dst)
        for e in children:
            self._check_cancel()
            self.copy(e.path, svc.join(dst, e.name))

    def delete(self, path: str) -> None:
        """Delete a file, or a folder and everything in it."""
        st = self.stat(path)
        if st is None:
            raise NotFoundError(f"{path} doesn't exist")
        if st.is_dir:
            for e in self.list_dir(path):
                self._check_cancel()
                self.delete(e.path)
            self._call(svc.remove_dir, path)
        else:
            self._call(svc.delete_file, path)

    def walk(self, path: str = "/"):
        """Yield every entry below path, folders before their contents."""
        for e in self.list_dir(path):
            yield e
            if e.is_dir:
                yield from self.walk(e.path)

    def usage(self) -> tuple[int, int]:
        i = self.info()
        return i.flash_total - i.flash_free, i.flash_total

    # ------------------------------------------------------- transfers
    def download(self, remote: str, local: str, progress: Progress | None = None) -> int:
        """Download a file or folder to a local path. Returns bytes received."""
        st = self.stat(remote)
        if st is None:
            raise NotFoundError(f"{remote} doesn't exist")
        if not st.is_dir:
            data = self.read_file(remote, progress)
            os.makedirs(os.path.dirname(os.path.abspath(local)), exist_ok=True)
            with open(local, "wb") as f:
                f.write(data)
            return len(data)
        os.makedirs(local, exist_ok=True)
        total = 0
        for e in self.list_dir(remote):
            self._check_cancel()
            total += self.download(e.path, os.path.join(local, local_name(e.name)), progress)
        return total

    def upload(self, local: str, remote_folder: str, progress: Progress | None = None,
               overwrite: bool = False, name: str | None = None, skipped: list | None = None) -> str:
        """Upload a file or folder into remote_folder. Returns the remote path.

        The handheld only accepts .tns documents, so inside folders anything else (desktop.ini,
        .DS_Store, stray .py files, ...) is left out and listed in `skipped`."""
        name = name or os.path.basename(os.path.normpath(local))
        dst = svc.join(remote_folder, name)
        if os.path.isdir(local):
            if not self.exists(dst):
                self.mkdir(dst)
            for child in sorted(os.listdir(local)):
                self._check_cancel()
                path = os.path.join(local, child)
                if not os.path.isdir(path) and not is_sendable(child):
                    if skipped is not None and not is_junk(child):
                        skipped.append(path)
                    continue
                self.upload(path, dst, progress, overwrite, skipped=skipped)
            return dst
        if not is_sendable(name):
            raise ValueError(f"{name}: the handheld only accepts TI-Nspire documents (.tns)")
        with open(local, "rb") as f:
            data = f.read()
        self.write_file(dst, data, progress, overwrite=overwrite)
        return dst

    def backup(self, local_dir: str, progress: Callable[[str, int, int], None] | None = None) -> int:
        """Copy everything on the handheld into local_dir. Returns the number of files."""
        entries = [e for e in self.walk("/")]
        files = [e for e in entries if not e.is_dir]
        for e in entries:
            if e.is_dir:
                os.makedirs(os.path.join(local_dir, *map(local_name, e.path.strip("/").split("/"))), exist_ok=True)
        os.makedirs(local_dir, exist_ok=True)
        for n, e in enumerate(files, 1):
            self._check_cancel()
            if progress:
                progress(e.path, n, len(files))
            target = os.path.join(local_dir, *map(local_name, e.path.strip("/").split("/")))
            with open(target, "wb") as f:
                f.write(self.read_file(e.path))
        return len(files)

    def restore(self, local_dir: str, remote_folder: str = "/", overwrite: bool = False,
                progress: Callable[[str, int, int], None] | None = None, skipped: list | None = None) -> int:
        """Upload a backup folder's .tns documents into remote_folder. Returns how many were written.

        Files the handheld won't take (anything not .tns, e.g. the system's own themes.csv) and files
        that already exist when overwrite is False are listed in `skipped`."""
        files = []
        for root, _dirs, names in os.walk(local_dir):
            for nm in sorted(names):
                path = os.path.join(root, nm)
                if is_sendable(nm):
                    files.append(path)
                elif skipped is not None and not is_junk(nm):
                    skipped.append(path)
        written = 0
        for n, path in enumerate(files, 1):
            self._check_cancel()
            rel = os.path.relpath(path, local_dir).replace(os.sep, "/")
            folder = svc.join(remote_folder, os.path.dirname(rel)) if os.path.dirname(rel) else remote_folder
            if folder != "/" and not self.exists(folder):
                self.mkdir(folder, parents=True)
            dst = svc.join(folder, os.path.basename(path))
            if progress:
                progress(dst, n, len(files))
            if self.exists(dst) and not overwrite:
                if skipped is not None:
                    skipped.append(path)
                continue
            with open(path, "rb") as f:
                data = f.read()
            self._call(svc.write_file, dst, data, None)
            written += 1
        return written

    def install_os(self, image: bytes, progress=None) -> None:
        if len(image) < 1024:
            raise RemoteError("That file is too small to be an OS image")
        self._call(svc.install_os, image, progress)


WINDOWS_RESERVED = {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)),
                    *(f"LPT{i}" for i in range(1, 10))}


def local_name(name: str) -> str:
    """A handheld name made safe for the PC's file system (Windows is the strictest)."""
    s = "".join("_" if c in '<>:"\\/|?*' or ord(c) < 32 else c for c in name).rstrip(" .") or "_"
    if s.split(".")[0].upper() in WINDOWS_RESERVED:
        s = "_" + s
    return s


def is_sendable(name: str) -> bool:
    """The handheld only accepts .tns documents (others are refused with FF 15)."""
    return name.lower().endswith(".tns")


def is_junk(name: str) -> bool:
    """OS clutter that shouldn't be reported as 'skipped'."""
    return name.startswith(".") or name.lower() in ("desktop.ini", "thumbs.db")


def _inside(path: str, folder: str) -> bool:
    path, folder = path.rstrip("/"), folder.rstrip("/")
    return path == folder or path.startswith(folder + "/")
