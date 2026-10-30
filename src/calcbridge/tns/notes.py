"""Build Notes pages (TI.Notepad ver 2.0, an "r2dtotree" rich-text tree) with real 2D math boxes.

Paragraph markup:   **bold**   text   $math$   ($!math$ = box with warning indicators off)
Math markup (LaTeX-like):
    \\frac{a}{b}   \\sqrt{a}   x^{n} or x^2   x_{k+1} or x_k   ( )   \\abs{a}   \\hat{x}
Letters are italic, digits/operators upright, and known words (sin, cos, ln, lim, ...) upright.

The node and template names mirror TI's own documents. Rendering facts checked on a CX II (OS 5.4):
the fonts lack ⋯ and combining/modifier hats (use … and \\hat, a raised caret); an italic f hides a
prime drawn right after it (a no-break space is inserted); math boxes wrap at plain spaces.
"""
from __future__ import annotations

from html import escape

NO_FILL = 268435199
TEXT_FONT, MATH_FONT = "TI-Nspire Sans", "TI-Nspire"
UPRIGHT_WORDS = ("sin", "cos", "tan", "ln", "log", "lim", "max", "min", "abs", "fl", "DP", "exp", "det")
FONT_SUBSTITUTIONS = {"⋯": "…", "′": "'"}


