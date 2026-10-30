"""Two-pane file manager: this computer on the left, the handheld on the right."""
from __future__ import annotations

import json
import os

from PySide6.QtCore import QDir, QMimeData, QSortFilterProxyModel, Qt, Signal
from PySide6.QtGui import QDrag, QStandardItem, QStandardItemModel
from PySide6.QtWidgets import (QAbstractItemView, QFileDialog, QFileSystemModel, QHBoxLayout, QHeaderView, QLabel,
                               QLineEdit, QMessageBox, QSplitter, QStyle, QToolButton, QTreeView, QVBoxLayout,
                               QWidget)

from ..protocol.services import dirname

REMOTE_MIME = "application/x-calcbridge-remote"
OS_EXTS = (".tco2", ".tcc2", ".tct2", ".tco", ".tcc", ".tct")
CONVERTIBLE = {".py": "Python program", ".lua": "Lua script", ".txt": "Notes page", ".md": "Notes page"}


def human_size(n: int) -> str:
    for unit in ("bytes", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:,} {unit}" if unit == "bytes" else f"{n:.1f} {unit}"
        n /= 1024
    return str(n)


class _FolderFirstProxy(QSortFilterProxyModel):
    def lessThan(self, left, right):
        a = self.sourceModel().itemFromIndex(left.siblingAtColumn(0))
        b = self.sourceModel().itemFromIndex(right.siblingAtColumn(0))
        da, db = a.data(Qt.UserRole + 2), b.data(Qt.UserRole + 2)
        if da != db:
            return bool(da) if self.sortOrder() == Qt.AscendingOrder else not da
        if left.column() == 1:
            return (a.data(Qt.UserRole + 3) or 0) < (b.data(Qt.UserRole + 3) or 0)
        return super().lessThan(left, right)


class RemoteTree(QTreeView):
    """Handheld listing: drags out remote paths; accepts local files and internal moves."""

    files_dropped = Signal(list, str)      # local paths, remote folder
    remote_dropped = Signal(list, str)     # remote paths, remote folder (move)
    export_requested = Signal(list)        # remote paths dragged out to the OS

    def __init__(self, panel):
        super().__init__()
        self.panel = panel
        self.setDragEnabled(True)
        self.setAcceptDrops(True)
        self.setDropIndicatorShown(True)
        self.setDragDropMode(QAbstractItemView.DragDrop)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setRootIsDecorated(False)
        self.setSortingEnabled(True)
        self.setUniformRowHeights(True)

    def startDrag(self, actions):
        paths = self.panel.selected_paths()
        if not paths:
            return
        mime = QMimeData()
        mime.setData(REMOTE_MIME, json.dumps(paths).encode())
        drag = QDrag(self)
        drag.setMimeData(mime)
        drag.exec(Qt.CopyAction | Qt.MoveAction)

    def _target_folder(self, pos):
        idx = self.indexAt(pos)
        if idx.isValid():
            item = self.panel.item_at(idx)
            if item is not None and item.data(Qt.UserRole + 2):
                return item.data(Qt.UserRole + 1)
        return self.panel.path

    def dragEnterEvent(self, e):
        if e.mimeData().hasUrls() or e.mimeData().hasFormat(REMOTE_MIME):
            e.acceptProposedAction()

    def dragMoveEvent(self, e):
        if e.mimeData().hasUrls() or e.mimeData().hasFormat(REMOTE_MIME):
            e.acceptProposedAction()

    def dropEvent(self, e):
        folder = self._target_folder(e.position().toPoint())
        md = e.mimeData()
        if md.hasFormat(REMOTE_MIME):
            paths = [p for p in json.loads(bytes(md.data(REMOTE_MIME)).decode()) if dirname(p) != folder and p != folder]
            if paths:
                self.remote_dropped.emit(paths, folder)
        elif md.hasUrls():
            local = [u.toLocalFile() for u in md.urls() if u.isLocalFile()]
            if local:
                self.files_dropped.emit(local, folder)
        e.acceptProposedAction()


