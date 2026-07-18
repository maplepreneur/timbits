# 🍩 Timbits

An emoji picker + clipboard history tool for Linux (built for Zorin OS, works
on other distros). Named after the Tim Hortons donut holes.

## Features

- **Emoji picker** — opens with the cursor in the search box. Type to search
  ~1900 emojis by name/keyword, move with the arrow keys, hit **Enter** and the
  emoji is pasted right where your cursor was. Remembers recent picks.
- **Clipboard history** — stores text, file copies and images (screenshots).
  Search filters everything, including **text inside images** via OCR
  (tesseract). Arrow keys to navigate, **Enter** pastes into the previously
  focused window, **Ctrl+Delete** forgets an entry.
- **Hotkeys** — set your own (default `Super+.` emoji, `Super+V` history).

## Install

```bash
./install.sh
```

This builds a release binary into `~/.local/bin/timbits`, writes a default
config, adds an autostart entry for the daemon, and creates launcher entries.

Requirements:

- **Rust toolchain** (build time) — https://rustup.rs
- **Wayland pasting**: `wtype` (or a running `ydotoold`) — *already installed
  on this machine*
- **Optional, for image OCR**: `sudo apt install tesseract-ocr`

## Hotkey setup

### Wayland (Zorin OS default)

GNOME/Wayland does not let apps grab global hotkeys, so bind custom shortcuts:

**Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → +**

| Name                | Command                        | Shortcut   |
|---------------------|--------------------------------|------------|
| Timbits Emoji       | `~/.local/bin/timbits emoji`   | `Super+.`  |
| Timbits Clipboard   | `~/.local/bin/timbits clipboard` | `Super+V` |

### X11 sessions

The daemon grabs the hotkeys automatically. Edit them in
`~/.config/timbits/config.toml`:

```toml
emoji_hotkey = "Super+Period"
clipboard_hotkey = "Super+V"
max_entries = 500
ocr_enabled = true
```

Then make sure the daemon is running (it autostarts on login after install, or
run `timbits daemon &`).

## Usage

```
timbits emoji        # emoji picker
timbits clipboard    # clipboard history
timbits daemon       # clipboard watcher (+ hotkeys on X11)
timbits install      # first-time setup
```

## How pasting works

When you pick something, timbits stages the content, spawns a tiny helper
(`timbits __serve-clip`) that owns the clipboard selection until you copy
something else (this is what makes the paste survive the picker closing on
both X11 and Wayland), then synthesizes `Ctrl+V` in the window you came from
(`wtype`/`ydotool` on Wayland, `enigo`/`xdotool` on X11).

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
