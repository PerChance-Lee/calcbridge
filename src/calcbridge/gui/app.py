"""Entry point for the desktop app.

    calcbridge             start the app (connects to a CX II automatically)
    calcbridge --sim       start with a simulated calculator (no hardware needed)
    calcbridge --self-test check that USB and Qt load (used by CI on the packaged app), then exit
"""
from __future__ import annotations

import argparse
import logging
import os
import sys

from .. import APP_NAME, __version__


def asset(name: str) -> str:
    bundle = getattr(sys, "_MEIPASS", None)  # inside a PyInstaller build
    if bundle:
        return os.path.join(bundle, "calcbridge", "assets", name)
    return os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", name)


def self_test() -> int:
    """Exit 0 if libusb and the Qt platform both load. Prints what it found."""
    ok = True
    try:
        from ..protocol.transport import _backend, list_usb_devices
        be = _backend()
        print("libusb backend:", "ok" if be is not None else "MISSING")
        ok &= be is not None
        if be is not None:
            print("calculators connected:", len(list_usb_devices()))
    except Exception as e:
        print("libusb check failed:", e)
        ok = False
    try:
        from PySide6.QtWidgets import QApplication
        from .main_window import MainWindow
        from .worker import DeviceManager
        from ..sim import SimCalculator, SimTransport
        import tempfile

        from PySide6.QtCore import QSettings
        app = QApplication.instance() or QApplication(sys.argv[:1])
        sim = SimCalculator(idle_drop_s=0)
        m = DeviceManager(transport_factory=lambda: SimTransport(sim))
        settings = QSettings(os.path.join(tempfile.gettempdir(), "calcbridge-selftest.ini"), QSettings.IniFormat)
        w = MainWindow(m, demo=True, settings=settings)
        w.show()
        app.processEvents()
        print("Qt window:", "ok")
        w.close()
    except Exception as e:
        print("Qt check failed:", e)
        ok = False
    print("self-test", "passed" if ok else "FAILED")
    return 0 if ok else 1


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="calcbridge", description=f"{APP_NAME} {__version__}")
    p.add_argument("--sim", action="store_true", help="use a simulated calculator")
    p.add_argument("--self-test", action="store_true", help="check that USB and Qt work, then exit")
    p.add_argument("--debug", action="store_true", help="verbose logging")
    args, qt_args = p.parse_known_args(argv)
    logging.basicConfig(level=logging.DEBUG if args.debug else logging.INFO,
                        format="%(asctime)s %(name)s %(levelname)s %(message)s")
    if args.self_test:
        return self_test()

    if sys.platform == "win32":
        try:  # own taskbar group and icon instead of python.exe's
            import ctypes
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("CalcBridge.CalcBridge")
        except Exception:
            pass

    from PySide6.QtGui import QIcon
    from PySide6.QtWidgets import QApplication

    from .main_window import MainWindow
    from .worker import DeviceManager

    app = QApplication([sys.argv[0], *qt_args])
    app.setApplicationName(APP_NAME)
    app.setOrganizationName("CalcBridge")
    app.setApplicationVersion(__version__)
    icon = asset("icon.png")
    if os.path.exists(icon):
        app.setWindowIcon(QIcon(icon))

    factory = None
    if args.sim:
        from ..sim import SimCalculator, SimTransport
        sim = SimCalculator(idle_drop_s=0)
        factory = lambda: SimTransport(sim)  # noqa: E731
    manager = DeviceManager(transport_factory=factory)
    window = MainWindow(manager, demo=args.sim)
    window.show()
    manager.start_polling()
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
