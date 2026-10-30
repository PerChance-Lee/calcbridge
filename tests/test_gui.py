"""Drive the real main window against the simulated calculator (headless: QT_QPA_PLATFORM=offscreen)."""
import os

import pytest

pytest.importorskip("PySide6")

from PySide6.QtCore import QItemSelectionModel, QSettings  # noqa: E402
from PySide6.QtWidgets import QFileDialog, QInputDialog, QMessageBox  # noqa: E402

from calcbridge.gui import files_panel, main_window  # noqa: E402
from calcbridge.gui.main_window import MainWindow  # noqa: E402
from calcbridge.gui.worker import DeviceManager  # noqa: E402
from calcbridge.protocol import keys  # noqa: E402
from calcbridge.sim import SimCalculator, SimTransport  # noqa: E402
from calcbridge.tns import TnsDocument, build_python  # noqa: E402


@pytest.fixture
def app_env(qtbot, tmp_path, monkeypatch):
    monkeypatch.setattr("calcbridge.paths.seq_state_file", lambda: str(tmp_path / "state.json"))
    monkeypatch.setattr(QMessageBox, "question", staticmethod(lambda *a, **k: QMessageBox.Yes))
    monkeypatch.setattr(QMessageBox, "warning", staticmethod(lambda *a, **k: QMessageBox.Yes))
    monkeypatch.setattr(QMessageBox, "information", staticmethod(lambda *a, **k: QMessageBox.Ok))
    monkeypatch.setattr(QMessageBox, "about", staticmethod(lambda *a, **k: None))
    sim = SimCalculator(idle_drop_s=0)
    m = DeviceManager(transport_factory=lambda: SimTransport(sim))
    w = MainWindow(m, demo=True, settings=QSettings(str(tmp_path / "settings.ini"), QSettings.IniFormat))
    qtbot.addWidget(w)
    w.files.local.set_dir(str(tmp_path))
    m.connect_device()
    qtbot.waitUntil(lambda: m.calc is not None and not m.busy, timeout=10000)
    qtbot.waitUntil(lambda: bool(w.files.remote.entries), timeout=5000)
    yield w, m, sim, tmp_path
    m.shutdown()


def idle(qtbot, m):
    qtbot.waitUntil(lambda: not m.busy, timeout=15000)


def names(w):
    return {e.name for e in w.files.remote.entries}


def select(w, *wanted):
    tree = w.files.remote.tree
    tree.clearSelection()
    model = w.files.remote.proxy
    for row in range(model.rowCount()):
        idx = model.index(row, 0)
        if w.files.remote.item_at(idx).data(256 + 1).rsplit("/", 1)[-1] in wanted:
            flags = QItemSelectionModel.SelectionFlag.Select | QItemSelectionModel.SelectionFlag.Rows
            tree.selectionModel().select(idx, flags)


def test_connects_and_shows_device(app_env):
    w, m, sim, _ = app_env
    assert w.device.fields["name"].text() == "Simulated CX II"
    assert {"MyLib", "PyLib", "Examples", "themes.csv"} <= names(w)
    assert w.a_upload.isEnabled()


def test_folder_navigation(app_env, qtbot):
    w, m, sim, _ = app_env
    w.open_folder("/Examples")
    idle(qtbot, m)
    assert w.files.remote.path == "/Examples" and "Getting Started.tns" in names(w)
    w.files.remote.go_up()
    idle(qtbot, m)
    assert w.files.remote.path == "/"


def test_upload_tns_python_conversion_and_overwrite(app_env, qtbot, monkeypatch):
    w, m, sim, tmp = app_env
    (tmp / "doc.tns").write_bytes(build_python("x.py", "print(1)"))
    (tmp / "prog.py").write_text("print('hi')\n")
    (tmp / "photo.jpg").write_bytes(b"\xff\xd8")
    w.upload([str(tmp / "doc.tns"), str(tmp / "prog.py"), str(tmp / "photo.jpg")])
    idle(qtbot, m)
    assert {"doc.tns", "prog.tns"} <= names(w) and "photo.jpg" not in names(w)
    assert TnsDocument.from_bytes(sim.get("/prog.tns").data).python_files() == {"prog.py": "print('hi')\n"}
    # second upload of the same file: answer "Yes to All"
    asked = []
    monkeypatch.setattr(main_window, "ask_overwrite", lambda parent, name: asked.append(name) or "all")
    (tmp / "doc.tns").write_bytes(build_python("x.py", "print(2)"))
    w.upload([str(tmp / "doc.tns")])
    idle(qtbot, m)
    assert asked == ["doc.tns"]
    assert "print(2)" in TnsDocument.from_bytes(sim.get("/doc.tns").data).python_files()["x.py"]


