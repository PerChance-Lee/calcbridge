"""Look inside a .tns: pages, Notes content (rendered), Python and Lua source."""
from __future__ import annotations

import os

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QFontDatabase
from PySide6.QtWidgets import (QFileDialog, QHBoxLayout, QLabel, QListWidget, QListWidgetItem, QMessageBox,
                               QPlainTextEdit, QPushButton, QScrollArea, QSplitter, QStackedWidget, QVBoxLayout,
                               QWidget)

from ..tns import TnsDocument
from .mathview import NotesView, parse_r2d


class ViewerTab(QWidget):
    edit_python = Signal(str, str)      # file name, code
    send_requested = Signal(bytes, str)  # document bytes, file name

    def __init__(self):
        super().__init__()
        self.doc = None
        self.blob = None
        self.title = QLabel("Open a .tns file from this computer, or double-click a document on the handheld.")
        self.title.setWordWrap(True)
        self.pages = QListWidget()
        self.pages.setMaximumWidth(260)
        self.pages.currentRowChanged.connect(self._show)
        self.stack = QStackedWidget()
        self.notes = NotesView(page_width=None)
        scroll = QScrollArea()
        scroll.setWidget(self.notes)
        scroll.setWidgetResizable(True)
        self.code = QPlainTextEdit(readOnly=True)
        mono = QFontDatabase.systemFont(QFontDatabase.FixedFont)
        self.code.setFont(mono)
        self.info = QLabel(alignment=Qt.AlignTop | Qt.AlignLeft)
        self.info.setWordWrap(True)
        for w in (scroll, self.code, self.info):
            self.stack.addWidget(w)
        split = QSplitter(Qt.Horizontal)
        split.addWidget(self.pages)
        split.addWidget(self.stack)
        split.setSizes([220, 700])
        open_btn = QPushButton("Open .tns…")
        open_btn.clicked.connect(self.open_file)
        self.save_py = QPushButton("Save Python files…")
        self.save_py.clicked.connect(self._save_python)
        self.edit_btn = QPushButton("Edit in Create tab")
        self.edit_btn.clicked.connect(self._edit)
        self.send_btn = QPushButton("Send to handheld")
        self.send_btn.clicked.connect(lambda: self.send_requested.emit(self.blob, self.name))
        bar = QHBoxLayout()
        for b in (open_btn, self.save_py, self.edit_btn, self.send_btn):
            bar.addWidget(b)
        bar.addStretch(1)
        lay = QVBoxLayout(self)
        lay.addWidget(self.title)
        lay.addLayout(bar)
        lay.addWidget(split, 1)
        self._entries = []
        self.name = ""
        self.source = None
        self._update_buttons()

    def _update_buttons(self):
        has_py = bool(self.doc and self.doc.python_files())
        self.save_py.setEnabled(has_py)
        self.edit_btn.setEnabled(has_py)
        self.send_btn.setEnabled(self.blob is not None and self.source == "computer")

    def open_file(self):
        path, _ = QFileDialog.getOpenFileName(self, "Open a TI-Nspire document", "", "TI-Nspire documents (*.tns)")
        if path:
            with open(path, "rb") as f:
                self.load(f.read(), os.path.basename(path), "computer")

    def load(self, blob: bytes, name: str, source: str):
        try:
            doc = TnsDocument.from_bytes(blob)
        except Exception as e:
            QMessageBox.warning(self, "Can't open document", f"{name} isn't a readable TI-Nspire document:\n{e}")
            return
        self.doc, self.blob, self.name, self.source = doc, blob, name, source
        self.title.setText(f"<b>{name}</b> — from the {source}. " + doc.summary().splitlines()[0])
        self.pages.clear()
        self._entries = []
        trees = {(p.problem, p.index): t for p, t in doc.notes_trees()}
        for page in doc.pages():
            label = f"Page {page.problem}.{page.index}: {', '.join(page.apps) or 'empty'}"
            tree = trees.get((page.problem, page.index))
            if tree is not None:
                self._entries.append(("notes", tree))
            elif "Python editor" in page.apps:
                self._entries.append(("code", "\n\n".join(f"# ---- {n}\n{c}" for n, c in doc.python_files().items())))
            elif "Lua script" in page.apps:
                self._entries.append(("code", doc.lua_script() or ""))
            else:
                self._entries.append(("info", f"{', '.join(page.apps)} pages need TI's software to display. "
                                              "CalcBridge can still transfer, back up and organise this document."))
            self.pages.addItem(QListWidgetItem(label))
        for n, c in doc.python_files().items():
            self._entries.append(("code", c))
            self.pages.addItem(QListWidgetItem(f"Python file: {n}"))
        if self.pages.count():
            self.pages.setCurrentRow(0)
        self._update_buttons()

    def _show(self, row):
        if row < 0 or row >= len(self._entries):
            return
        kind, payload = self._entries[row]
        if kind == "notes":
            try:
                self.notes.set_paragraphs(parse_r2d(payload))
            except Exception as e:
                self.notes.set_paragraphs([[("text", f"This page couldn't be drawn ({e}).", {})]])
            self.stack.setCurrentIndex(0)
        elif kind == "code":
            self.code.setPlainText(payload)
            self.stack.setCurrentIndex(1)
        else:
            self.info.setText(payload)
            self.stack.setCurrentIndex(2)

    def _save_python(self):
        files = self.doc.python_files() if self.doc else {}
        folder = QFileDialog.getExistingDirectory(self, "Save Python files to")
        if folder:
            for n, c in files.items():
                with open(os.path.join(folder, n), "w", encoding="utf-8", newline="\n") as f:
                    f.write(c)

    def _edit(self):
        files = self.doc.python_files() if self.doc else {}
        if files:
            name, code = next(iter(files.items()))
            self.edit_python.emit(name, code)
