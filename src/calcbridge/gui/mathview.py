"""Render Notes pages (text + 2D math boxes) roughly the way the handheld draws them.

Model: a page is a list of paragraphs; a paragraph is a list of pieces:
    ("text", str, style)   style: dict(bold, italic, color, sub, sup, size)
    ("math", items)        items as produced by notes.parse_math or parse_r2d
Math items: ("text", s) | ("styled", s, style) | ("sub", items) | ("sup", items) | ("pow", items)
            | ("frac", num, den) | ("sqrt", items) | ("paren", "(" or ")")
"""
from __future__ import annotations

import html
import xml.etree.ElementTree as ET

from PySide6.QtCore import QPointF, QRectF, QSize, Qt
from PySide6.QtGui import QColor, QFont, QFontMetricsF, QPainter, QPainterPath, QPen
from PySide6.QtWidgets import QSizePolicy, QWidget

from ..tns.notes import MathSyntaxError, parse_math, split_markup

MATH_FAMILIES = ["Cambria Math", "STIX Two Math", "Times New Roman", "Times", "serif"]
TEXT_FAMILIES = ["Segoe UI", "Helvetica Neue", "Arial", "sans-serif"]


def _font(families, size, italic=False, bold=False):
    f = QFont()
    f.setFamilies(families)
    f.setPointSizeF(size)
    f.setItalic(italic)
    f.setBold(bold)
    return f


class Box:
    __slots__ = ("w", "asc", "desc", "draw")

    def __init__(self, w, asc, desc, draw):
        self.w, self.asc, self.desc, self.draw = w, asc, desc, draw


def _text_box(s, font, color, dy=0.0):
    fm = QFontMetricsF(font)
    w = fm.horizontalAdvance(s)

    def draw(p, x, y):
        p.setFont(font)
        p.setPen(color)
        p.drawText(QPointF(x, y + dy), s)
    return Box(w, fm.ascent() - dy, fm.descent() + dy, draw)


class MathLayout:
    def __init__(self, size=12.0, color=QColor("black")):
        self.size, self.color = size, color

    def layout(self, items, size=None) -> Box:
        size = size or self.size
        boxes = []
        i = 0
        while i < len(items):
            it = items[i]
            kind = it[0]
            if kind == "text":
                boxes.extend(self._plain(it[1], size))
            elif kind == "styled":
                st = it[2]
                s = size * (0.7 if st.get("sub") or st.get("sup") else 1)
                f = _font(MATH_FAMILIES, s, italic=st.get("italic", False), bold=st.get("bold", False))
                dy = size * 0.3 if st.get("sub") else (-size * 0.45 if st.get("sup") else 0)
                boxes.append(_text_box(it[1], f, QColor(st.get("color") or self.color), dy))
            elif kind in ("sub", "sup"):
                inner = self.layout(it[1], size * 0.7)
                dy = size * 0.3 if kind == "sub" else -size * 0.45
                boxes.append(self._shift(inner, dy))
            elif kind == "pow":
                inner = self.layout(it[1], size * 0.7)
                boxes.append(self._shift(inner, -size * 0.45 - max(0.0, inner.desc - size * 0.2)))
            elif kind == "frac":
                boxes.append(self._frac(self.layout(it[1], size * 0.9), self.layout(it[2], size * 0.9), size))
            elif kind == "sqrt":
                boxes.append(self._sqrt(self.layout(it[1], size), size))
            elif kind == "paren":
                boxes.append(("paren", it[1]))
            i += 1
        return self._row(boxes, size)

    def _plain(self, s, size):
        """Letters italic, everything else upright (the handheld's default math styling)."""
        out, cur, cur_it = [], "", None
        for ch in s:
            it = ch.isalpha()
            if cur and it != cur_it:
                out.append(_text_box(cur, _font(MATH_FAMILIES, size, italic=cur_it), self.color))
                cur = ""
            cur, cur_it = cur + ch, it
        if cur:
            out.append(_text_box(cur, _font(MATH_FAMILIES, size, italic=cur_it), self.color))
        return out

    @staticmethod
    def _shift(inner, dy):
        def draw(p, x, y):
            inner.draw(p, x, y + dy)
        return Box(inner.w, inner.asc - dy, inner.desc + dy, draw)

    def _frac(self, num, den, size):
        pad, gap = size * 0.15, size * 0.15
        axis = size * 0.3
        w = max(num.w, den.w) + 2 * pad

        def draw(p, x, y):
            num.draw(p, x + (w - num.w) / 2, y - axis - gap - num.desc)
            den.draw(p, x + (w - den.w) / 2, y - axis + gap + den.asc)
            p.setPen(QPen(self.color, max(1.0, size / 14)))
            p.drawLine(QPointF(x + pad / 2, y - axis), QPointF(x + w - pad / 2, y - axis))
        return Box(w, axis + gap + num.asc + num.desc, den.asc + den.desc + gap - axis, draw)

    def _sqrt(self, inner, size):
        lead, top = size * 0.6, size * 0.15

        def draw(p, x, y):
            p.setPen(QPen(self.color, max(1.0, size / 14)))
            path = QPainterPath(QPointF(x, y - inner.asc * 0.4))
            path.lineTo(x + lead * 0.3, y - inner.asc * 0.5)
            path.lineTo(x + lead * 0.55, y + inner.desc)
            path.lineTo(x + lead * 0.9, y - inner.asc - top)
            path.lineTo(x + lead + inner.w + 2, y - inner.asc - top)
            p.drawPath(path)
            inner.draw(p, x + lead, y)
        return Box(lead + inner.w + 3, inner.asc + top + 1, inner.desc + 1, draw)

    def _row(self, boxes, size):
        # stretch parentheses to the tallest thing between each matching pair
        real = [b for b in boxes if isinstance(b, Box)]
        base_asc = max((b.asc for b in real), default=size * 0.8)
        base_desc = max((b.desc for b in real), default=size * 0.2)
        resolved, stack = list(boxes), []
        for idx, b in enumerate(boxes):
            if isinstance(b, tuple):
                if b[1] == "(":
                    stack.append(idx)
                elif stack:
                    o = stack.pop()
                    span = [x for x in boxes[o + 1:idx] if isinstance(x, Box)]
                    asc = max((x.asc for x in span), default=size * 0.75)
                    desc = max((x.desc for x in span), default=size * 0.25)
                    resolved[o] = self._paren("(", asc, desc, size)
                    resolved[idx] = self._paren(")", asc, desc, size)
        for idx, b in enumerate(resolved):
            if isinstance(b, tuple):
                resolved[idx] = self._paren(b[1], base_asc * 0.9, base_desc, size)
        w = sum(b.w for b in resolved)
        asc = max((b.asc for b in resolved), default=size * 0.8)
        desc = max((b.desc for b in resolved), default=size * 0.2)

        def draw(p, x, y):
            for b in resolved:
                b.draw(p, x, y)
                x += b.w
        return Box(w, asc, desc, draw)

    def _paren(self, ch, asc, desc, size):
        h = asc + desc
        w = size * 0.25 + h * 0.08

        def draw(p, x, y):
            p.setPen(QPen(self.color, max(1.0, size / 15)))
            path = QPainterPath()
            top, bot = y - asc, y + desc
            if ch == "(":
                path.moveTo(x + w * 0.8, top)
                path.quadTo(x + w * 0.05, (top + bot) / 2, x + w * 0.8, bot)
            else:
                path.moveTo(x + w * 0.2, top)
                path.quadTo(x + w * 0.95, (top + bot) / 2, x + w * 0.2, bot)
            p.drawPath(path)
        return Box(w, asc, desc, draw)