class LocalTree(QTreeView):
    """This computer: standard file drags out; accepts handheld paths (downloads)."""

    remote_dropped = Signal(list, str)  # remote paths, local folder

    def __init__(self, panel):
        super().__init__()
        self.panel = panel
        self.setDragEnabled(True)
        self.setAcceptDrops(True)
        self.setDragDropMode(QAbstractItemView.DragDrop)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setSortingEnabled(True)

    def dragEnterEvent(self, e):
        if e.mimeData().hasFormat(REMOTE_MIME):
            e.acceptProposedAction()
        else:
            super().dragEnterEvent(e)

    def dragMoveEvent(self, e):
        if e.mimeData().hasFormat(REMOTE_MIME):
            e.acceptProposedAction()
        else:
            super().dragMoveEvent(e)

    def dropEvent(self, e):
        if e.mimeData().hasFormat(REMOTE_MIME):
            idx = self.indexAt(e.position().toPoint())
            folder = self.panel.current_dir
            if idx.isValid() and self.panel.model.isDir(idx):
                folder = self.panel.model.filePath(idx)
            self.remote_dropped.emit(json.loads(bytes(e.mimeData().data(REMOTE_MIME)).decode()), folder)
            e.acceptProposedAction()
        else:
            e.ignore()


class LocalPanel(QWidget):
    open_document = Signal(str)  # local .tns path

    def __init__(self, start_dir):
        super().__init__()
        self.model = QFileSystemModel(self)
        self.model.setFilter(QDir.AllEntries | QDir.NoDotAndDotDot | QDir.AllDirs)
        self.model.setRootPath("")
        self.tree = LocalTree(self)
        self.tree.setModel(self.model)
        self.tree.setColumnWidth(0, 230)
        self.tree.doubleClicked.connect(self._activated)
        self.path_edit = QLineEdit()
        self.path_edit.returnPressed.connect(lambda: self.set_dir(self.path_edit.text()))
        up = QToolButton(text="↑", toolTip="Up one folder")
        up.clicked.connect(self.go_up)
        browse = QToolButton(text="…", toolTip="Choose a folder")
        browse.clicked.connect(self._browse)
        bar = QHBoxLayout()
        bar.addWidget(QLabel("<b>This computer</b>"))
        bar.addWidget(up)
        bar.addWidget(self.path_edit, 1)
        bar.addWidget(browse)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addLayout(bar)
        lay.addWidget(self.tree)
        self.current_dir = ""
        self.set_dir(start_dir)

    def go_up(self):
        parent = os.path.dirname(self.current_dir)
        if parent and parent != self.current_dir:  # at a drive/volume root there's nowhere to go
            self.set_dir(parent)

    def set_dir(self, path):
        if path and os.path.isdir(path):
            self.current_dir = os.path.abspath(path)
            self.tree.setRootIndex(self.model.index(self.current_dir))
            self.path_edit.setText(self.current_dir)

    def _browse(self):
        d = QFileDialog.getExistingDirectory(self, "Choose a folder", self.current_dir)
        if d:
            self.set_dir(d)

    def _activated(self, idx):
        path = self.model.filePath(idx)
        if self.model.isDir(idx):
            self.set_dir(path)
        elif path.lower().endswith(".tns"):
            self.open_document.emit(path)

    def selected_paths(self):
        return sorted({self.model.filePath(i) for i in self.tree.selectionModel().selectedRows(0)})


