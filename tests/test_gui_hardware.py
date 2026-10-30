"""The real main window against a real CX II (pytest --hardware). Works in a scratch folder it removes."""
import time

import pytest

pytestmark = pytest.mark.hardware

pytest.importorskip("PySide6")
from PySide6.QtCore import QSettings  # noqa: E402
from PySide6.QtWidgets import QMessageBox  # noqa: E402

from calcbridge.gui.main_window import MainWindow  # noqa: E402
from calcbridge.gui.worker import DeviceManager  # noqa: E402
from calcbridge.tns import TnsDocument  # noqa: E402


def test_gui_end_to_end_on_hardware(qtbot, tmp_path, monkeypatch):
    errors = []
    monkeypatch.setattr(QMessageBox, "question", staticmethod(lambda *a, **k: QMessageBox.Yes))
    monkeypatch.setattr(QMessageBox, "warning", staticmethod(lambda *a, **k: errors.append(a[2]) or QMessageBox.Ok))
    monkeypatch.setattr(QMessageBox, "information", staticmethod(lambda *a, **k: QMessageBox.Ok))
    m = DeviceManager()
    w = MainWindow(m, settings=QSettings(str(tmp_path / "s.ini"), QSettings.IniFormat))
    qtbot.addWidget(w)
    idle = lambda: qtbot.waitUntil(lambda: not m.busy, timeout=60000)  # noqa: E731
    try:
        m.start_polling()
        qtbot.waitUntil(lambda: m.calc is not None, timeout=90000)
        idle()
        assert w.device.fields["name"].text() not in ("", "—")
        assert {"MyLib", "PyLib"} <= {e.name for e in w.files.remote.entries}

        scratch = f"/calcbridge-gui-{int(time.time())}"
        m.submit(lambda c: c.mkdir(scratch))
        idle()
        w.open_folder(scratch)
        idle()
        assert w.files.remote.path == scratch and w.files.remote.entries == []

        w.capture()
        qtbot.waitUntil(lambda: w.screen.gallery.count() == 1, timeout=30000)
        img = w.screen.gallery.item(0).data(256)
        assert (img.width(), img.height()) == (320, 240)

        w.press_key("esc")
        idle()

        w.create.kind.setCurrentIndex(0)
        w.create.name.setText("GUI test")
        w.create.editor.setPlainText(r"**GUI test** $\frac{1}{2}+\sqrt{x}$")
        w.create._send(False)
        idle()
        assert "GUI test.tns" in {e.name for e in w.files.remote.entries}

        w.open_remote_document(f"{scratch}/GUI test.tns")
        qtbot.waitUntil(lambda: w.viewer.doc is not None, timeout=30000)
        assert isinstance(w.viewer.doc, TnsDocument) and w.viewer.pages.count() == 1

        m.submit(lambda c: c.delete(scratch))
        idle()
        assert not errors, errors
    finally:
        m.shutdown()
