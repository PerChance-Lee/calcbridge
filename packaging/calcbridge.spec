# PyInstaller spec:  pyinstaller --noconfirm --clean packaging/calcbridge.spec
# Windows: dist/CalcBridge/ with CalcBridge.exe (app) and calcbridge-cli.exe (command line).
# macOS:   dist/CalcBridge.app
import os
import sys

from calcbridge import __version__

root = os.path.abspath(os.path.join(SPECPATH, ".."))
src = os.path.join(root, "src")
icon = os.path.join(src, "calcbridge", "assets", "icon.png")
datas = [(os.path.join(src, "calcbridge", "assets"), "calcbridge/assets")]
# Qt modules the app never imports (keeps the download small)
excludes = ["tkinter", "PySide6.QtNetwork", "PySide6.QtQml", "PySide6.QtQuick", "PySide6.QtOpenGL",
            "PySide6.QtSql", "PySide6.QtTest", "PySide6.QtDBus", "PySide6.QtConcurrent", "PySide6.QtXml"]

a = Analysis([os.path.join(SPECPATH, "gui_entry.py")], pathex=[src], datas=datas, excludes=excludes,
             hiddenimports=["calcbridge.tns.tixc_decode"])
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, [], exclude_binaries=True, name="CalcBridge", console=False, icon=icon, upx=False)

if sys.platform == "win32":
    c = Analysis([os.path.join(SPECPATH, "cli_entry.py")], pathex=[src], datas=datas, excludes=excludes)
    cpyz = PYZ(c.pure)
    cexe = EXE(cpyz, c.scripts, [], exclude_binaries=True, name="calcbridge-cli", console=True, icon=icon,
               upx=False)
    coll = COLLECT(exe, a.binaries, a.datas, cexe, c.binaries, c.datas, name="CalcBridge", upx=False)
else:
    coll = COLLECT(exe, a.binaries, a.datas, name="CalcBridge", upx=False)
    app = BUNDLE(coll, name="CalcBridge.app", icon=icon, bundle_identifier="io.github.perchance-lee.calcbridge",
                 info_plist={"CFBundleShortVersionString": __version__, "CFBundleVersion": __version__,
                             "NSHighResolutionCapable": True, "LSMinimumSystemVersion": "13.0"})
