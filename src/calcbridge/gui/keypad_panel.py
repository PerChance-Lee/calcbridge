"""Remote keypad: press the handheld's keys from the computer, with a live view of its screen."""
from __future__ import annotations

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QKeyEvent, QPixmap
from PySide6.QtWidgets import (QGridLayout, QGroupBox, QHBoxLayout, QLabel, QLineEdit, QPushButton, QSizePolicy,
                               QVBoxLayout, QWidget)

# (label, key name) rows, roughly following the CX II keypad from top to bottom.
TOP = [[("esc", "esc"), ("⌂ home", "home"), ("menu", "menu"), ("tab", "tab")],
       [("ctrl", "ctrl"), ("shift", "shift"), ("del", "del"), ("cat", "catalog")]]
NAV = [[None, ("▲", "up"), None], [("◀", "left"), ("●", "click"), ("▶", "right")], [None, ("▼", "down"), None]]
MATH = [[("=", "="), ("sin", "sin"), ("^", "^"), ("x²", "square"), ("10ˣ", "10^"), ("eˣ", "e^")],
        [("var", "var"), ("(", "("), (")", ")"), ("÷", "/"), ("×", "*"), ("cos", "cos")],
        [("7", "7"), ("8", "8"), ("9", "9"), ("−", "-"), ("π", "pi"), ("?!", "?")],
        [("4", "4"), ("5", "5"), ("6", "6"), ("+", "+"), ("EE", "EE"), (",", ",")],
        [("1", "1"), ("2", "2"), ("3", "3"), ("(−)", "neg"), ("θ", "theta"), ("flag", "flag")],
        [("0", "0"), (".", "."), ("space", " "), ("enter", "enter"), ("i", "i"), (":", ":")]]
LETTERS = ["abcdefg", "hijklmn", "opqrstu", "vwxyz"]

PC_KEYS = {Qt.Key_Up: "up", Qt.Key_Down: "down", Qt.Key_Left: "left", Qt.Key_Right: "right",
           Qt.Key_Return: "enter", Qt.Key_Enter: "enter", Qt.Key_Escape: "esc", Qt.Key_Backspace: "del",
           Qt.Key_Delete: "del", Qt.Key_Tab: "tab", Qt.Key_Home: "home", Qt.Key_Space: " "}


class KeypadTab(QWidget):
    key_requested = Signal(str)       # key name
    text_requested = Signal(str)      # string to type
    refresh_requested = Signal()

    def __init__(self):
        super().__init__()
        self.screen = QLabel(alignment=Qt.AlignCenter)
        self.screen.setFixedSize(640, 480)
        self.screen.setStyleSheet("background:#1e1e1e; color:#bbb;")
        self.screen.setText("Connect a calculator to see its screen")
        self.auto = QTimer(self)
        self.auto.timeout.connect(self.refresh_requested)
        refresh = QPushButton("Refresh screen")
        refresh.clicked.connect(self.refresh_requested)
        hint = QLabel("Click this panel, then use your keyboard: arrows, Enter, Esc, Backspace, Tab, letters and "
                      "digits go straight to the handheld. Keys act on the calculator immediately.")
        hint.setWordWrap(True)
        left = QVBoxLayout()
        left.addWidget(self.screen)
        row = QHBoxLayout()
        row.addWidget(refresh)
        row.addStretch(1)
        left.addLayout(row)
        left.addWidget(hint)
        left.addStretch(1)

        right = QVBoxLayout()
        right.addWidget(self._grid("Keys", TOP))
        right.addWidget(self._grid("Touchpad", NAV))
        right.addWidget(self._grid("Math", MATH))
        right.addWidget(self._grid("Letters", [[(c.upper(), c) for c in row] for row in LETTERS]))
        type_box = QGroupBox("Type text")
        tl = QHBoxLayout(type_box)
        self.type_edit = QLineEdit(placeholderText="e.g. 2+2 then press Send")
        send = QPushButton("Send")
        send.clicked.connect(self._send_text)
        self.type_edit.returnPressed.connect(self._send_text)
        tl.addWidget(self.type_edit)
        tl.addWidget(send)
        right.addWidget(type_box)
        right.addStretch(1)

        lay = QHBoxLayout(self)
        lay.addLayout(left)
        lay.addLayout(right, 1)
        self.setFocusPolicy(Qt.StrongFocus)

    def _grid(self, title, rows):
        box = QGroupBox(title)
        g = QGridLayout(box)
        g.setSpacing(3)
        for r, row in enumerate(rows):
            for c, spec in enumerate(row):
                if spec is None:
                    continue
                label, key = spec
                b = QPushButton(label)
                b.setFocusPolicy(Qt.NoFocus)
                b.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
                b.setMinimumWidth(44)
                b.clicked.connect(lambda _=False, k=key: self.key_requested.emit(k))
                g.addWidget(b, r, c)
        return box

    def _send_text(self):
        t = self.type_edit.text()
        if t:
            self.text_requested.emit(t)
            self.type_edit.clear()

    def keyPressEvent(self, e: QKeyEvent):
        if e.key() in PC_KEYS:
            self.key_requested.emit(PC_KEYS[e.key()])
            return
        t = e.text()
        if t and (t.isalnum() or t in "+-*/^=()., ?:"):
            self.key_requested.emit(t)
            return
        super().keyPressEvent(e)

    def set_screen(self, img):
        self.screen.setPixmap(QPixmap.fromImage(img).scaled(640, 480, Qt.KeepAspectRatio, Qt.FastTransformation))

    def showEvent(self, e):
        self.auto.start(2000)
        self.refresh_requested.emit()
        super().showEvent(e)

    def hideEvent(self, e):
        self.auto.stop()
        super().hideEvent(e)
