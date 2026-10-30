"""Create documents: Notes with real 2D math, Python programs, Lua scripts, plain text."""
from __future__ import annotations

import os
import re

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QFont, QFontDatabase
from PySide6.QtWidgets import (QCheckBox, QComboBox, QFileDialog, QFormLayout, QGridLayout, QGroupBox,
                               QHBoxLayout, QLabel, QLineEdit, QMessageBox, QPlainTextEdit, QPushButton,
                               QScrollArea, QSplitter, QVBoxLayout, QWidget)

from ..tns import build_lua, build_notes, build_python, build_text
from ..tns.notes import MathSyntaxError
from .mathview import NotesView

KINDS = ["Notes with math", "Python program", "Lua script", "Plain text"]

NOTES_EXAMPLE = r"""**Newton's method**
**What:** $x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$
Try $f(x)=x^{2}-5$ with $x_{0}=2$: $x_{1}=\frac{9}{4}$, $x_{2}=\frac{161}{72}≈2.23611$.
Roots: $\sqrt{2}≈1.414$, error $\abs{x_{2}-x_{1}}$."""

PY_EXAMPLE = """# Newton's method for x**2 - 5
def newton(f, fp, x, n):
    for k in range(n):
        x = x - f(x)/fp(x)
    return x

print(newton(lambda x: x**2 - 5, lambda x: 2*x, 2.0, 6))
"""

LUA_EXAMPLE = """function on.paint(gc)
  gc:drawString("Hello from CalcBridge", 20, 40)
end
"""

INSERTS = [("a/b", r"\frac{}{}"), ("√", r"\sqrt{}"), ("xⁿ", "^{}"), ("xₖ", "_{}"), ("|x|", r"\abs{}"),
           ("x̂", r"\hat{x}"), ("ε", "ε"), ("π", "π"), ("θ", "θ"), ("τ", "τ"), ("Δ", "Δ"), ("Σ", "Σ"),
           ("≤", "≤"), ("≥", "≥"), ("≠", "≠"), ("≈", "≈"), ("±", "±"), ("→", "→"), ("←", "←"), ("∞", "∞"),
           ("…", "…"), ("·", "·"), ("×", "×")]


def safe_identifier(name: str) -> str:
    s = re.sub(r"\W", "_", name.strip().lower()) or "program"
    return ("p_" + s) if s[0].isdigit() else s


