"""Screen capture: single captures into a gallery, live view, zoom, border, save/copy/drag out."""
from __future__ import annotations

import datetime
import os
import tempfile

from PySide6.QtCore import QMimeData, QSize, Qt, QTimer, QUrl, Signal
from PySide6.QtGui import QColor, QDrag, QGuiApplication, QIcon, QImage, QImageWriter, QPainter, QPixmap
from PySide6.QtWidgets import (QCheckBox, QComboBox, QFileDialog, QHBoxLayout, QLabel, QListWidget,
                               QListWidgetItem, QMessageBox, QPushButton, QScrollArea, QSpinBox, QVBoxLayout,
                               QWidget)


def screenshot_to_image(shot) -> QImage:
    if shot.bpp == 16:
        # Little-endian RGB565, which is exactly Qt's RGB16 (checked against a real CX II: TI's blue
        # title bars and buttons come out blue).
        img = QImage(shot.pixels, shot.width, shot.height, shot.width * 2, QImage.Format_RGB16)
        return img.copy()
    grey = bytearray()
    for b in shot.pixels:
        grey += bytes(((b >> 4) * 17, (b & 15) * 17))
    img = QImage(bytes(grey), shot.width, shot.height, shot.width, QImage.Format_Grayscale8)
    return img.copy()


def with_border(img: QImage) -> QImage:
    """TI's capture border: 3 px all round, 326x246 for a CX II screen."""
    out = QImage(img.width() + 6, img.height() + 6, QImage.Format_RGB32)
    out.fill(QColor(60, 60, 60))
    p = QPainter(out)
    p.drawImage(3, 3, img)
    p.end()
    return out


class GalleryList(QListWidget):
    def startDrag(self, actions):
        item = self.currentItem()
        if item is None:
            return
        img = item.data(Qt.UserRole)
        path = os.path.join(tempfile.gettempdir(), item.text().replace(":", "-") + ".png")
        img.save(path, "PNG")
        mime = QMimeData()
        mime.setImageData(img)
        mime.setUrls([QUrl.fromLocalFile(path)])
        drag = QDrag(self)
        drag.setMimeData(mime)
        drag.setPixmap(QPixmap.fromImage(img).scaled(160, 120, Qt.KeepAspectRatio))
        drag.exec(Qt.CopyAction)


