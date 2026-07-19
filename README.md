# 🍩 Timbits

An emoji picker + clipboard history tool for Linux (built for Zorin OS, works
on other distros). Named after the Tim Hortons donut holes.

## Features

- **Emoji picker** — opens with the cursor in the search box. Type to search
  the full Unicode emoji set (Emoji 17, fully-qualified) by name or keyword
  (e.g. `canada`, `canadian flag`, `lol`, `coffee`), browse by category chip,
  move with the arrow keys, hit **Enter** and the emoji is pasted right where
  your cursor was. Remembers recent picks.
- **Clipboard history** — stores text, file copies (uri-list) and images
  (screenshots). Search filters everything, including **text inside images
  and image files** via OCR (tesseract). Arrow keys to navigate, **Enter**
  pastes into the previously focused window, **Ctrl+Delete** forgets an entry.
- **Hotkeys** — set your own (default `Super+.` emoji, `Super+Shift+C` history).
- **Emoji skin tone** — in Settings, pick a preferred tone (default: none). The
  picker lists each emoji once and pastes hands/people in your preferred tone.

## Install

```bash
./install.sh
```

This builds a release binary into `~/.local/bin/timbits`, writes a default
config, adds an autostart entry for the daemon, creates launcher entries, and
on **GNOME / Zorin** registers the emoji and clipboard hotkeys via `gsettings`.

Requirements:

- **Rust toolchain** (build time) — https://rustup.rs
- **Wayland pasting (GNOME)**: `wl-copy`/`wl-paste` plus a running **ydotoold**
  (`ydotool`; socket often `/tmp/.ydotool_socket`). `wtype` is not supported on
  GNOME. On X11, `xdotool`/`enigo` work.
- **Colour emoji**: system `fonts-noto-color-emoji` (Noto Color Emoji)
- **Emoji search data**: bundled `emojis.json` (Unicode Emoji 17 fully-qualified
  set + emojilib-style keywords) plus built-in aliases; optional
  `~/.config/timbits/emoji_aliases.toml`. Refresh the catalogue with
  **Settings → Update emoji catalogue**, or `timbits update-emojis` (network).
  Dev shipping builds: `cargo run --bin update-emojis` then rebuild.
- **Optional, for image OCR**: `sudo apt install tesseract-ocr`
- **No Python required** for the app (paste is pure Rust)

## Hotkey setup

Edit hotkeys in `~/.config/timbits/config.toml`, then re-run `timbits install`
(or restart the daemon on X11):

```toml
emoji_hotkey = "Super+Period"
clipboard_hotkey = "Super+Shift+C"
max_entries = 500
ocr_enabled = true
```

### GNOME / Zorin (Wayland)

`timbits install` registers custom shortcuts automatically (defaults
`Super+.` and `Super+Shift+C`). It also clears the system/IBus Super+. emoji
chord so Timbits owns that key (IBus may keep Super+; for the stock picker).
No manual Settings trip needed unless you prefer the GUI.

### Other Wayland desktops

Bind custom shortcuts in your DE to:

| Name                | Command                          | Shortcut   |
|---------------------|----------------------------------|------------|
| Timbits Emoji       | `~/.local/bin/timbits emoji`     | `Super+.`  |
| Timbits Clipboard   | `~/.local/bin/timbits clipboard` | `Super+Shift+C` |

### X11 sessions

The daemon grabs the hotkeys automatically. Make sure it is running (it
autostarts on login after install, or run `timbits daemon &`).

## Usage

```
timbits emoji        # emoji picker
timbits clipboard    # clipboard history
timbits settings     # preferences (also the app-menu launcher)
timbits daemon       # clipboard watcher (+ hotkeys on X11)
timbits install      # first-time setup
```

## How pasting works

On **GNOME Wayland**, Timbits puts the selection on the clipboard with
`wl-copy`, then synthesizes `Ctrl+V` with **ydotool** (requires `ydotoold`).
On X11 it uses arboard + enigo/xdotool. A legacy `__serve-clip` helper remains
as fallback.

If key synthesis isn't available, the item is still on your clipboard — just
press `Ctrl+V` yourself.

## Wayland notes

- **GNOME (Zorin)**: the compositor only lets the *focused* window read the
  clipboard, so the background daemon can't see copies made while unfocused.
  The history picker ingests the current clipboard every time it opens (it is
  focused then), so the item you want is always captured. On KDE Plasma and
  wlroots compositors the daemon watches continuously via the data-control
  protocol.
- Global hotkeys: use your DE's custom shortcuts (see above). There is no
  cross-compositor global-hotkey API on Wayland.

## Data

- `~/.local/share/timbits/history.db` — SQLite history
- `~/.local/share/timbits/images/` — copied images (PNG)
- `~/.config/timbits/config.toml` — settings

## Development

```bash
cargo build     # debug build
cargo test      # unit tests (storage, hotkey parsing, clipboard roundtrip)
```

See `AGENTS.md` for the architecture overview.