class RemotePanel(QWidget):
    """The handheld's files. The main window wires the actions to device jobs."""

    open_document = Signal(str)   # remote .tns path
    path_changed = Signal(str)

    def __init__(self):
        super().__init__()
        self.path = "/"
        self.entries = []
        self.model = QStandardItemModel(0, 3, self)
        self.model.setHorizontalHeaderLabels(["Name", "Size", "Type"])
        self.proxy = _FolderFirstProxy(self)
        self.proxy.setSourceModel(self.model)
        self.proxy.setSortCaseSensitivity(Qt.CaseInsensitive)
        self.tree = RemoteTree(self)
        self.tree.setModel(self.proxy)
        self.tree.sortByColumn(0, Qt.AscendingOrder)
        self.tree.header().setSectionResizeMode(0, QHeaderView.Stretch)
        self.tree.doubleClicked.connect(self._activated)
        self.crumb = QLabel()
        self.crumb.setTextInteractionFlags(Qt.TextSelectableByMouse)
        self.up_btn = QToolButton(text="↑", toolTip="Up one folder (Backspace)")
        self.up_btn.clicked.connect(self.go_up)
        bar = QHBoxLayout()
        bar.addWidget(QLabel("<b>Handheld</b>"))
        bar.addWidget(self.up_btn)
        bar.addWidget(self.crumb, 1)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addLayout(bar)
        lay.addWidget(self.tree)
        self._update_crumb()

    def _update_crumb(self):
        self.crumb.setText("My Documents" + ("" if self.path == "/" else self.path.replace("/", " › ")))
        self.up_btn.setEnabled(self.path != "/")

    def set_entries(self, path, entries):
        self.path, self.entries = path, entries
        self.model.removeRows(0, self.model.rowCount())
        style = self.style()
        icons = {True: style.standardIcon(QStyle.SP_DirIcon), False: style.standardIcon(QStyle.SP_FileIcon)}
        for e in entries:
            name = QStandardItem(icons[e.is_dir], e.name)
            name.setData(e.path, Qt.UserRole + 1)
            name.setData(e.is_dir, Qt.UserRole + 2)
            name.setData(0 if e.is_dir else e.size, Qt.UserRole + 3)
            name.setEditable(False)
            size = QStandardItem("" if e.is_dir else human_size(e.size))
            size.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
            kind = QStandardItem("Folder" if e.is_dir else ("TI-Nspire document" if e.is_document else "File"))
            for it in (size, kind):
                it.setEditable(False)
            self.model.appendRow([name, size, kind])
        self.tree.sortByColumn(self.tree.header().sortIndicatorSection(), self.tree.header().sortIndicatorOrder())
        self._update_crumb()
        self.path_changed.emit(path)

    def clear(self):
        self.model.removeRows(0, self.model.rowCount())
        self.entries = []

    def item_at(self, proxy_index):
        return self.model.itemFromIndex(self.proxy.mapToSource(proxy_index.siblingAtColumn(0)))

    def selected_entries(self):
        out = []
        for idx in self.tree.selectionModel().selectedRows(0):
            path = self.item_at(idx).data(Qt.UserRole + 1)
            out.extend(e for e in self.entries if e.path == path)
        return out

    def selected_paths(self):
        return [e.path for e in self.selected_entries()]

    def names(self):
        return {e.name for e in self.entries}

    def go_up(self):
        if self.path != "/":
            self.open_folder(dirname(self.path))

    def open_folder(self, path):  # replaced by the main window with a device job
        pass

    def _activated(self, idx):
        e = next((x for x in self.entries if x.path == self.item_at(idx).data(Qt.UserRole + 1)), None)
        if e is None:
            return
        if e.is_dir:
            self.open_folder(e.path)
        elif e.is_document:
            self.open_document.emit(e.path)


class FilesTab(QWidget):
    def __init__(self, start_dir):
        super().__init__()
        self.local = LocalPanel(start_dir)
        self.remote = RemotePanel()
        split = QSplitter(Qt.Horizontal)
        split.addWidget(self.local)
        split.addWidget(self.remote)
        split.setSizes([480, 520])
        lay = QVBoxLayout(self)
        lay.addWidget(split)


def unique_copy_name(name: str, taken: set[str]) -> str:
    """'Copy of x.tns', 'Copy of x (2).tns', ... like TI's software."""
    stem, ext = os.path.splitext(name)
    cand = f"Copy of {stem}{ext}"
    n = 2
    while cand in taken:
        cand = f"Copy of {stem} ({n}){ext}"
        n += 1
    return cand


def next_folder_name(taken: set[str]) -> str:
    n = 1
    while f"Folder{n}" in taken:
        n += 1
    return f"Folder{n}"


def ask_overwrite(parent, name) -> str:
    """-> 'yes' | 'all' | 'no' | 'cancel' (TI's Yes / Yes to All / No / Cancel)."""
    box = QMessageBox(parent)
    box.setIcon(QMessageBox.Question)
    box.setWindowTitle("Replace file?")
    box.setText(f"“{name}” already exists on the handheld. Replace it?")
    yes = box.addButton("Yes", QMessageBox.YesRole)
    yes_all = box.addButton("Yes to All", QMessageBox.YesRole)
    no = box.addButton("No", QMessageBox.NoRole)
    box.addButton("Cancel", QMessageBox.RejectRole)
    box.exec()
    clicked = box.clickedButton()
    return {yes: "yes", yes_all: "all", no: "no"}.get(clicked, "cancel")