class ScreenTab(QWidget):
    capture_requested = Signal()

    def __init__(self):
        super().__init__()
        self.counter = 0
        self.view = QLabel(alignment=Qt.AlignCenter)
        self.view.setMinimumSize(326, 246)
        self.view.setStyleSheet("background: #202020;")
        scroll = QScrollArea()
        scroll.setWidget(self.view)
        scroll.setWidgetResizable(True)
        self.capture_btn = QPushButton("Capture")
        self.capture_btn.setToolTip("Capture the handheld's screen (Ctrl+Shift+S)")
        self.capture_btn.clicked.connect(self.capture_requested)
        self.live = QCheckBox("Live view")
        self.interval = QSpinBox(suffix=" s", minimum=1, maximum=30, value=1)
        self.zoom = QComboBox()
        self.zoom.addItems(["100%", "200%", "300%"])
        self.zoom.setCurrentIndex(1)
        self.zoom.currentIndexChanged.connect(self._show_current)
        self.border = QCheckBox("Border")
        self.border.toggled.connect(self._show_current)
        copy_btn = QPushButton("Copy")
        copy_btn.clicked.connect(self.copy_current)
        save_btn = QPushButton("Save…")
        save_btn.clicked.connect(self.save_current)
        save_all = QPushButton("Save all…")
        save_all.clicked.connect(self.save_all)
        remove_btn = QPushButton("Remove")
        remove_btn.clicked.connect(self.remove_current)
        bar = QHBoxLayout()
        for w in (self.capture_btn, self.live, self.interval, QLabel("Zoom"), self.zoom, self.border, copy_btn,
                  save_btn, save_all, remove_btn):
            bar.addWidget(w)
        bar.addStretch(1)
        self.gallery = GalleryList()
        self.gallery.setViewMode(QListWidget.IconMode)
        self.gallery.setIconSize(QSize(120, 90))
        self.gallery.setFixedHeight(150)
        self.gallery.setDragEnabled(True)
        self.gallery.setFlow(QListWidget.LeftToRight)
        self.gallery.setWrapping(False)
        self.gallery.currentItemChanged.connect(lambda *_: self._show_current())
        lay = QVBoxLayout(self)
        lay.addLayout(bar)
        lay.addWidget(scroll, 1)
        lay.addWidget(QLabel("Captures (drag one into another app):"))
        lay.addWidget(self.gallery)
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.capture_requested)
        self.live.toggled.connect(self._live_toggled)
        self.interval.valueChanged.connect(lambda v: self.timer.setInterval(v * 1000))
        self.live_image = None

    def _live_toggled(self, on):
        if on:
            self.timer.start(self.interval.value() * 1000)
            self.capture_requested.emit()
        else:
            self.timer.stop()

    def add_capture(self, img: QImage):
        """A capture result. In live view the newest frame replaces the display without filling the gallery."""
        if self.live.isChecked():
            self.live_image = img
            self._render(img)
            return
        self.counter += 1
        name = f"{datetime.date.today():%m-%d-%Y} Image {self.counter:03d}"
        item = QListWidgetItem(QIcon(QPixmap.fromImage(img)), name)
        item.setData(Qt.UserRole, img)
        self.gallery.addItem(item)
        self.gallery.setCurrentItem(item)
        QGuiApplication.clipboard().setImage(self._final(img))

    def _final(self, img):
        return with_border(img) if self.border.isChecked() else img

    def _show_current(self):
        item = self.gallery.currentItem()
        img = self.live_image if (self.live.isChecked() and self.live_image is not None) else (
            item.data(Qt.UserRole) if item else None)
        if img is not None:
            self._render(img)

    def _render(self, img):
        factor = self.zoom.currentIndex() + 1
        shown = self._final(img)
        self.view.setPixmap(QPixmap.fromImage(shown).scaled(shown.width() * factor, shown.height() * factor,
                                                            Qt.KeepAspectRatio, Qt.FastTransformation))

    def current_image(self):
        item = self.gallery.currentItem()
        if self.live.isChecked() and self.live_image is not None:
            return self.live_image, f"{datetime.date.today():%m-%d-%Y} Live"
        return (item.data(Qt.UserRole), item.text()) if item else (None, None)

    def copy_current(self):
        img, _ = self.current_image()
        if img is not None:
            QGuiApplication.clipboard().setImage(self._final(img))

    @staticmethod
    def formats():
        supported = {bytes(f).decode().lower() for f in QImageWriter.supportedImageFormats()}
        order = [("png", "PNG"), ("jpg", "JPEG"), ("bmp", "BMP"), ("tif", "TIFF"), ("gif", "GIF")]
        return [(ext, label) for ext, label in order if ext in supported or (ext == "tif" and "tiff" in supported)]

    def _save(self, img, path):
        factor = self.zoom.currentIndex() + 1  # TI saves at the displayed size
        out = self._final(img)
        out = out.scaled(out.width() * factor, out.height() * factor, Qt.KeepAspectRatio, Qt.FastTransformation)
        if not out.save(path):
            raise OSError(f"Couldn't write {path}")

    def save_current(self):
        img, name = self.current_image()
        if img is None:
            return
        filters = ";;".join(f"{label} (*.{ext})" for ext, label in self.formats())
        path, _ = QFileDialog.getSaveFileName(self, "Save screen capture", name + ".png", filters)
        if path:
            try:
                self._save(img, path)
            except OSError as e:
                QMessageBox.warning(self, "Save failed", str(e))

    def save_all(self):
        if not self.gallery.count():
            return
        folder = QFileDialog.getExistingDirectory(self, "Save all captures to")
        if not folder:
            return
        for i in range(self.gallery.count()):
            it = self.gallery.item(i)
            self._save(it.data(Qt.UserRole), os.path.join(folder, it.text() + ".png"))

    def remove_current(self):
        row = self.gallery.currentRow()
        if row >= 0:
            self.gallery.takeItem(row)
