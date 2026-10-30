import html
import os
import xml.dom.minidom

import pytest

from calcbridge.tns import TnsDocument, build_lua, build_notes, build_python, build_text
from calcbridge.tns import format as tf
from calcbridge.tns.notes import MathSyntaxError, notes_card, parse_math

SAMPLES = os.environ.get("CALCBRIDGE_TI_SAMPLES")  # optional folder of TI-made .tns files (not shipped)


def test_crypto_roundtrip():
    for n in (0, 1, 7, 8, 9, 5000):
        plain = os.urandom(n)
        assert tf.decrypt(tf.encrypt(plain)) == plain


def test_tixc_roundtrip_including_many_tags():
    body = "".join(f"<item n=\"{i}\">v{i} ε√≤</item>" for i in range(400))
    xml_in = f'<prob xmlns="urn:TI.Problem" ver="1.0"><sym></sym><list>{body}</list><e/></prob>'
    out = tf.tixc_decode_xml(tf.tixc_encode(xml_in))
    dom = xml.dom.minidom.parseString(out.encode("utf-8"))
    items = dom.getElementsByTagName("item")
    assert len(items) == 400 and items[399].firstChild.data == "v399 ε√≤"
    assert dom.getElementsByTagName("e")


def test_container_roundtrip():
    entries = [tf.Entry("Document.xml", "<doc ver=\"1.0\"><nps>1</nps></doc>", tf.METHOD_TI),
               tf.Entry("Problem1.xml", "<prob><sym></sym></prob>", tf.METHOD_TI),
               tf.Entry("a.py", b"print(1)\n", tf.METHOD_DEFLATE)]
    blob = tf.write(entries)
    assert blob.startswith(b"*TIMLP0500") and b"TIPD" in blob
    version, back = tf.read(blob)
    assert version == 0x500
    assert [e.name for e in back] == ["Document.xml", "Problem1.xml", "a.py"]
    assert b"<nps>1</nps>" in back[0].data and back[2].data == b"print(1)\n"


def _inner_trees_wellformed(blob):
    doc = TnsDocument.from_bytes(blob)
    for _page, tree in doc.notes_trees():
        xml.dom.minidom.parseString(tree.encode("utf-8"))
    return doc


def test_notes_document():
    blob = build_notes([[("Title", dict(base_style=1, color=0x1F5FD0)),
                         r"**What:** $x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$ and $\sqrt{2}≈1.41$",
                         r"Hat $\hat{x}$, abs $\abs{x-y}$, ⋯ becomes $a+⋯+b^{2}$"],
                        ["Second page"]])
    doc = _inner_trees_wellformed(blob)
    assert [p.apps for p in doc.pages()] == [["Notes"], ["Notes"]]
    tree = doc.notes_trees()[0][1]
    for needle in ("0fraction", "0sqrt", "0power", "0paren", " '", "…", 'fesup="1"', 'fesub="1"'):
        assert needle in tree
    assert "⋯" not in tree


def test_math_syntax_errors():
    for bad in (r"\frac{1}", r"\nope{x}", "x^", "a}"):
        with pytest.raises(MathSyntaxError):
            parse_math(bad)
    with pytest.raises(MathSyntaxError):
        notes_card(["unclosed $x"])


def test_python_document():
    blob = build_python("hello.py", "print('hi')\r\n", extra={"helper.py": "X = 1\n"})
    doc = TnsDocument.from_bytes(blob)
    assert doc.python_files() == {"hello.py": "print('hi')\n", "helper.py": "X = 1\n"}
    assert doc.pages()[0].apps == ["Python editor"]
    with pytest.raises(ValueError):
        build_python("1bad name.py", "")


def test_lua_and_text_documents():
    doc = TnsDocument.from_bytes(build_lua('function on.paint(gc) gc:drawString("a]]>b", 10, 10) end'))
    assert doc.pages()[0].apps == ["Lua script"]
    assert "a]]>b" in doc.lua_script()
    doc = _inner_trees_wellformed(build_text("line with $ and **stars**\nsecond", title="T"))
    tree = doc.notes_trees()[0][1]
    text = html.unescape(tree)
    assert ">$ </leaf>" in text and ">**stars**</leaf>" in text and "0el" not in text


def test_summary():
    s = TnsDocument.from_bytes(build_python("p.py", "")).summary()
    assert "Python editor" in s and "p.py" in s


@pytest.mark.skipif(not SAMPLES, reason="set CALCBRIDGE_TI_SAMPLES to a folder of TI-made documents")
def test_reads_ti_made_documents():
    for name in os.listdir(SAMPLES):
        if name.endswith(".tns"):
            doc = TnsDocument.from_file(os.path.join(SAMPLES, name))
            assert doc.pages(), name