class Formats:
    """The formatManager table: one entry per distinct (font, style, colour, size, sub, sup)."""

    def __init__(self):
        self.keys = [(TEXT_FONT, 0, 0, 11, 0, 0)]
        self.refs = [2147483647]

    def get(self, font=TEXT_FONT, style=0, color=0, size=11, sub=0, sup=0):
        key = (font, style, color, size, sub, sup)
        if key not in self.keys:
            self.keys.append(key)
            self.refs.append(0)
        i = self.keys.index(key)
        if i:
            self.refs[i] += 1
        return i

    def xml(self):
        out = []
        for i, (font, style, color, size, sub, sup) in enumerate(self.keys):
            out.append(f'<formatEntry entryIndex="{i}" entryID="{i}" entryRefCnt="{self.refs[i]}" tc="{color}" '
                       f'fc="{NO_FILL}" fs="{size}" fst="{style}" cc="0" fest="0" feun="0" fesub="{sub}" '
                       f'fesup="{sup}" fn0="{escape(font)}"></formatEntry>')
        cap = max(10, -(-len(self.keys) // 10) * 10)
        return f'<formatManager tableSize="{len(self.keys)}" capacity="{cap}">{"".join(out)}</formatManager>'


def _leaf(name, runs):
    runs = [(f, t) for f, t in runs if t] or [(0, "")]
    attrs, pos = [], 0
    for k, (f, t) in enumerate(runs):
        pos += len(t)
        attrs.append(f'id{k}="{f}" pp{k}="{pos}"')
    ucf = ' ucf="1"' if any(f for f, _ in runs) else ""
    text = escape("".join(t for _, t in runs), quote=False)
    head = f'<leaf name="{name}" np="{len(runs)}" {" ".join(attrs)}{ucf}'
    return head + (f">{text}</leaf>" if text else "/>")


class MathSyntaxError(ValueError):
    pass


class _MathParser:
    def __init__(self, src, fmt, sub=0, sup=0):
        self.s, self.i, self.fmt, self.sub, self.sup = src, 0, fmt, sub, sup

    def style_runs(self, text):
        runs, i = [], 0
        while i < len(text):
            word = next((w for w in UPRIGHT_WORDS if text.startswith(w, i) and
                         not (i + len(w) < len(text) and text[i + len(w)].isalpha() and w not in ("fl", "DP"))),
                        None)
            if word:
                ch, style, i = word, 0, i + len(word)
            else:
                ch = text[i]
                style = 2 if ch.isalpha() else 0
                i += 1
            f = self.fmt.get(MATH_FONT, style, 0, 11, self.sub, self.sup)
            if runs and runs[-1][0] == f:
                runs[-1] = (f, runs[-1][1] + ch)
            else:
                runs.append((f, ch))
        return runs

    def group(self):
        if self.i >= len(self.s):
            raise MathSyntaxError("expected an argument at the end of the formula")
        if self.s[self.i] == "{":
            self.i += 1
            items = self.seq(stop="}")
            if self.i >= len(self.s):
                raise MathSyntaxError("missing }")
            self.i += 1
            return items
        ch = self.s[self.i]
        self.i += 1
        return [("text", ch)]

    def seq(self, stop=None):
        items = []
        while self.i < len(self.s) and (stop is None or self.s[self.i] != stop):
            c = self.s[self.i]
            if c == "\\":
                j = self.i + 1
                while j < len(self.s) and self.s[j].isalpha():
                    j += 1
                cmd, self.i = self.s[self.i + 1:j], j
                if cmd == "frac":
                    items.append(("frac", self.group(), self.group()))
                elif cmd == "sqrt":
                    items.append(("sqrt", self.group()))
                elif cmd == "abs":
                    items += [("text", "|")] + self.group() + [("text", "|")]
                elif cmd == "hat":
                    items += self.group() + [("sup", [("text", "^")])]
                else:
                    raise MathSyntaxError(f"unknown command \\{cmd}")
            elif c == "^":
                self.i += 1
                items.append(("pow", self.group()))
            elif c == "_":
                self.i += 1
                items.append(("sub", self.group()))
            elif c in "()":
                self.i += 1
                items.append(("paren", c))
            elif c == "'" and items and items[-1][0] == "text" and items[-1][1].isalpha():
                self.i += 1
                items += [("text", " "), ("text", c)]  # keep the prime clear of the italic f
            elif c == "}":
                raise MathSyntaxError("unexpected }")
            else:
                self.i += 1
                items.append(("text", FONT_SUBSTITUTIONS.get(c, c)))
        return items

    @staticmethod
    def flatten(items):
        out = []
        for it in items:
            kind = it[0]
            if kind in ("text", "paren"):
                out.append(it[1])
            elif kind == "frac":
                out.append(f"({_MathParser.flatten(it[1])})/({_MathParser.flatten(it[2])})")
            elif kind == "sqrt":
                out.append(f"√({_MathParser.flatten(it[1])})")
            elif kind == "pow":
                out.append("^" + _MathParser.flatten(it[1]))
            elif kind in ("sub", "sup"):
                out.append(_MathParser.flatten(it[1]))
        return "".join(out)

    def el(self, items):
        parts, runs = [], []

        def flush():
            parts.append(_leaf("0text", runs[:]))
            runs.clear()

        for it in items:
            kind = it[0]
            if kind == "text":
                runs.extend(self.style_runs(it[1]))
            elif kind in ("sub", "sup"):
                p = _MathParser("", self.fmt, sub=int(kind == "sub"), sup=int(kind == "sup"))
                runs.extend(p.style_runs(self.flatten(it[1])))
            else:
                flush()
                if kind == "paren":
                    data = 0 if it[1] == "(" else 2147483648
                    parts.append(f'<leaf name="0paren" id0="0" ucf="1" data="{data}">{it[1]}</leaf>')
                elif kind == "pow":
                    parts.append(f'<node name="0power" id0="0" ucf="1">^({self.el(it[1])})</node>')
                elif kind == "frac":
                    parts.append(f'<node name="0fraction" id0="0">(({self.el(it[1])})/({self.el(it[2])}))</node>')
                elif kind == "sqrt":
                    parts.append(f'<node name="0sqrt" id0="0">√({self.el(it[1])})</node>')
        flush()
        return f'<node name="0el" id0="0">{"".join(parts)}</node>'


def parse_math(src: str):
    """Parse math markup into the item tree (raises MathSyntaxError). Used by the GUI preview too."""
    p = _MathParser(src, Formats())
    items = p.seq()
    if p.i < len(src):
        raise MathSyntaxError("unexpected }")
    return items


def math_box(src, fmt, warn=True):
    p = _MathParser(src, fmt)
    items = p.seq()
    if p.i < len(src):
        raise MathSyntaxError("unexpected }")
    return ('<node name="0el" flags="8" id0="0"><mbox><attr iomode="1" digits="0" angle="0" symbol="1" '
            f'wrap="true" warn="{str(warn).lower()}" comp="1" active="false" id0="0"/>'
            f'<out><err>0</err></out></mbox>{p.el(items)}</node>')


def _words(text, f):
    out, cur = [], ""
    for ch in text:
        cur += ch
        if ch == " ":
            out.append(_leaf("1word", [(f, cur)]))
            cur = ""
    if cur:
        out.append(_leaf("1word", [(f, cur)]))
    return out


def split_markup(markup: str):
    """-> list of ("math", src) / ("text", str, bold) pieces."""
    pieces = []
    for k, chunk in enumerate(markup.split("$")):
        if k % 2:
            pieces.append(("math", chunk))
            continue
        for j, piece in enumerate(chunk.split("**")):
            if piece:
                pieces.append(("text", piece, bool(j % 2)))
    if markup.count("$") % 2:
        raise MathSyntaxError("a $ math section isn't closed")
    return pieces


def paragraph(markup, fmt, color=0, size=11, base_style=0, raw=False):
    """raw=True takes the text literally ($ and ** are not markup)."""
    leaves = []
    for piece in ([("text", markup, False)] if raw else split_markup(markup)):
        if piece[0] == "math":
            src = piece[1]
            leaves.append(_leaf("1word", [(0, "")]))
            leaves.append(math_box(src.lstrip("!"), fmt, warn=not src.startswith("!")))
            leaves.append(_leaf("1word", [(0, "")]))
        else:
            style = base_style | (1 if piece[2] else 0)
            f = fmt.get(TEXT_FONT, style, color, size) if (style or color or size != 11) else 0
            text = "".join(FONT_SUBSTITUTIONS.get(c, c) for c in piece[1])
            leaves += _words(text, f)
    leaves.append(_leaf("1word", [(0, "")]))
    return f'<node name="1para"><node name="1rtline">{"".join(leaves)}</node></node>'


def notes_card(paragraphs) -> str:
    """paragraphs: markup strings, or (markup, dict(color=, size=, base_style=)) tuples."""
    fmt = Formats()
    body = []
    for p in paragraphs or [""]:
        markup, opts = (p, {}) if isinstance(p, str) else p
        body.append(paragraph(markup, fmt, **opts))
    first = body[0]
    i = first.find('<leaf name="1word"')
    j = first.find(">", i)
    if first[j - 1] == "/":
        first = first[:j - 1] + '><cursor index="0"/></leaf>' + first[j + 1:]
    else:
        first = first[:j + 1] + '<cursor index="0"/>' + first[j + 1:]
    body[0] = first
    tree = f'<r2dtotree version="1">{fmt.xml()}<node name="1doc">{"".join(body)}</node></r2dtotree>'
    return ('<card clay="0" h1="10000" h2="10000" w1="10000" w2="10000"><isDummyCard>0</isDummyCard>'
            '<flag>0</flag><wdgt xmlns:np="urn:TI.Notepad" type="TI.Notepad" ver="2.0">'
            f'<np:mFlags>0</np:mFlags><np:value>3</np:value><np:fmtxt>{escape(tree)}</np:fmtxt></wdgt></card>')
