"""Runs calculator jobs on one background thread and reports back on the GUI thread."""
from __future__ import annotations

import logging
import traceback
from concurrent.futures import ThreadPoolExecutor

from PySide6.QtCore import QObject, QTimer, Signal, Slot

from ..device import Calculator, Cancelled
from ..protocol.errors import NoDeviceError
from ..protocol.transport import list_usb_devices

log = logging.getLogger(__name__)


class Job(QObject):
    """Lives on the GUI thread; the worker emits its private signals, so callbacks always run on the
    GUI thread (queued connections to bound slots)."""

    _done = Signal(object)
    _failed = Signal(str, object)
    _progress = Signal(int, int, str)

    def __init__(self, manager, on_done, on_error, on_progress):
        super().__init__(manager)
        self.manager, self.on_done, self.on_error, self.on_progress = manager, on_done, on_error, on_progress
        self._done.connect(self._handle_done)
        self._failed.connect(self._handle_failed)
        self._progress.connect(self._handle_progress)

    @Slot(object)
    def _handle_done(self, result):
        try:
            if self.on_done:
                self.on_done(result)
        finally:
            self.manager._job_finished(self)

    @Slot(str, object)
    def _handle_failed(self, msg, exc):
        try:
            self.manager._on_failed(msg, exc, self.on_error)
        finally:
            self.manager._job_finished(self)

    @Slot(int, int, str)
    def _handle_progress(self, done, total, text):
        if self.on_progress:
            self.on_progress(done, total, text)


class DeviceManager(QObject):
    """Owns the connection. Every job runs in submission order on a single worker thread."""

    connected = Signal(object)        # DeviceInfo
    disconnected = Signal(str)        # reason
    busy_changed = Signal(bool)
    message = Signal(str)             # status/log lines
    device_present = Signal(bool)     # USB presence (hotplug polling)
    progress = Signal(int, int, str)  # any job's progress, for the status bar

    def __init__(self, transport_factory=None, parent=None):
        super().__init__(parent)
        self._pool = ThreadPoolExecutor(max_workers=1, thread_name_prefix="calc")
        self.calc: Calculator | None = None
        self.info = None
        self._transport_factory = transport_factory  # tests/demo pass a simulator transport
        self._jobs: set[Job] = set()
        self._present = False
        self._poll = QTimer(self)
        self._poll.timeout.connect(self._poll_usb)
        self.auto_connect = True
        self.connecting = False

    # ------------------------------------------------------------- jobs
    def submit(self, fn, *args, on_done=None, on_error=None, on_progress=None, label="", needs_device=True,
               with_progress=False):
        """Run fn(calc, *args[, progress=cb]) — or fn(*args) when needs_device=False — in the background.
        Callbacks run on the GUI thread."""
        def relay(done, total, text):
            self.progress.emit(done, total, text)
            if on_progress:
                on_progress(done, total, text)

        job = Job(self, on_done, on_error, relay)
        self._jobs.add(job)
        if len(self._jobs) == 1:
            self.busy_changed.emit(True)
        calc = self.calc

        def run():
            try:
                if needs_device:
                    if calc is None:
                        raise NoDeviceError("No calculator is connected")
                    if calc is not self.calc:  # queued before a disconnect: don't touch the old link
                        raise Cancelled("Disconnected before this ran")
                    calc.cancel_event.clear()  # a Cancel meant for an earlier job doesn't carry over

                    def progress(done, total, text=label):
                        job._progress.emit(int(done), int(total), text)
                    result = fn(calc, *args, progress=progress) if (with_progress or on_progress) else fn(calc, *args)
                else:
                    result = fn(*args)
                job._done.emit(result)
            except Exception as e:  # reported on the GUI thread
                log.debug("job failed: %s", traceback.format_exc())
                job._failed.emit(str(e) or type(e).__name__, e)

        self._pool.submit(run)
        return job

    def _job_finished(self, job):
        self._jobs.discard(job)
        job.deleteLater()
        if not self._jobs:
            self.busy_changed.emit(False)

    def _on_failed(self, msg, exc, on_error):
        if isinstance(exc, Cancelled):
            self.message.emit(msg)  # the user asked for it; no error dialog
            return
        if isinstance(exc, NoDeviceError) and self.calc is not None:
            self._drop(f"Calculator disconnected: {msg}")
        if on_error:
            on_error(msg)
        else:
            self.message.emit(f"Error: {msg}")

    @property
    def busy(self):
        return bool(self._jobs)

    def cancel(self):
        if self.calc is not None:
            self.calc.cancel_event.set()

    # ------------------------------------------------------- connection
    def start_polling(self, interval_ms=2000):
        self._poll.start(interval_ms)
        QTimer.singleShot(0, self._poll_usb)

    @Slot()
    def _poll_usb(self):
        if self._transport_factory is not None:
            present = True
        elif self.busy:
            return  # a job will notice an unplug itself
        else:
            try:
                present = bool(list_usb_devices())
            except Exception as e:
                log.debug("USB poll failed: %s", e)
                present = False
        if present != self._present:
            self._present = present
            self.device_present.emit(present)
            if present:
                self.auto_connect = True
            elif self.calc is not None:
                self._drop("Calculator unplugged")
        if present and self.calc is None and self.auto_connect and not self.connecting:
            self.connect_device()

    def connect_device(self):
        if self.calc is not None or self.connecting:
            return
        self.connecting = True
        self.message.emit("Connecting…")

        def do_connect(progress=None):
            transport = self._transport_factory() if self._transport_factory else None
            calc = Calculator(transport=transport, progress=progress)
            try:
                info = calc.info()
            except Exception:
                calc.close()
                raise
            return calc, info

        def ok(result):
            self.connecting = False
            self.calc, self.info = result
            self.message.emit(f"Connected to {self.info.name} (OS {self.info.os_version})")
            self.connected.emit(self.info)

        def fail(msg):
            self.connecting = False
            self.auto_connect = False  # don't hammer a calculator that won't talk; the user can retry
            self.message.emit(f"Couldn't connect: {msg}")
            self.disconnected.emit(msg)

        def progress(done, total, _text):
            self.message.emit(f"Syncing with the calculator… ({done}/{total})")

        job = Job(self, ok, fail, progress)
        self._jobs.add(job)
        if len(self._jobs) == 1:
            self.busy_changed.emit(True)

        def run():
            try:
                job._done.emit(do_connect(lambda d, t: job._progress.emit(d, t, "")))
            except Exception as e:
                job._failed.emit(str(e) or type(e).__name__, e)

        self._pool.submit(run)

    def disconnect_device(self):
        self.auto_connect = False
        self._drop("Disconnected")

    def mark_lost(self, reason):
        """The handheld is restarting (e.g. after an OS install): reconnect only once it has dropped
        off USB and come back, not while it's still enumerated but busy."""
        self._present = True
        self.auto_connect = False
        self._drop(reason)

    def _drop(self, reason):
        calc, self.calc, self.info = self.calc, None, None
        if calc is not None:
            self._pool.submit(calc.close)
        self.message.emit(reason)
        self.disconnected.emit(reason)

    def shutdown(self):
        """Stop queued jobs, ask the running one to stop at its next item, then close the link (which
        also saves the sequence number for next time)."""
        self._poll.stop()
        calc, self.calc = self.calc, None
        if calc is not None:
            calc.cancel_event.set()
        self._pool.shutdown(wait=True, cancel_futures=True)
        if calc is not None:
            try:
                calc.close()
            except Exception as e:
                log.debug("close failed: %s", e)
