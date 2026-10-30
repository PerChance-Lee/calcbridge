# TI-Nspire CX II USB protocol notes

What CalcBridge relies on, including behaviour observed on a real CX II running OS 5.4 that isn't
written down elsewhere. Sources: libnspire, tilibs, hackspire.org, Luna, TnsTools, and testing.

## USB
- VID `0x0451`, PID `0xE022` (CX II family). One vendor-specific interface, bulk endpoints `0x81` (in)
  and `0x01` (out).
- **Windows** binds WinUSB automatically (MS OS descriptors) — no driver install. **macOS** needs no
  driver. Only one program can claim the interface at a time (close TI's software first).

## NNSE link layer (CX II only)
12-byte big-endian header: `misc, service, src, dest, unknown, reqAck, length(2), seqno(2), checksum(2)`.
The checksum makes the 16-bit ones'-complement sum of the whole message `0xFFFF`. Services:
`0x01` address request, `0x02` time, `0x03` echo, `0x04` stream (carries NavNet), `0x08` "unknown"
(answer `81 03`); bit `0x80` marks an ack. Host address `0xFE`, calculator `0x01`, broadcast `0xFF`.

**Handshake:** after plug-in the calculator repeats address requests (`00` + 64-byte client ID). Answer
with `80` (twice — it sometimes drops the first), answer its `0x08` with `81 03`, and its time request
with `80` + u32 Unix time + 12 zero bytes. Then stream packets flow, each acked by seqno.

**Observed:**
- The calculator accepts a stream packet only if its seqno is exactly *last + 1*. *last* itself is acked
  again but not processed (a duplicate); anything else is silently dropped.
- The handshake happens once per USB connection. A program that connects later sees a silent link and
  must rediscover the sequence number — CalcBridge stores it per device and probes forward from a few
  numbers behind. `libusb_reset_device` on Windows (WinUSB) does **not** restart the handshake.
- After ~1.5 s of idle the calculator drops the **first** packet it receives. Retransmitting the same
  seqno (reqAck `0x09`, "not the first try") gets it through, so every send retries on a ~0.7 s timeout.

## NavNet
16-byte big-endian header: `0x54FD, src_addr, src_sid, dst_addr, dst_sid, data_checksum (CRC-like),
size, ack, seq, header_checksum (byte sum)`; `size = 0xFF` means a u32 length follows (up to 1440 bytes on
the CX II). Host address `0x6400`, calculator `0x6401`. The first packet assigns the address
(`0x4003 → 0x4003`, data `64 01 FF 00`). Each operation connects to a service from a fresh host session
id (`0x8000…`) and ends with a disconnect to `0x40DE` carrying that id. Packets for another session are
nacked (`src_sid 0xD3`), except disconnect notices, which are acked.

| Service | Use | Notes |
|---|---|---|
| `0x4020` | device info | `01` → 109-byte struct (flash/RAM free+total u64, versions, LCD, IDs); `02` → product name; `03` → `.tns\0.tco2\0` |
| `0x4024` | screenshot | `00` → header (u32 size, w, h, bpp) then RLE chunks; pixels are little-endian RGB565 |
| `0x4042` | keypresses | `01 00 00 80`, then a 26-byte event with ASCII, scan code, modifier (tilibs codes) — **works on the CX II** |
| `0x4060` | files | list `0D/0E/0F`, stat `2001`, read `0701`, write `0301`, mkdir `0A03`, rmdir `0B03`, delete `0901`, move/rename `2101`, copy `0C01` (files only) |
| `0x4080` | OS install | `03`+u32 size → `04`; chunks `05`+data; status after the first chunk; progress `06 pct` until ≥100 |

File results are `FF xx`: `00` OK, `02` out of memory, `07` packet too large, `09` parent missing, `0A`
not found, `0F` invalid folder name, `12` folder exists, `13` path too long, `14` invalid file name,
`15` no extension (uploads must end in `.tns`), `16` forbidden characters, OS install `20` wrong OS type,
`80` downgrade refused.

## .tns documents
A ZIP variant: first local header magic `*TIMLP` + 4 hex digits (version, e.g. `0500`), end-of-directory
magic `TIPD`. XML parts (`Document.xml`, `ProblemN.xml`) use method 13: a 40-byte header 3DES-ECB
encrypted with a fixed key (`TIEN0100`, block count, counter seed, 21 bytes of key material), then a
3DES counter-mode keystream XORed over raw-deflated **TIXC** (tokenised XML). Scripts (`.py`) and images
are plain deflate.

Notes pages are `TI.Notepad` ver 2.0 widgets whose `np:fmtxt` holds an escaped `r2dtotree`: a format
table (`fst` 1 bold / 2 italic, `fesub`/`fesup`, `tc` = 0xRRGGBB, math font "TI-Nspire") and a tree of
`1doc › 1para › 1rtline › 1word` leaves. A math box is `0el flags=8` + `mbox` attributes + an expression
tree using templates `0fraction` (`((a)/(b))`), `0sqrt`, `0power`, and `0paren` leaves.

**Observed rendering on OS 5.4:** no glyph for `⋯` or for combining/modifier hats (precomposed `ŷ` is
fine); an italic *f* hides a prime drawn right after it; math boxes wrap at plain spaces; the dashed red
outline around a math box marks the cursor, not an error.