class CreateTab(QWidget):
    send_requested = Signal(bytes, str, bool)   # document bytes, file name (.tns), into PyLib?
    status = Signal(str)

    def __init__(self):
        super().__init__()
        self.kind = QComboBox()
        self.kind.addItems(KINDS)
        self.name = QLineEdit("New document")
        self.script = QLineEdit()
        self.script.setPlaceholderText("program.py (Python only)")
        form = QFormLayout()
        form.addRow("Type", self.kind)
        form.addRow("Document name", self.name)
        form.addRow("Python file name", self.script)

        self.editor = QPlainTextEdit()
        mono = QFontDatabase.systemFont(QFontDatabase.FixedFont)
        mono.setPointSize(11)
        self.editor.setFont(mono)
        self.inserts = QGroupBox("Insert")
        g = QGridLayout(self.inserts)
        for i, (label, text) in enumerate(INSERTS):
            b = QPushButton(label)
            b.setFocusPolicy(Qt.NoFocus)
            b.clicked.connect(lambda _=False, t=text: self._insert(t))
            g.addWidget(b, i // 12, i % 12)
        self.help = QLabel()
        self.help.setWordWrap(True)

        self.preview = NotesView(page_width=320)
        self.handheld_width = QCheckBox("Preview at handheld width (320 px)")
        self.handheld_width.setChecked(True)
        self.handheld_width.toggled.connect(lambda on: self._set_preview_width(on))
        scroll = QScrollArea()
        scroll.setWidget(self.preview)
        scroll.setWidgetResizable(True)
        self.preview_box = QWidget()
        pl = QVBoxLayout(self.preview_box)
        pl.setContentsMargins(0, 0, 0, 0)
        pl.addWidget(QLabel("<b>Preview</b> (approximate; the calculator's fonts differ slightly)"))
        pl.addWidget(self.handheld_width)
        pl.addWidget(scroll)

        left = QWidget()
        ll = QVBoxLayout(left)
        ll.setContentsMargins(0, 0, 0, 0)
        ll.addLayout(form)
        ll.addWidget(self.inserts)
        ll.addWidget(self.editor, 1)
        ll.addWidget(self.help)
        split = QSplitter(Qt.Horizontal)
        split.addWidget(left)
        split.addWidget(self.preview_box)
        split.setSizes([560, 380])

        open_btn = QPushButton("Open file…")
        open_btn.clicked.connect(self.open_file)
        save_btn = QPushButton("Save .tns to computer…")
        save_btn.clicked.connect(self.save_to_computer)
        self.send_btn = QPushButton("Send to handheld")
        self.send_btn.clicked.connect(lambda: self._send(False))
        self.module_btn = QPushButton("Install as Python module (PyLib)")
        self.module_btn.clicked.connect(lambda: self._send(True))
        bar = QHBoxLayout()
        for b in (open_btn, save_btn, self.send_btn, self.module_btn):
            bar.addWidget(b)
        bar.addStretch(1)

        lay = QVBoxLayout(self)
        lay.addWidget(split, 1)
        lay.addLayout(bar)

        self._debounce = QTimer(self, singleShot=True, interval=250)
        self._debounce.timeout.connect(self._update_preview)
        self.editor.textChanged.connect(self._debounce.start)
        self.kind.currentIndexChanged.connect(self._kind_changed)
        self.name.textChanged.connect(self._sync_script_name)
        self._kind_changed(0)

    # ------------------------------------------------------------- UI
    def _set_preview_width(self, on):
        self.preview.page_width = 320 if on else None
        self._update_preview()

    def _insert(self, text):
        cur = self.editor.textCursor()
        if self.kind.currentIndex() == 0 and not self._inside_math():
            text = f"${text}$"
        cur.insertText(text)
        if "{}" in text:  # put the cursor inside the first {}
            pos = cur.position() - len(text) + text.index("{}") + 1
            cur.setPosition(pos)
            self.editor.setTextCursor(cur)
        self.editor.setFocus()

    def _inside_math(self):
        cur = self.editor.textCursor()
        line = cur.block().text()[:cur.positionInBlock()]
        return line.count("$") % 2 == 1

    def _kind_changed(self, i):
        notes = i == 0
        self.inserts.setVisible(notes)
        self.preview_box.setVisible(i in (0, 3))
        self.script.setEnabled(i == 1)
        self.module_btn.setVisible(i == 1)
        examples = {0: NOTES_EXAMPLE, 1: PY_EXAMPLE, 2: LUA_EXAMPLE, 3: "Type plain text here."}
        if not self.editor.toPlainText().strip() or self.editor.toPlainText() in examples.values():
            self.editor.setPlainText(examples[i])
        self.help.setText({
            0: "Each line is a paragraph. **bold**, and $…$ for math: \\frac{a}{b}, \\sqrt{x}, x^{2}, x_{k}, "
               "\\abs{x}, \\hat{x}. Math appears in real TI math boxes on the handheld.",
            1: "The program opens in the handheld's Python editor; press ctrl+R there to run it. "
               "\"Install as module\" puts it in PyLib so other programs can import it.",
            2: "TI-Nspire Lua (API level 2). The script runs when the document opens.",
            3: "Each line becomes a paragraph on one Notes page.",
        }[i])
        self._sync_script_name()
        self._update_preview()

    def _sync_script_name(self):
        if self.kind.currentIndex() == 1:
            self.script.setText(safe_identifier(self.name.text()) + ".py")

    def showEvent(self, e):
        self._update_preview()
        super().showEvent(e)

    def _update_preview(self):
        if self.kind.currentIndex() not in (0, 3):
            return
        lines = self.editor.toPlainText().split("\n")
        if self.kind.currentIndex() == 0:
            self.preview.set_markup(lines)
        else:
            self.preview.set_paragraphs([[("text", ln, {})] for ln in lines])

    # ---------------------------------------------------------- output
    def build(self) -> tuple[bytes, str]:
        kind, text = self.kind.currentIndex(), self.editor.toPlainText()
        name = self.name.text().strip() or "New document"
        if kind == 0:
            data = build_notes([text.split("\n")])
        elif kind == 1:
            data = build_python(self.script.text().strip() or safe_identifier(name) + ".py", text)
        elif kind == 2:
            data = build_lua(text)
        else:
            data = build_text(text)
        return data, name + ".tns"

    def _build_or_warn(self):
        try:
            return self.build()
        except (MathSyntaxError, ValueError) as e:
            QMessageBox.warning(self, "Can't build the document", str(e))
            return None

    def save_to_computer(self):
        built = self._build_or_warn()
        if not built:
            return
        data, fname = built
        path, _ = QFileDialog.getSaveFileName(self, "Save document", fname, "TI-Nspire document (*.tns)")
        if path:
            with open(path, "wb") as f:
                f.write(data)
            self.status.emit(f"Saved {path}")

    def _send(self, as_module):
        built = self._build_or_warn()
        if built:
            data, fname = built
            if as_module:
                # TI requires the module's document to be named like the module
                fname = os.path.splitext(self.script.text().strip())[0] + ".tns"
            self.send_requested.emit(data, fname, as_module)

    def open_file(self):
        path, _ = QFileDialog.getOpenFileName(self, "Open a source file", "",
                                              "Sources (*.py *.lua *.txt *.md);;All files (*)")
        if not path:
            return
        self.load_source(path)

    def load_source(self, path, code=None):
        ext = os.path.splitext(path)[1].lower()
        kind = {".py": 1, ".lua": 2}.get(ext, 3)
        if code is None:
            with open(path, encoding="utf-8", errors="replace") as f:
                code = f.read()
        self.kind.setCurrentIndex(kind)
        self.name.setText(os.path.splitext(os.path.basename(path))[0])
        self.editor.setPlainText(code)
        if kind == 1:
            self.script.setText(safe_identifier(os.path.splitext(os.path.basename(path))[0]) + ".py")
