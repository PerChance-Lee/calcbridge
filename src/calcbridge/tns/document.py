"""Create and inspect TI-Nspire documents."""
from __future__ import annotations

import datetime
import html
import re
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field

from . import format as tnsformat
from .format import METHOD_DEFLATE, METHOD_TI, Entry
from .notes import notes_card

APP_NAMES = {
    "TI.Notepad": "Notes", "TI.Calculator": "Calculator", "TI.Scratchpad": "Scratchpad",
    "TI.GeoGrapher": "Graphs & Geometry", "TI.DataGrapher": "Data & Statistics", "TI.ListsSpreadsheet": "Lists & Spreadsheet",
    "TI.Spreadsheet": "Lists & Spreadsheet", "TI.PythonEditor": "Python editor", "TI.PythonShell": "Python shell",
    "TI.ScriptApp": "Lua script", "TI.Question": "Question", "TI.DataQuest": "Vernier DataQuest",
    "TI.ChemBox": "Chemistry",
}


def document_xml(problems: int = 1) -> str:
    """Document.xml with the settings TI uses for a new non-CAS document."""
    today = datetime.date.today()
    return ('<doc ver="1.0"><properties><product>16</product><platform>2</platform>'
            f'<swver>5.4.0 CalcBridge</swver><date>{today.year}-{today.month}-{today.day}</date></properties>'
            '<settings><assessmentMode>0</assessmentMode><lastview>1</lastview><readOnlyMode>0</readOnlyMode>'
            '<lang>en</lang><dfmt>0</dfmt><tfmt>0</tfmt><curr>0</curr><expf>1</expf><ddig>7</ddig><angf>1</angf>'
            '<exapp>1</exapp><cplxf>1</cplxf><unit>1</unit><vectf>1</vectf><base>1</base><gg_ddig>4</gg_ddig>'
            '<gg_graphang>1</gg_graphang><gg_geomang>2</gg_geomang><gg_axisendv>1</gg_axisendv>'
            '<gg_tooltipfunman>0</gg_tooltipfunman><gg_autopoi>1</gg_autopoi><gg_calcmenu>0</gg_calcmenu>'
            '<gg_hideplotlabels>0</gg_hideplotlabels></settings>'
            '<cardIntendedData><cardIntendedWidth>320</cardIntendedWidth><cardIntendedHeight>217</cardIntendedHeight>'
            '<cardIntendedDensity>125.000000</cardIntendedDensity></cardIntendedData>'
            f'<rights></rights><nps>{problems}</nps><imgsize>0</imgsize></doc>')


def _problem(cards: list[str]) -> str:
    return f'<prob xmlns="urn:TI.Problem" ver="1.0" pbname=""><sym></sym>{"".join(cards)}</prob>'


def _card(widget: str) -> str:
    return ('<card clay="0" h1="10000" h2="10000" w1="10000" w2="10000"><isDummyCard>0</isDummyCard>'
            f'<flag>0</flag>{widget}</card>')


# ------------------------------------------------------------------ builders
def build_notes(pages: list[list]) -> bytes:
    """pages: one list of paragraph markups per Notes page (see notes.py for the markup)."""
    return tnsformat.write([Entry("Document.xml", document_xml(), METHOD_TI),
                            Entry("Problem1.xml", _problem([notes_card(p) for p in pages]), METHOD_TI)])


def build_python(script_name: str, code: str, extra: dict[str, str] | None = None) -> bytes:
    """A Python program document. script_name must end in .py; it is the one that opens first."""
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*\.py", script_name):
        raise ValueError("Python file names must be a valid identifier ending in .py (e.g. my_prog.py)")
    widget = ('<wdgt xmlns:py="urn:TI.PythonEditor" type="TI.PythonEditor" ver="1.0"><py:data><py:name>'
              f'{html.escape(script_name)}</py:name><py:dirf>0</py:dirf></py:data><py:mFlags>0</py:mFlags>'
              '<py:value>10</py:value></wdgt>')
    entries = [Entry("Document.xml", document_xml(), METHOD_TI), Entry("Problem1.xml", _problem([_card(widget)]), METHOD_TI),
               Entry(script_name, code.replace("\r\n", "\n").encode("utf-8"), METHOD_DEFLATE)]
    for name, src in (extra or {}).items():
        entries.append(Entry(name, src.replace("\r\n", "\n").encode("utf-8"), METHOD_DEFLATE))
    return tnsformat.write(entries)