# ------------------------------------------------------------- page widget
class NotesView(QWidget):
    """Draws paragraphs with word wrap; math boxes get the handheld's grey background."""

    def __init__(self, parent=None, text_size=11.0, page_width=None):
        super().__init__(parent)
        self.paragraphs = []
        self.text_size = text_size
        self.page_width = page_width  # e.g. 320 to mimic the handheld screen
        self.error = None
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Minimum)
        self._height = 50

    def set_paragraphs(self, paragraphs, error=None):
        self.paragraphs, self.error = paragraphs, error
        self._height = self._layout(self.width() or 400, None)
        self.updateGeometry()
        self.update()

    def set_markup(self, paragraph_markups):
        paras, error = [], None
        for m in paragraph_markups:
            try:
                paras.append(markup_to_pieces(m))
            except MathSyntaxError as e:
                error = str(e)
                paras.append([("text", m, {"color": "#b00020"})])
        self.set_paragraphs(paras, error)

    def sizeHint(self):
        return QSize(self.page_width or 400, self._height)

    def minimumSizeHint(self):
        return QSize(120, self._height)

    def hasHeightForWidth(self):
        return True

    def heightForWidth(self, w):
        return self._layout(w, None)

    def resizeEvent(self, e):
        h = self._layout(self.width(), None)
        if h != self._height:
            self._height = h
            self.updateGeometry()
        super().resizeEvent(e)

    def paintEvent(self, _):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.fillRect(self.rect(), self.palette().base())
        self._layout(self.width(), p)
        p.end()

    def _layout(self, width, painter):
        margin = 8
        avail = max(60, (min(width, self.page_width) if self.page_width else width) - 2 * margin)
        y = margin
        text_color = self.palette().text().color()
        math = MathLayout(self.text_size + 1, text_color)
        for para in self.paragraphs:
            line, line_w, asc, desc = [], 0.0, 0.0, 0.0

            def flush():
                nonlocal y, line, line_w, asc, desc
                a = asc or QFontMetricsF(_font(TEXT_FAMILIES, self.text_size)).ascent()
                d = desc or QFontMetricsF(_font(TEXT_FAMILIES, self.text_size)).descent()
                if painter is not None:
                    x = margin
                    for box, is_math in line:
                        if is_math:
                            painter.fillRect(QRectF(x, y + a - box.asc - 1, box.w + 4, box.asc + box.desc + 2),
                                             QColor(200, 200, 200, 120))
                            box.draw(painter, x + 2, y + a)
                            x += box.w + 4
                        else:
                            box.draw(painter, x, y + a)
                            x += box.w
                y += a + d + 3
                line, line_w, asc, desc = [], 0.0, 0.0, 0.0

            for piece in para:
                if piece[0] == "math":
                    box = math.layout(piece[1])
                    units = [(box, True, box.w + 4)]
                else:
                    st = piece[2] if len(piece) > 2 else {}
                    size = st.get("size", self.text_size) * (0.7 if st.get("sub") or st.get("sup") else 1)
                    f = _font(TEXT_FAMILIES, size, italic=st.get("italic", False), bold=st.get("bold", False))
                    color = QColor(st.get("color") or text_color)
                    dy = size * 0.3 if st.get("sub") else (-size * 0.45 if st.get("sup") else 0)
                    units = []
                    for word in _split_words(piece[1]):
                        b = _text_box(word, f, color, dy)
                        units.append((b, False, b.w))
                for box, is_math, w in units:
                    if line and line_w + w > avail:
                        flush()
                    line.append((box, is_math))
                    line_w += w
                    asc, desc = max(asc, box.asc), max(desc, box.desc)
            flush()
        if self.error and painter is not None:
            painter.setPen(QColor("#b00020"))
            painter.setFont(_font(TEXT_FAMILIES, self.text_size - 1))
            painter.drawText(QPointF(margin, y + 12), f"Markup problem: {self.error}")
        return int(y + (24 if self.error else margin))


