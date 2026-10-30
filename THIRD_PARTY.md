# Third-party code and credits

CalcBridge is licensed under the GNU GPL v3 or later (see `LICENSE`). It builds on the work below.

## Code included in this repository

### TnsTools — `src/calcbridge/tns/tixc_decode.py`
The TIXC token-stream decoder is vendored from TnsTools (https://github.com/MaksimirKurtov/TnsTools),
with one change: literal closing tags (which TI writes once its 256-entry tag dictionary is full) are
passed through. The method-13 header layout in `tns/format.py` follows TnsTools' notes.

```
MIT License

Copyright (c) 2026 tnstools contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Protocol and format knowledge (ported or re-implemented)

- **libnspire** (https://github.com/Vogtinator/libnspire, GPL-3.0-or-later): the NNSE link layer, NavNet
  packets and the device-info, file, screenshot and OS-install services in `src/calcbridge/protocol/` are
  a Python port. This is why CalcBridge is GPL-3.0.
- **tilibs / libticalcs** (https://github.com/debrouxl/tilibs, GPL-2.0-or-later): the remote keypress
  service and key codes in `protocol/keys.py`.
- **Luna** (https://github.com/ndless-nspire/Luna, MPL-1.1): documented the .tns container and the
  Python/Lua document structure. No Luna code or data is included; `tns/format.py` is an independent
  implementation.
- **hackspire.org**: USB protocol notes and result codes.
- **n-link** (https://github.com/lights0123/n-link): reference for how a desktop app drives libnspire.

Facts found on real hardware (CX II, OS 5.4) are marked "observed" in the code comments.

## Runtime dependencies (installed separately or bundled in the app builds)

| Package | License |
|---|---|
| PySide6 / Qt for Python (PySide6-Essentials) | LGPL-3.0 |
| pyusb | BSD-3-Clause |
| libusb (via libusb-package) | LGPL-2.1-or-later |
| pycryptodome | BSD-2-Clause / Public Domain |

## Trademarks

TI-Nspire and TI-Nspire CX II are trademarks of Texas Instruments. CalcBridge is an independent project
and is not affiliated with or endorsed by Texas Instruments.