def test_upload_folder_and_download(app_env, qtbot):
    w, m, sim, tmp = app_env
    src = tmp / "pack"
    (src / "inner").mkdir(parents=True)
    (src / "a.tns").write_bytes(b"A" * 5000)
    (src / "inner" / "b.tns").write_bytes(b"B")
    w.upload([str(src)])
    idle(qtbot, m)
    assert sim.get("/pack/inner/b.tns").data == b"B"
    out = tmp / "out"
    out.mkdir()
    w.download_to(["/pack"], str(out))
    idle(qtbot, m)
    assert (out / "pack" / "a.tns").read_bytes() == b"A" * 5000


def test_new_folder_rename_delete(app_env, qtbot, monkeypatch):
    w, m, sim, _ = app_env
    monkeypatch.setattr(QInputDialog, "getText", staticmethod(lambda *a, **k: (k.get("text") or "Folder1", True)))
    w.new_folder()
    idle(qtbot, m)
    assert "Folder1" in names(w)
    sim.put("/note.tns", b"n")
    w.refresh()
    idle(qtbot, m)
    select(w, "note.tns")
    monkeypatch.setattr(QInputDialog, "getText", staticmethod(lambda *a, **k: ("renamed", True)))
    w.rename_selected()
    idle(qtbot, m)
    assert "renamed.tns" in names(w)  # .tns is kept
    select(w, "renamed.tns", "Folder1")
    w.delete_selected()
    idle(qtbot, m)
    assert not {"renamed.tns", "Folder1"} & names(w)


def test_copy_paste_cut(app_env, qtbot):
    w, m, sim, _ = app_env
    select(w, "themes.csv")
    assert w.files.remote.selected_paths() == ["/themes.csv"]
    w.duplicate_selected()
    idle(qtbot, m)
    assert "Copy of themes.csv" in names(w), w.log.toPlainText()
    select(w, "Copy of themes.csv")
    w._clip("cut")
    w.open_folder("/MyLib")
    idle(qtbot, m)
    w.paste()
    idle(qtbot, m)
    assert "Copy of themes.csv" in names(w)
    assert sim.get("/Copy of themes.csv") is None


def test_capture_and_live_view(app_env, qtbot):
    w, m, sim, _ = app_env
    w.capture()
    qtbot.waitUntil(lambda: w.screen.gallery.count() == 1, timeout=5000)
    img = w.screen.gallery.item(0).data(256)
    assert (img.width(), img.height()) == (320, 240)
    assert w.screen.gallery.item(0).text().endswith("Image 001")
    w.screen.live.setChecked(True)
    qtbot.waitUntil(lambda: w.screen.live_image is not None, timeout=5000)
    w.screen.live.setChecked(False)
    assert w.screen.gallery.count() == 1  # live frames don't fill the gallery


def test_keypad_sends_keys(app_env, qtbot):
    w, m, sim, _ = app_env
    w.press_key("esc")
    w.type_text("2+2")
    idle(qtbot, m)
    assert sim.keys_pressed[:4] == [keys.KEYS["esc"], keys.KEYS["2"], keys.KEYS["+"], keys.KEYS["2"]]


def test_create_notes_and_python_module(app_env, qtbot):
    w, m, sim, _ = app_env
    w.create.kind.setCurrentIndex(0)
    w.create.name.setText("Formulas")
    w.create.editor.setPlainText(r"Newton $x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$")
    w.create._send(False)
    idle(qtbot, m)
    doc = TnsDocument.from_bytes(sim.get("/Formulas.tns").data)
    assert doc.pages()[0].apps == ["Notes"] and "0fraction" in doc.notes_trees()[0][1]
    w.create.kind.setCurrentIndex(1)
    w.create.name.setText("my tools")
    w.create.editor.setPlainText("def sq(x):\n    return x*x\n")
    w.create._send(True)
    idle(qtbot, m)
    assert sim.get("/PyLib/my_tools.tns") is not None