def build_lua(code: str) -> bytes:
    """A Lua script document (TI.ScriptApp, API level 2 as Luna produces)."""
    code = code.replace("]]>", "]]]]><![CDATA[>")
    widget = ('<wdgt xmlns:sc="urn:TI.ScriptApp" type="TI.ScriptApp" ver="1.0"><sc:mFlags>0</sc:mFlags>'
              f'<sc:value>-1</sc:value><sc:script version="512" id="0"><![CDATA[{code}]]></sc:script></wdgt>')
    return tnsformat.write([Entry("Document.xml", document_xml(), METHOD_TI),
                            Entry("Problem1.xml", _problem([_card(widget)]), METHOD_TI)])


def build_text(text: str, title: str | None = None) -> bytes:
    """Plain text -> one Notes page (no math markup interpretation)."""
    paras = []
    if title:
        paras.append((title, dict(base_style=1, color=0x1F5FD0, raw=True)))
    paras += [(line, dict(raw=True)) for line in text.replace("\r\n", "\n").split("\n")]
    return build_notes([paras])


# ---------------------------------------------------------------- inspection
@dataclass
class Page:
    problem: int
    index: int
    apps: list[str]
    widgets: list[ET.Element] = field(repr=False, default_factory=list)


@dataclass
class TnsDocument:
    version: int
    entries: list[Entry]

    @classmethod
    def from_bytes(cls, blob: bytes) -> "TnsDocument":
        version, entries = tnsformat.read(blob)
        return cls(version, entries)

    @classmethod
    def from_file(cls, path: str) -> "TnsDocument":
        with open(path, "rb") as f:
            return cls.from_bytes(f.read())

    def entry(self, name):
        return next((e for e in self.entries if e.name == name), None)

    @property
    def problems(self) -> list[str]:
        names = sorted((e.name for e in self.entries if re.fullmatch(r"Problem\d+\.xml", e.name)),
                       key=lambda n: int(re.findall(r"\d+", n)[0]))
        return names

    def pages(self) -> list[Page]:
        out = []
        for pi, name in enumerate(self.problems, 1):
            root = _parse_xml(self.entry(name).data)
            if root is None:
                continue
            for ci, card in enumerate(_iter(root, "card"), 1):
                widgets = list(_iter(card, "wdgt"))
                out.append(Page(pi, ci, [APP_NAMES.get(w.get("type", ""), w.get("type", "?")) for w in widgets],
                                widgets))
        return out

    def python_files(self) -> dict[str, str]:
        return {e.name: e.data.decode("utf-8", "replace") for e in self.entries if e.name.endswith(".py")}

    def notes_trees(self) -> list[tuple[Page, str]]:
        """(page, r2dtotree XML) for every Notes widget, for viewers."""
        out = []
        for page in self.pages():
            for w in page.widgets:
                for el in w.iter():
                    if el.tag.endswith("fmtxt") and el.text:
                        out.append((page, el.text))
        return out

    def lua_script(self) -> str | None:
        for page in self.pages():
            for w in page.widgets:
                for el in w.iter():
                    if el.tag.endswith("script") and el.text:
                        return el.text
        return None

    def summary(self) -> str:
        lines = [f"TI-Nspire document, format version {self.version:04X}"]
        for p in self.pages():
            lines.append(f"  Problem {p.problem}, page {p.index}: {', '.join(p.apps) or 'empty'}")
        for name in self.python_files():
            lines.append(f"  Python file: {name}")
        others = [e.name for e in self.entries if not e.name.endswith((".xml", ".py"))]
        if others:
            lines.append(f"  Other files: {', '.join(others)}")
        return "\n".join(lines)


def _parse_xml(data: bytes):
    text = data.decode("utf-8", "replace")
    text = re.sub(r"<\?xml[^>]*\?>", "", text, count=1)
    try:
        return ET.fromstring(text)
    except ET.ParseError:
        return None


def _iter(root, local):
    for el in root.iter():
        if el.tag == local or el.tag.endswith("}" + local):
            yield el