def _split_words(s):
    out, cur = [], ""
    for ch in s:
        cur += ch
        if ch == " ":
            out.append(cur)
            cur = ""
    if cur:
        out.append(cur)
    return out


def markup_to_pieces(markup, style=None):
    style = dict(style or {})
    pieces = []
    for piece in split_markup(markup):
        if piece[0] == "math":
            pieces.append(("math", parse_math(piece[1].lstrip("!"))))
        else:
            st = dict(style)
            if piece[2]:
                st["bold"] = True
            pieces.append(("text", piece[1], st))
    return pieces


# ------------------------------------------------------------ r2dtotree → model
def parse_r2d(tree_xml: str):
    """Turn a TI Notes r2dtotree (from any .tns) into paragraphs for NotesView."""
    root = ET.fromstring(tree_xml)
    formats = {}
    for fe in root.iter("formatEntry"):
        formats[fe.get("entryID")] = dict(
            bold=fe.get("fst") in ("1", "3"), italic=fe.get("fst") in ("2", "3"),
            sub=fe.get("fesub") == "1", sup=fe.get("fesup") == "1",
            color=_tc_color(fe.get("tc", "0")), math=fe.get("fn0", "").strip() == "TI-Nspire")
    paragraphs = []
    doc = next((n for n in root.iter("node") if n.get("name") == "1doc"), root)
    for para in doc:
        if para.get("name") != "1para":
            continue
        pieces = []
        _collect_para(para, formats, pieces)
        paragraphs.append(pieces)
    return paragraphs


def _tc_color(v):
    try:
        n = int(v)
    except ValueError:
        return None
    return None if n in (0, 1) else f"#{n & 0xFFFFFF:06x}"


def _runs(leaf, formats):
    text = "".join(leaf.itertext())
    n = int(leaf.get("np", "1") or 1)
    out, start = [], 0
    for k in range(n):
        end = int(leaf.get(f"pp{k}", len(text)) or 0)
        st = formats.get(leaf.get(f"id{k}", "0"), {})
        if end > start:
            out.append((text[start:end], st))
        start = end
    if start < len(text):
        out.append((text[start:], formats.get(leaf.get("id0", "0"), {})))
    return out


def _collect_para(node, formats, pieces):
    for child in node:
        name = child.get("name", "")
        if child.tag == "leaf" and name == "1word":
            for text, st in _runs(child, formats):
                pieces.append(("text", text, {k: v for k, v in st.items() if k != "math"}))
        elif child.tag == "node" and name == "0el" and child.get("flags") == "8":
            inner = next((c for c in child if c.tag == "node" and c.get("name") == "0el"), None)
            pieces.append(("math", _math_items(inner, formats) if inner is not None else []))
        elif child.tag == "node":
            _collect_para(child, formats, pieces)


def _math_items(el, formats):
    items = []
    for c in el:
        name = c.get("name", "")
        if c.tag == "leaf" and name == "0text":
            for text, st in _runs(c, formats):
                items.append(("styled", text, st))
        elif c.tag == "leaf" and name == "0paren":
            items.append(("paren", "".join(c.itertext()) or "("))
        elif c.tag == "node":
            kids = [k for k in c if k.tag == "node" and k.get("name") == "0el"]
            if name == "0fraction" and len(kids) >= 2:
                items.append(("frac", _math_items(kids[0], formats), _math_items(kids[1], formats)))
            elif name == "0sqrt" and kids:
                items.append(("sqrt", _math_items(kids[0], formats)))
            elif name == "0power" and kids:
                items.append(("pow", _math_items(kids[0], formats)))
            elif name == "0el":
                items.extend(_math_items(c, formats))
            else:  # a template we don't draw: show its linear text
                items.append(("text", html.unescape("".join(c.itertext()))))
    return items
