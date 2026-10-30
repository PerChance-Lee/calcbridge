"""Screenshot pixel format: the CX II sends little-endian RGB565 (red in the high bits)."""
import struct

import pytest

pytest.importorskip("PySide6")
from PySide6.QtGui import QColor  # noqa: E402

from calcbridge.gui.screen_panel import screenshot_to_image, with_border  # noqa: E402
from calcbridge.protocol.services import Screenshot  # noqa: E402


def test_rgb565_colours():
    # pure red, green, blue pixels as the handheld encodes them
    px = struct.pack("<HHH", 0xF800, 0x07E0, 0x001F) + bytes(2 * (4 - 3))
    img = screenshot_to_image(Screenshot(4, 1, 16, px))
    assert QColor(img.pixel(0, 0)).red() > 240 and QColor(img.pixel(0, 0)).blue() < 10
    assert QColor(img.pixel(1, 0)).green() > 240
    assert QColor(img.pixel(2, 0)).blue() > 240 and QColor(img.pixel(2, 0)).red() < 10


def test_border_size():
    img = screenshot_to_image(Screenshot(320, 240, 16, bytes(320 * 240 * 2)))
    b = with_border(img)
    assert (b.width(), b.height()) == (326, 246)  # TI's bordered capture size
