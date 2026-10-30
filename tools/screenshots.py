"""Render each tab of the app (against the simulator) to PNGs: python tools/screenshots.py OUTDIR"""
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))
from PySide6.QtWidgets import QApplication  # noqa: E402

from calcbridge.gui.main_window import MainWindow  # noqa: E402
from calcbridge.gui.worker import DeviceManager  # noqa: E402
from calcbridge.sim import SimCalculator, SimTransport  # noqa: E402
from calcbridge.tns import build_notes, build_python  # noqa: E402

out = sys.argv[1] if len(sys.argv) > 1 else "screenshots"
os.makedirs(out, exist_ok=True)
app = QApplication([])
from PySide6.QtWidgets import QMessageBox  # noqa: E402

for _name in ("warning", "information", "question", "critical"):
    setattr(QMessageBox, _name, staticmethod(lambda *a, _n=_name, **k: print(f"[{_n}]", *a[1:3], flush=True)
                                             or QMessageBox.Yes))
sim = SimCalculator(idle_drop_s=0)
sim.put("/equations/05 Taylor polynomial.tns", build_notes([[
    ("5  Taylor polynomial $T_{n}$", dict(base_style=1, color=0x1F5FD0)),
    r"**What:** $T_{n}(x)=f(a)+f'(a)(x-a)+\frac{f''(a)}{2!}(x-a)^{2}+⋯+\frac{f^{(n)}(a)}{n!}(x-a)^{n}$",
    r"**Try:** Find $T_{2}$ for $f(x)=\sqrt{x}$ at $a=4$, then estimate $\sqrt{4.1}$."]]))
sim.put("/equations/18 myNewton.tns", build_python("mynewton.py", "def myNewton(f, fp, x0, tau, nmax):\n    return x0\n"))
real = os.path.join(os.path.dirname(__file__), "..", "docs", "images", "handheld-notes.png")
if os.path.exists(real):  # show a real handheld screen instead of the simulator's test pattern
    from PySide6.QtGui import QImage  # noqa: E402
    img = QImage(real).convertToFormat(QImage.Format_RGB16)
    sim.screen = bytes(img.constBits())[:320 * 240 * 2]
m = DeviceManager(transport_factory=lambda: SimTransport(sim))
from PySide6.QtCore import QSettings  # noqa: E402

w = MainWindow(m, demo=True, settings=QSettings(os.path.join(out, "settings.ini"), QSettings.IniFormat))
w.resize(1280, 800)
w.show()


def pump(cond=lambda: True, secs=5.0):
    end = time.time() + secs
    while time.time() < end:
        app.processEvents()
        if cond() and not m.busy:
            break
        time.sleep(0.01)
    for _ in range(20):
        app.processEvents()


import tempfile  # noqa: E402

demo = os.path.join(tempfile.mkdtemp(), "Math class")  # neutral local folder (no personal files in the docs)
os.makedirs(os.path.join(demo, "Homework"))
for fname, data in (("Taylor notes.tns", build_notes([["Taylor"]])), ("newton.py", b"print(1)\n"),
                    ("bisection.tns", build_python("bisection.py", "print(1)\n"))):
    with open(os.path.join(demo, fname), "wb") as f:
        f.write(data)
w.files.local.set_dir(demo)
w.files.local.path_edit.setText("Documents › Math class")  # don't publish the real temp path/user name
m.connect_device()
pump(lambda: m.calc is not None)
pump(lambda: bool(w.files.remote.entries))
w.open_folder("/equations")
pump(lambda: w.files.remote.path == "/equations")
w.grab().save(os.path.join(out, "1-files.png"))
w.capture()
pump(lambda: w.screen.gallery.count() > 0)
w.grab().save(os.path.join(out, "2-screen.png"))
w.tabs.setCurrentWidget(w.keypad)
pump()
w.grab().save(os.path.join(out, "3-keypad.png"))
w.tabs.setCurrentWidget(w.create)
w.create.kind.setCurrentIndex(0)
pump()
w.grab().save(os.path.join(out, "4-create.png"))
w.tabs.setCurrentWidget(w.files)  # leave the keypad tab so its auto-refresh stops
pump()
w.open_remote_document("/equations/05 Taylor polynomial.tns")
pump(lambda: w.viewer.doc is not None)
print("viewer loaded:", w.viewer.doc is not None, flush=True)
w.grab().save(os.path.join(out, "5-viewer.png"))
print("saved to", os.path.abspath(out), flush=True)
w.close()
print("closed", flush=True)
