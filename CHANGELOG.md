# Changelog

## 2.0.0 (unreleased)

A rewrite in Rust: the same features as 1.x, plus several new ones, in a much smaller and faster app.

**New**
- Connects as soon as the calculator is plugged in (USB hotplug instead of polling), and again after a
  re-plug or an OS install.
- Live screen at about 10 frames a second (1.x refreshed once a second at most), with a captures strip.
- Linux builds (AppImage and `.deb`, with a udev rule) alongside Windows and macOS.
- Windows installer (per-user, no admin) and MSI; 3.5 MB instead of a 47 MB zip.
- Create: several Notes pages (`---`), `# Title` lines, Greek letters and symbols as `\alpha`, `\pi`,
  `\le`, `\infty`, …, a symbol palette, a Python preview that shows what fits on the handheld's screen.
- `.md` files keep their math when sent (1.x sent them as plain text).
- Files: filter boxes, a typed path, Properties, rename keeps `.tns`, *Replace / Skip* for clashes.
- Dark mode; keyboard shortcuts listed in Settings; Log with Copy.
- Command line: `calcbridge devices`, `calcbridge keys`, `--trace`.
- Backup carries on past a file the handheld won't send and names it at the end (1.x stopped at the
  first one).

**Fixed (1.x bugs that don't carry over)**
- The keypad's *i* key typed the letter i instead of the imaginary unit.
- Delete and Tab on the keyboard didn't reach the handheld from the keypad tab.
- Capture and the keypad screen could stop updating after a disconnect.
- Closing the window during a transfer froze it; now it asks first.
- A failed paste after Cut lost the clipboard; moving now checks for clashes before anything moves.
- `/PyLib` is created when installing a module, and Create makes a missing target folder.
- Lua scripts with characters outside ASCII (π, é, ≤, …) made documents the handheld rejected as
  badly formatted.
- A reply left over from an interrupted run could be taken as an answer by the next one, giving
  garbled listings or "short reply" errors.

**Changed**
- The keypad's *ctrl* and *shift* keys are replaced by the combined keys the protocol supports
  (ctrl·enter, ctrl·menu, ctrl·tab, shift·tab, …): remote key events can't hold a modifier.
- OS install still works as in 1.x and still hasn't been tried on real hardware.

## 1.0.0 (2026-10-30)

First release: Python and Qt. Files, screen capture, remote keypad, Create, Viewer, backup and restore,
OS install, command line.
