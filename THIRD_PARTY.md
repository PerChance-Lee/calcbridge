# Third-party code and credits

CalcBridge is licensed under the GNU GPL v3 or later (see `LICENSE`). It builds on the work below.

## Code included in this repository

### TnsTools — `crates/tns/src/tixc_decode.rs`
The TIXC token-stream decoder is ported from TnsTools (https://github.com/MaksimirKurtov/TnsTools), with
one change: literal closing tags (which TI writes once its 256-entry tag dictionary is full) are passed
through. The method-13 header layout in `crates/tns/src/crypto.rs` follows TnsTools' notes.

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
  packets and the device-info, file, screenshot and OS-install services in `crates/link` are a port.
  This is why CalcBridge is GPL-3.0.
- **tilibs / libticalcs** (https://github.com/debrouxl/tilibs, GPL-2.0-or-later): the remote keypress
  service and key codes in `crates/link/src/keys.rs`.
- **Luna** (https://github.com/ndless-nspire/Luna, MPL-1.1): documented the .tns container and the
  Python/Lua document structure. No Luna code or data is included; `crates/tns` is an independent
  implementation.
- **hackspire.org**: USB protocol notes and result codes.
- **n-link** (https://github.com/lights0123/n-link): reference for how a desktop app drives libnspire.

Facts found on real hardware (CX II, OS 5.4) are marked "observed" in the code comments and collected
in `docs/PROTOCOL.md`.

## Libraries built into the app

All are under permissive licenses compatible with the GPL. The main ones:

| Library | Used for | License |
|---|---|---|
| Tauri, tauri-plugin-dialog / -opener / -window-state / -single-instance | the app shell | MIT OR Apache-2.0 |
| nusb | USB (pure Rust, no libusb) | MIT OR Apache-2.0 |
| RustCrypto `des`, `cipher` | the .tns 3DES layer | MIT OR Apache-2.0 |
| flate2 / miniz_oxide | deflate | MIT OR Apache-2.0 |
| roxmltree | reading document XML | MIT OR Apache-2.0 |
| image | saving screenshots | MIT OR Apache-2.0 |
| serde, serde_json, thiserror, log, getrandom, clap | plumbing, the command line | MIT OR Apache-2.0 |
| React, React DOM | the interface | MIT |
| KaTeX (including its fonts) | drawing 2D math | MIT |

`cargo tree` and `app/package-lock.json` list every transitive dependency.

## Trademarks

TI-Nspire and TI-Nspire CX II are trademarks of Texas Instruments. CalcBridge is an independent project
and is not affiliated with or endorsed by Texas Instruments.