def test_viewer_opens_handheld_document(app_env, qtbot):
    w, m, sim, _ = app_env
    sim.put("/prog.tns", build_python("prog.py", "print(42)\n"))
    w.open_remote_document("/prog.tns")
    qtbot.waitUntil(lambda: w.viewer.doc is not None, timeout=5000)
    assert w.viewer.pages.count() >= 1 and w.tabs.currentWidget() is w.viewer
    assert "print(42)" in w.viewer.code.toPlainText()


def test_backup_restore(app_env, qtbot, monkeypatch):
    w, m, sim, tmp = app_env
    monkeypatch.setattr(QFileDialog, "getExistingDirectory", staticmethod(lambda *a, **k: str(tmp)))
    w.backup()
    idle(qtbot, m)
    backups = [p for p in os.listdir(tmp) if "backup" in p]
    assert backups and os.path.exists(os.path.join(tmp, backups[0], "themes.csv"))


def test_install_os_checks_extension(app_env, qtbot, monkeypatch, tmp_path):
    w, m, sim, _ = app_env
    errors = []
    monkeypatch.setattr(w, "error", lambda msg, title=None: errors.append(msg))
    bad = tmp_path / "os.tcc2"
    bad.write_bytes(b"TI-Nspire" + bytes(5000))
    w.install_os(str(bad))
    assert errors and ".tco2" in errors[0]
    good = tmp_path / "os.tco2"
    good.write_bytes(b"TI-Nspire" + bytes(5000))
    w.install_os(str(good))
    idle(qtbot, m)
    assert sim.os_installed == good.read_bytes()


def test_unplug_shows_disconnected(app_env, qtbot, monkeypatch):
    w, m, sim, _ = app_env
    monkeypatch.setattr(w, "error", lambda msg, title=None: None)
    sim.unplug()
    w.refresh()
    qtbot.waitUntil(lambda: m.calc is None, timeout=10000)
    assert not w.a_upload.isEnabled()
    assert "Connected" not in w.device.state.text()


def test_drop_files_from_desktop_onto_handheld(app_env, qtbot):
    from PySide6.QtCore import QMimeData, QPointF, Qt, QUrl
    from PySide6.QtGui import QDropEvent
    w, m, sim, tmp = app_env
    f = tmp / "dropped.tns"
    f.write_bytes(b"D" * 100)
    mime = QMimeData()
    mime.setUrls([QUrl.fromLocalFile(str(f))])
    ev = QDropEvent(QPointF(5, 5), Qt.CopyAction, mime, Qt.LeftButton, Qt.NoModifier)
    # dropping on a folder row puts the file in that folder; on empty space, in the current folder
    target = w.files.remote.tree._target_folder(QPointF(5, 5).toPoint())
    w.files.remote.tree.dropEvent(ev)
    idle(qtbot, m)
    assert sim.get(f"{target.rstrip('/')}/dropped.tns").data == b"D" * 100


def test_drag_handheld_items_to_computer(app_env, qtbot):
    import json

    from PySide6.QtCore import QMimeData, QPointF, Qt
    from PySide6.QtGui import QDropEvent
    w, m, sim, tmp = app_env
    mime = QMimeData()
    mime.setData(files_panel.REMOTE_MIME, json.dumps(["/themes.csv"]).encode())
    ev = QDropEvent(QPointF(5, 5), Qt.CopyAction, mime, Qt.LeftButton, Qt.NoModifier)
    w.files.local.tree.dropEvent(ev)
    idle(qtbot, m)
    assert (tmp / "themes.csv").read_bytes() == sim.get("/themes.csv").data


def test_overwrite_prompt_choices(qtbot, monkeypatch):
    from PySide6.QtWidgets import QWidget
    parent = QWidget()
    qtbot.addWidget(parent)

    def fake_exec(box):
        box._clicked = [b for b in box.buttons() if b.text() == "Yes to All"][0]
        return 0
    monkeypatch.setattr(QMessageBox, "exec", fake_exec)
    monkeypatch.setattr(QMessageBox, "clickedButton", lambda box: box._clicked)
    assert files_panel.ask_overwrite(parent, "x.tns") == "all"
    assert files_panel.unique_copy_name("a.tns", {"Copy of a.tns"}) == "Copy of a (2).tns"
    assert files_panel.next_folder_name({"Folder1", "Folder2"}) == "Folder3"
