"""The main window: wires panels to background device jobs."""
from __future__ import annotations

import datetime
import os
import time

from PySide6.QtCore import QSettings, Qt, QTimer, QUrl
from PySide6.QtGui import QAction, QDesktopServices, QKeySequence
from PySide6.QtWidgets import (QDockWidget, QFileDialog, QFormLayout, QGroupBox, QInputDialog, QLabel, QMainWindow,
                               QMessageBox, QPlainTextEdit, QProgressBar, QPushButton, QStyle, QTabWidget, QToolBar,
                               QVBoxLayout, QWidget)

from .. import APP_NAME, __version__
from ..protocol import keys as keymod
from ..protocol.errors import NoDeviceError
from ..protocol.services import basename, dirname, join
from ..tns import build_lua, build_python, build_text
from .create_panel import CreateTab, safe_identifier
from .files_panel import (CONVERTIBLE, OS_EXTS, FilesTab, ask_overwrite, human_size, next_folder_name,
                          unique_copy_name)
from .keypad_panel import KeypadTab
from .screen_panel import ScreenTab, screenshot_to_image
from .viewer_panel import ViewerTab

REPO_URL = "https://github.com/PerChance-Lee/calcbridge"


class DevicePanel(QWidget):
    def __init__(self):
        super().__init__()
        self.state = QLabel()
        self.state.setWordWrap(True)
        self.fields = {}
        box = QGroupBox("Handheld")
        form = QFormLayout(box)
        for key, label in (("name", "Name"), ("model", "Model"), ("os", "OS version"), ("boot", "Boot loaders"),
                           ("id", "Product ID"), ("battery", "Battery"), ("lcd", "Screen")):
            self.fields[key] = QLabel("—")
            self.fields[key].setTextInteractionFlags(Qt.TextSelectableByMouse)
            self.fields[key].setWordWrap(True)
            form.addRow(label, self.fields[key])
        self.storage = QProgressBar()
        self.storage.setFormat("%v of %m KB used")
        self.ram = QProgressBar()
        self.ram.setFormat("%v of %m KB used")
        form.addRow("Storage", self.storage)
        form.addRow("Memory", self.ram)
        self.connect_btn = QPushButton("Connect")
        self.refresh_btn = QPushButton("Refresh info")
        lay = QVBoxLayout(self)
        lay.addWidget(self.state)
        lay.addWidget(box)
        lay.addWidget(self.connect_btn)
        lay.addWidget(self.refresh_btn)
        lay.addStretch(1)
        self.show_info(None)

    def show_info(self, info, message=None):
        connected = info is not None
        self.refresh_btn.setEnabled(connected)
        self.connect_btn.setText("Disconnect" if connected else "Connect")
        if not connected:
            self.state.setText(message or "<b>No calculator connected.</b><br>Plug in your TI-Nspire CX II with "
                               "its USB cable and turn it on. CalcBridge connects automatically.")
            for f in self.fields.values():
                f.setText("—")
            self.storage.setMaximum(1)
            self.storage.setValue(0)
            self.ram.setMaximum(1)
            self.ram.setValue(0)
            return
        self.state.setText("<b style='color:#1a7f37'>● Connected</b>")
        self.fields["name"].setText(info.name)
        self.fields["model"].setText(info.model)
        self.fields["os"].setText(info.os_version)
        self.fields["boot"].setText(f"boot1 {info.boot1_version}, boot2 {info.boot2_version}")
        self.fields["id"].setText(info.electronic_id or "—")
        batt = {"unknown": "Not reported by this model", "ok": "OK", "low": "Low", "powered": "On USB power"}
        self.fields["battery"].setText(batt.get(info.battery, info.battery) + (" (charging)" if info.charging else ""))
        self.fields["lcd"].setText(f"{info.lcd_width}×{info.lcd_height}, {info.lcd_bpp}-bit colour")
        self.storage.setMaximum(max(1, info.flash_total // 1024))
        self.storage.setValue((info.flash_total - info.flash_free) // 1024)
        self.ram.setMaximum(max(1, info.ram_total // 1024))
        self.ram.setValue((info.ram_total - info.ram_free) // 1024)


class MainWindow(QMainWindow):
    def __init__(self, manager, demo=False, settings=None):
        super().__init__()
        self.m = manager
        self.demo = demo
        self.settings = settings or QSettings("CalcBridge", "CalcBridge")
        self.setWindowTitle(f"{APP_NAME}{' — simulator' if demo else ''}")
        self.resize(1180, 760)
        self.clipboard = None  # ("copy" | "cut", [remote paths])
        self._capture_in_flight = False
        self._keys_in_flight = 0

        start_dir = self.settings.value("local_dir", os.path.expanduser("~"))
        self.files = FilesTab(start_dir if os.path.isdir(start_dir) else os.path.expanduser("~"))
        self.screen = ScreenTab()
        self.keypad = KeypadTab()
        self.create = CreateTab()
        self.viewer = ViewerTab()
        self.log = QPlainTextEdit(readOnly=True)
        self.tabs = QTabWidget()
        for w, name in ((self.files, "Files"), (self.screen, "Screen"), (self.keypad, "Remote keypad"),
                        (self.create, "Create"), (self.viewer, "Viewer"), (self.log, "Log")):
            self.tabs.addTab(w, name)
        self.setCentralWidget(self.tabs)

        self.device = DevicePanel()
        dock = QDockWidget("Device", self)
        dock.setObjectName("device")
        dock.setWidget(self.device)
        dock.setFeatures(QDockWidget.DockWidgetMovable)
        self.addDockWidget(Qt.LeftDockWidgetArea, dock)

        self.status_label = QLabel()
        self.progress = QProgressBar()
        self.progress.setMaximumWidth(260)
        self.progress.hide()
        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.hide()
        self.cancel_btn.clicked.connect(self.m.cancel)
        self.statusBar().addWidget(self.status_label, 1)
        self.statusBar().addPermanentWidget(self.progress)
        self.statusBar().addPermanentWidget(self.cancel_btn)

        self._make_actions()
        self._wire()
        self.set_connected(None)
        geo = self.settings.value("geometry")
        if geo is not None:
            self.restoreGeometry(geo)

    # ------------------------------------------------------------ setup
    def _act(self, text, slot, shortcut=None, tip=None, device=True, icon=None):
        a = QAction(text, self)
        if icon is not None:
            a.setIcon(self.style().standardIcon(icon))
        a.triggered.connect(slot)
        if shortcut:
            a.setShortcut(QKeySequence(shortcut))
        if tip:
            a.setToolTip(tip)
        a.setProperty("needs_device", device)
        return a

    def _make_actions(self):
        S = QStyle
        self.a_refresh = self._act("Refresh", self.refresh, "F5", icon=S.SP_BrowserReload)
        self.a_upload = self._act("Upload files…", self.upload_dialog, "Ctrl+U", "Send files from this computer",
                                  icon=S.SP_ArrowUp)
        self.a_upload_dir = self._act("Upload folder…", self.upload_folder_dialog, icon=S.SP_DirIcon)
        self.a_download = self._act("Download", self.download_selected, "Ctrl+D",
                                    "Save the selected handheld items to the folder shown on the left",
                                    icon=S.SP_ArrowDown)
        self.a_newdir = self._act("New folder", self.new_folder, "Ctrl+Shift+N", icon=S.SP_FileDialogNewFolder)
        self.a_rename = self._act("Rename", self.rename_selected, "F2", icon=S.SP_FileDialogDetailedView)
        self.a_delete = self._act("Delete", self.delete_selected, "Del", icon=S.SP_TrashIcon)
        self.a_copy = self._act("Copy", lambda: self._clip("copy"), "Ctrl+C")
        self.a_cut = self._act("Cut", lambda: self._clip("cut"), "Ctrl+X")
        self.a_paste = self._act("Paste", self.paste, "Ctrl+V")
        self.a_dup = self._act("Duplicate", self.duplicate_selected, "Ctrl+Shift+D")
        self.a_capture = self._act("Capture screen", self.capture, "Ctrl+Shift+S", icon=S.SP_DesktopIcon)
        self.a_backup = self._act("Back up handheld…", self.backup, icon=S.SP_DriveHDIcon)
        self.a_restore = self._act("Restore backup…", self.restore, icon=S.SP_DialogOpenButton)
        self.a_os = self._act("Install OS…", self.install_os, icon=S.SP_ComputerIcon)
        self.a_about = self._act("About", self.about, device=False, icon=S.SP_MessageBoxInformation)
        tb = QToolBar("Main")
        tb.setObjectName("main")
        tb.setMovable(False)
        tb.setToolButtonStyle(Qt.ToolButtonTextUnderIcon)
        for a in (self.a_refresh, self.a_upload, self.a_download, self.a_newdir, self.a_rename, self.a_delete,
                  self.a_capture, self.a_backup, self.a_restore, self.a_os):
            tb.addAction(a)
        tb.addSeparator()
        tb.addAction(self.a_about)
        self.addToolBar(tb)
        tree = self.files.remote.tree
        tree.setContextMenuPolicy(Qt.ActionsContextMenu)
        for a in (self.a_download, self.a_rename, self.a_delete, self.a_copy, self.a_cut, self.a_paste, self.a_dup,
                  self.a_newdir, self.a_refresh):
            tree.addAction(a)
        open_act = self._act("Open in viewer", self.open_selected_in_viewer)
        tree.insertAction(self.a_download, open_act)
        props = self._act("Properties", self.properties)
        tree.addAction(props)
        self.device_actions = [a for a in self.findChildren(QAction) if a.property("needs_device")]
        menu = self.menuBar()
        fm = menu.addMenu("&File")
        for a in (self.a_upload, self.a_upload_dir, self.a_download, self.a_backup, self.a_restore):
            fm.addAction(a)
        fm.addSeparator()
        fm.addAction(self._act("Quit", self.close, "Ctrl+Q", device=False))
        em = menu.addMenu("&Edit")
        for a in (self.a_copy, self.a_cut, self.a_paste, self.a_dup, self.a_rename, self.a_delete, self.a_newdir):
            em.addAction(a)
        hm = menu.addMenu("&Handheld")
        for a in (self.a_refresh, self.a_capture, self.a_os):
            hm.addAction(a)
        hm.addAction(self._act("Connect / disconnect", self.toggle_connection, device=False))
        help_menu = menu.addMenu("&Help")
        help_menu.addAction(self._act("Project page", lambda: QDesktopServices.openUrl(QUrl(REPO_URL)), device=False))
        help_menu.addAction(self._act("TI OS downloads", lambda: QDesktopServices.openUrl(
            QUrl("https://education.ti.com/en/products/calculators/graphing-calculators/ti-nspire-cx-ii")), device=False))
        help_menu.addAction(self.a_about)

    def _wire(self):
        m = self.m
        m.connected.connect(self.set_connected)
        m.disconnected.connect(lambda reason: self.set_connected(None, reason))
        m.message.connect(self.say)
        m.busy_changed.connect(self._busy)
        m.progress.connect(self._progress)
        self.device.connect_btn.clicked.connect(self.toggle_connection)
        self.device.refresh_btn.clicked.connect(self.refresh_info)
        self.files.remote.open_folder = self.open_folder
        self.files.remote.open_document.connect(self.open_remote_document)
        self.files.local.open_document.connect(self.open_local_document)
        self.files.remote.tree.files_dropped.connect(self.upload)
        self.files.remote.tree.remote_dropped.connect(self.move_into)
        self.files.local.tree.remote_dropped.connect(self.download_to)
        self.screen.capture_requested.connect(self.capture)
        self.keypad.key_requested.connect(self.press_key)
        self.keypad.text_requested.connect(self.type_text)
        self.keypad.refresh_requested.connect(self._keypad_refresh)
        self.create.send_requested.connect(self.send_built)
        self.create.status.connect(self.say)
        self.viewer.edit_python.connect(self._edit_python)
        self.viewer.send_requested.connect(lambda blob, name: self.send_built(blob, name, False))

    # ---------------------------------------------------------- status
    def say(self, text):
        self.status_label.setText(text)
        self.log.appendPlainText(f"{datetime.datetime.now():%H:%M:%S}  {text}")

    def _busy(self, busy):
        if not busy:
            self.progress.hide()
            self.cancel_btn.hide()

    def _progress(self, done, total, text):
        if total <= 0:
            return
        self.progress.show()
        self.cancel_btn.show()
        self.progress.setMaximum(total)
        self.progress.setValue(min(done, total))
        if text:
            self.status_label.setText(text)

    def error(self, msg, title="Something went wrong"):
        self.say(f"Error: {msg}")
        QMessageBox.warning(self, title, msg)

    # ------------------------------------------------------- connection
    def toggle_connection(self):
        if self.m.calc is not None:
            self.m.disconnect_device()
        else:
            self.m.auto_connect = True
            self.m.connect_device()

    def set_connected(self, info, reason=None):
        for a in self.device_actions:
            a.setEnabled(info is not None)
        if info is None:
            self.files.remote.clear()
            self.files.remote.crumb.setText("Not connected")
            hint = None
            if reason and ("isn't answering" in reason or "stopped answering" in reason):
                hint = ("<b>The calculator isn't answering.</b><br>Unplug it, plug it back in, and click Connect.")
            elif reason and "Can't open" in reason:
                hint = f"<b>{reason}</b>"
            self.device.show_info(None, hint)
            return
        self.device.show_info(info)
        self.open_folder("/")

    def refresh_info(self):
        self.m.submit(lambda c: c.info(), on_done=self.device.show_info, on_error=self.error)

    # ------------------------------------------------------------ files
    def open_folder(self, path):
        def done(entries, path=path):
            self.files.remote.set_entries(path, entries)
            self.say(f"{len(entries)} items in {path}")
        self.m.submit(lambda c: c.list_dir(path), on_done=done, on_error=self.error)

    def refresh(self):
        self.open_folder(self.files.remote.path)
        self.refresh_info()

    def _plan_uploads(self, local_paths, folder, existing):
        """Decide what to send; asks about conversions and overwrites. -> list of jobs or None."""
        plan, skipped, convert_ok = [], [], None
        policy = None
        for path in local_paths:
            ext = os.path.splitext(path)[1].lower()
            name = os.path.basename(os.path.normpath(path))
            data = None
            if os.path.isdir(path) or ext == ".tns":
                pass
            elif ext in OS_EXTS:
                if QMessageBox.question(self, "Install OS?", f"{name} is an OS file. Install it on the handheld "
                                        "now?") == QMessageBox.Yes:
                    self.install_os(path)
                continue
            elif ext in CONVERTIBLE:
                if convert_ok is None:
                    convert_ok = QMessageBox.question(
                        self, "Convert files?", "The handheld only accepts TI-Nspire documents (.tns). Convert "
                        f"{name}{' and similar files' if len(local_paths) > 1 else ''} into documents "
                        "(Python program / Lua script / Notes page) and send them?") == QMessageBox.Yes
                if not convert_ok:
                    skipped.append(name)
                    continue
                with open(path, encoding="utf-8", errors="replace") as f:
                    src = f.read()
                stem = os.path.splitext(name)[0]
                try:
                    data = (build_python(safe_identifier(stem) + ".py", src) if ext == ".py" else
                            build_lua(src) if ext == ".lua" else build_text(src, title=stem))
                except ValueError as e:
                    self.error(f"Couldn't convert {name}: {e}")
                    continue
                name = stem + ".tns"
            else:
                skipped.append(name)
                continue
            overwrite = False
            if name in existing:
                if policy != "all":
                    policy = ask_overwrite(self, name)
                if policy == "cancel":
                    return None
                if policy == "no":
                    continue
                overwrite = True
            plan.append((path, name, data, overwrite))
        if skipped:
            QMessageBox.information(self, "Some files were skipped",
                                    "The handheld only accepts TI-Nspire documents (.tns) and folders. "
                                    "Skipped:\n" + "\n".join(skipped))
        return plan

    def upload(self, local_paths, folder=None):
        folder = folder or self.files.remote.path

        def with_listing(entries):
            plan = self._plan_uploads(local_paths, folder, {e.name for e in entries})
            if not plan:
                return

            def run(c, progress):
                for n, (path, name, data, overwrite) in enumerate(plan, 1):
                    progress(n - 1, len(plan), f"Sending {name} ({n} of {len(plan)})…")
                    if data is not None:
                        c.write_file(join(folder, name), data, overwrite=overwrite)
                    else:
                        c.upload(path, folder, overwrite=overwrite, name=name)
                    c._check_cancel()
                progress(len(plan), len(plan), "Done")
                return len(plan)
            self.m.submit(run, on_done=lambda n: (self.say(f"Sent {n} item(s) to {folder}"), self.refresh()),
                          on_error=lambda msg: (self.error(msg), self.refresh()), with_progress=True,
                          label="Sending…")
        self.m.submit(lambda c: c.list_dir(folder), on_done=with_listing, on_error=self.error)

    def upload_dialog(self):
        paths, _ = QFileDialog.getOpenFileNames(self, "Send files to the handheld", self.files.local.current_dir,
                                                "TI-Nspire documents (*.tns);;Convertible (*.py *.lua *.txt *.md);;"
                                                "All files (*)")
        if paths:
            self.upload(paths)

    def upload_folder_dialog(self):
        path = QFileDialog.getExistingDirectory(self, "Send a folder to the handheld", self.files.local.current_dir)
        if path:
            self.upload([path])

    def download_selected(self):
        paths = self.files.remote.selected_paths()
        if paths:
            self.download_to(paths, self.files.local.current_dir)

    def download_to(self, remote_paths, local_folder):
        targets = [(p, os.path.join(local_folder, basename(p))) for p in remote_paths]
        clash = [t for _, t in targets if os.path.exists(t)]
        if clash and QMessageBox.question(self, "Replace files?", "These already exist on this computer and will be "
                                          "replaced:\n" + "\n".join(os.path.basename(c) for c in clash)) != QMessageBox.Yes:
            return

        def run(c, progress):
            total = 0
            for n, (p, t) in enumerate(targets, 1):
                progress(n - 1, len(targets), f"Receiving {basename(p)} ({n} of {len(targets)})…")
                total += c.download(p, t)
                c._check_cancel()
            progress(len(targets), len(targets), "Done")
            return total
        self.m.submit(run, on_done=lambda n: self.say(f"Received {human_size(n)} into {local_folder}"),
                      on_error=self.error, with_progress=True)

    def new_folder(self):
        name, ok = QInputDialog.getText(self, "New folder", "Folder name:",
                                        text=next_folder_name(self.files.remote.names()))
        if ok and name.strip():
            path = join(self.files.remote.path, name.strip())
            self.m.submit(lambda c: c.mkdir(path), on_done=lambda _: self.refresh(), on_error=self.error)

    def rename_selected(self):
        sel = self.files.remote.selected_entries()
        if len(sel) != 1:
            return
        e = sel[0]
        name, ok = QInputDialog.getText(self, "Rename", "New name:", text=e.name)
        name = name.strip()
        if not ok or not name or name == e.name:
            return
        if e.name.lower().endswith(".tns") and not name.lower().endswith(".tns"):
            name += ".tns"  # the handheld only lists .tns documents
        self.m.submit(lambda c: c.rename(e.path, name), on_done=lambda _: self.refresh(), on_error=self.error)

    def delete_selected(self):
        sel = self.files.remote.selected_entries()
        if not sel:
            return
        names = "\n".join(("📁 " if e.is_dir else "") + e.name for e in sel[:15]) + ("\n…" if len(sel) > 15 else "")
        if QMessageBox.question(self, "Delete from handheld?", "Permanently delete these from the handheld?\n\n"
                                + names) != QMessageBox.Yes:
            return
        paths = [e.path for e in sel]

        def run(c, progress):
            for n, p in enumerate(paths, 1):
                progress(n - 1, len(paths), f"Deleting {basename(p)}…")
                c.delete(p)
            return len(paths)
        self.m.submit(run, on_done=lambda n: (self.say(f"Deleted {n} item(s)"), self.refresh()),
                      on_error=lambda msg: (self.error(msg), self.refresh()), with_progress=True)

    def _clip(self, mode):
        paths = self.files.remote.selected_paths()
        if paths:
            self.clipboard = (mode, paths)
            self.say(f"{'Copied' if mode == 'copy' else 'Cut'} {len(paths)} item(s)")

    def paste(self):
        if not self.clipboard:
            return
        mode, paths = self.clipboard
        folder = self.files.remote.path
        taken = set(self.files.remote.names())
        ops = []
        for p in paths:
            name = basename(p)
            if mode == "cut":
                if dirname(p) == folder:
                    continue
                if name in taken:
                    self.error(f"{name} already exists here")
                    return
                ops.append(("move", p, join(folder, name)))
            else:
                if name in taken:
                    name = unique_copy_name(name, taken)
                taken.add(name)
                ops.append(("copy", p, join(folder, name)))

        def run(c, progress):
            for n, (op, src, dst) in enumerate(ops, 1):
                progress(n - 1, len(ops), f"{'Moving' if op == 'move' else 'Copying'} {basename(src)}…")
                (c.move if op == "move" else c.copy)(src, dst)
            return len(ops)
        if mode == "cut":
            self.clipboard = None
        self.m.submit(run, on_done=lambda n: self.refresh(), on_error=lambda msg: (self.error(msg), self.refresh()),
                      with_progress=True)

    def duplicate_selected(self):
        sel = self.files.remote.selected_paths()
        if sel:
            self.clipboard = ("copy", sel)
            self.paste()

    def move_into(self, paths, folder):
        def run(c, progress):
            for n, p in enumerate(paths, 1):
                progress(n - 1, len(paths), f"Moving {basename(p)}…")
                c.move(p, join(folder, basename(p)))
            return len(paths)
        self.m.submit(run, on_done=lambda n: self.refresh(), on_error=lambda msg: (self.error(msg), self.refresh()),
                      with_progress=True)

    def properties(self):
        sel = self.files.remote.selected_entries()
        if len(sel) == 1:
            e = sel[0]
            QMessageBox.information(self, e.name, f"Location: {e.path}\nType: "
                                    f"{'Folder' if e.is_dir else 'TI-Nspire document' if e.is_document else 'File'}"
                                    + ("" if e.is_dir else f"\nSize: {human_size(e.size)}"))

    # --------------------------------------------------------- documents
    def open_selected_in_viewer(self):
        docs = [e for e in self.files.remote.selected_entries() if e.is_document]
        if docs:
            self.open_remote_document(docs[0].path)

    def open_remote_document(self, path):
        def done(blob):
            self.viewer.load(blob, basename(path), "handheld")
            self.tabs.setCurrentWidget(self.viewer)
        self.m.submit(lambda c: c.read_file(path), on_done=done, on_error=self.error)

    def open_local_document(self, path):
        with open(path, "rb") as f:
            self.viewer.load(f.read(), os.path.basename(path), "computer")
        self.tabs.setCurrentWidget(self.viewer)

    def _edit_python(self, name, code):
        self.create.load_source(name, code)
        self.tabs.setCurrentWidget(self.create)

    def send_built(self, data, fname, as_module):
        if self.m.calc is None:
            self.error("Connect a calculator first.")
            return
        folder = "/PyLib" if as_module else self.files.remote.path
        path = join(folder, fname)

        def check(st):
            overwrite = False
            if st is not None:
                if QMessageBox.question(self, "Replace?", f"{fname} already exists in {folder}. Replace it?") \
                        != QMessageBox.Yes:
                    return
                overwrite = True

            def ok(_):
                self.say(f"Sent {fname} to {folder}"
                         + (" — on the handheld, open the home screen and choose Refresh Libraries, or just "
                            "import it from another program." if as_module else ""))
                self.refresh()
            self.m.submit(lambda c: c.write_file(path, data, overwrite=overwrite), on_done=ok, on_error=self.error)
        self.m.submit(lambda c: c.stat(path), on_done=check, on_error=self.error)

    # ------------------------------------------------------ screen/keys
    def capture(self):
        if self._capture_in_flight:
            return
        self._capture_in_flight = True

        def done(shot):
            self._capture_in_flight = False
            self.screen.add_capture(screenshot_to_image(shot))
            if not self.screen.live.isChecked():
                self.say("Screen captured and copied to the clipboard")
                self.tabs.setCurrentWidget(self.screen)

        def fail(msg):
            self._capture_in_flight = False
            self.screen.live.setChecked(False)
            self.error(msg)
        self.m.submit(lambda c: c.screenshot(), on_done=done, on_error=fail)

    def _keypad_refresh(self):
        if self.m.calc is None or self._capture_in_flight or self._keys_in_flight:
            return
        self._capture_in_flight = True

        def done(shot):
            self._capture_in_flight = False
            self.keypad.set_screen(screenshot_to_image(shot))

        def fail(msg):
            self._capture_in_flight = False
            self.say(f"Error: {msg}")
        self.m.submit(lambda c: c.screenshot(), on_done=done, on_error=fail)

    def press_key(self, name):
        if self.m.calc is None:
            return
        self._keys_in_flight += 1
        code = keymod.key_code(name)

        def done(_):
            self._keys_in_flight -= 1
            if not self._keys_in_flight:
                QTimer.singleShot(250, self._keypad_refresh)

        def fail(msg):
            self._keys_in_flight -= 1
            self.say(f"Error: {msg}")
        self.m.submit(lambda c: c._call(keymod.press, code), on_done=done, on_error=fail)

    def type_text(self, text):
        try:
            codes = keymod.text_codes(text)
        except KeyError as e:
            self.error(str(e))
            return

        def run(c):
            for code in codes:
                c._call(keymod.press, code)
                time.sleep(0.08)
        self.m.submit(run, on_done=lambda _: QTimer.singleShot(250, self._keypad_refresh), on_error=self.error)

    # ----------------------------------------------- backup/restore/OS
    def backup(self):
        base = QFileDialog.getExistingDirectory(self, "Choose where to save the backup", self.files.local.current_dir)
        if not base:
            return
        name = (self.m.info.name if self.m.info else "handheld").replace("/", "-")
        target = os.path.join(base, f"{name} backup {datetime.datetime.now():%Y-%m-%d %H%M}")

        def run(c, progress):
            return c.backup(target, progress=lambda path, n, total: progress(n, total, f"Backing up {path}"))
        self.m.submit(run, on_done=lambda n: QMessageBox.information(self, "Backup complete",
                                                                     f"Saved {n} files to\n{target}"),
                      on_error=self.error, with_progress=True)

    def restore(self):
        src = QFileDialog.getExistingDirectory(self, "Choose a backup folder to restore", self.files.local.current_dir)
        if not src:
            return
        box = QMessageBox(self)
        box.setWindowTitle("Restore backup")
        box.setText(f"Send everything in\n{src}\nto the handheld's My Documents?")
        skip = box.addButton("Keep existing files", QMessageBox.AcceptRole)
        over = box.addButton("Replace existing files", QMessageBox.DestructiveRole)
        box.addButton(QMessageBox.Cancel)
        box.exec()
        if box.clickedButton() not in (skip, over):
            return
        overwrite = box.clickedButton() is over

        def run(c, progress):
            return c.restore(src, "/", overwrite=overwrite,
                             progress=lambda path, n, total: progress(n, total, f"Restoring {path}"))
        self.m.submit(run, on_done=lambda n: (self.say(f"Restored {n} files"), self.refresh()),
                      on_error=self.error, with_progress=True)

    def install_os(self, path=None):
        info = self.m.info
        if info is None:
            return
        want = info.os_extension or ".tco2"
        if not path:
            path, _ = QFileDialog.getOpenFileName(self, "Choose an OS file", self.files.local.current_dir,
                                                  f"TI-Nspire OS for this model (*{want});;All OS files "
                                                  "(*.tco2 *.tcc2 *.tct2)")
            if not path:
                return
        ext = os.path.splitext(path)[1].lower()
        if ext != want.lower():
            self.error(f"This handheld needs a {want} OS file, not {ext}. (CX II = .tco2, CX II CAS = .tcc2, "
                       "CX II-T = .tct2.)", "Wrong OS file")
            return
        with open(path, "rb") as f:
            image = f.read()
        if QMessageBox.warning(
                self, "Install OS", f"Install {os.path.basename(path)} on {info.name}?\n\n• Unsaved work on the "
                "handheld will be lost.\n• Don't unplug the calculator until it restarts.\n• The handheld checks "
                "the file itself and refuses anything that isn't a genuine TI OS.",
                QMessageBox.Yes | QMessageBox.Cancel) != QMessageBox.Yes:
            return
        state = {"installed": False}

        def run(c, progress):
            def cb(stage, done, total):
                if stage == "install" and done >= 100:
                    state["installed"] = True
                progress(done, total, "Sending OS…" if stage == "upload" else f"Installing… {done}%")
            try:
                c.install_os(image, progress=cb)
            except NoDeviceError:
                if not state["installed"]:
                    raise  # unplugged mid-transfer
            return True

        def done(_):
            QMessageBox.information(self, "OS installed", "The handheld is installing the OS and will restart. "
                                    "CalcBridge reconnects when it's back.")
            self.m.mark_lost("Handheld restarting after the OS install")
        self.m.submit(run, on_done=done, on_error=self.error, with_progress=True)

    # ------------------------------------------------------------- misc
    def about(self):
        QMessageBox.about(self, f"About {APP_NAME}",
                          f"<h3>{APP_NAME} {__version__}</h3>"
                          "<p>An open-source desktop link for the TI-Nspire CX II: file transfer, screen capture, "
                          "backups, OS installation, a remote keypad, and document tools.</p>"
                          f"<p><a href='{REPO_URL}'>{REPO_URL}</a></p>"
                          "<p>Licensed under the GNU GPL v3 or later. Protocol knowledge from libnspire, tilibs, "
                          "Luna, TnsTools and hackspire.org.</p>"
                          "<p><small>Not affiliated with or endorsed by Texas Instruments. TI-Nspire is a "
                          "trademark of Texas Instruments.</small></p>")

    def closeEvent(self, e):
        self.settings.setValue("geometry", self.saveGeometry())
        self.settings.setValue("local_dir", self.files.local.current_dir)
        self.screen.timer.stop()
        self.keypad.auto.stop()
        self.m.shutdown()
        super().closeEvent(e)
