# AGENTS.md — Timbits

Emoji picker + clipboard history tool for Linux. Single-binary Rust app using
egui (eframe 0.35), SQLite (rusqlite bundled) and arboard.

## Commands

- Build: `cargo build` (debug), `cargo build --release` (install build)
- Test: `cargo test`
- Run: `./target/debug/timbits emoji|clipboard|daemon|install`
- Install for the user: `./install.sh`

## Architecture

One binary, subcommands dispatched in `src/main.rs`:

| Module | Role |
|---|---|
| `config.rs` | TOML config + XDG paths (`~/.config/timbits`, `~/.local/share/timbits`) |
| `storage.rs` | SQLite layer (`history.db`). Entries: text/image/files, dedupe by content hash, `ocr_text` column makes images searchable |
| `clip.rs` | Clipboard read (arboard: file_list → text → image), hashing, PNG saving, ingest + OCR, `__serve-clip` helper |
| `ocr.rs` | Shells out to `tesseract` CLI at runtime (optional, no dev deps) |
| `paste.rs` | Stage content → spawn `__serve-clip` → synthesize Ctrl+V (`wtype`/`ydotool` on Wayland, `enigo`/`xdotool` on X11) |
| `daemon.rs` | Clipboard poll loop (700 ms) + X11 global hotkeys (global-hotkey crate) + hotkey string parser |
| `emoji_picker.rs` | egui emoji grid: search-first, arrow-key nav, Enter pastes, recents file |
| `history_picker.rs` | egui history list + preview pane; ingests clipboard on open (focused-window read works on GNOME Wayland) |
| `ui_common.rs` | Floating undecorated pickers; Adwaita-like theme from GNOME color-scheme/accent |
| `emoji_raster.rs` | Colour emoji textures from system NotoColorEmoji (ttf-parser CBDT/PNG) |
| `settings.rs` | Preferences UI (hotkeys, OCR, max entries); app-menu launcher target |
| `install.rs` | `timbits install`: default config, autostart, launcher entries, GNOME hotkeys |
| `gnome_hotkeys.rs` | GNOME/Zorin custom keybindings via `gsettings`; clears IBus Super+. conflict |

## Key design decisions (don't regress these!)

1. **Emoji paste (proven path)**: Super+. / Super+E run
   `dotfiles/Zorin/keyd/emoji-picker.sh` → GTK4 picker (`emoji-picker.py`,
   Noto Color Emoji) → `wl-copy` → sleep 350 ms → `ydotool key super+v`
   (or `ctrl+shift+v` in terminals) → fallback `inject-paste.py`.
   Do **not** use `ydotool type` for emoji (exits 0, injects nothing).
   Clipboard history paste uses the same Super+V chord.
2. **Emoji font**: egui/ab_glyph cannot rasterize color CBDT fonts (Noto Color
   Emoji). We bundle monochrome `assets/NotoEmoji.ttf` (variable font from
   google/fonts, ~2 MB) and register it as a fallback family in
   `ui_common::apply_fonts`. Keep it in git.
3. **egui 0.35 API**: `eframe::App` uses `fn ui(&mut self, ui: &mut egui::Ui,
   frame)`. Panels are the unified `egui::Panel::{top,bottom,left,right}` +
   `egui::CentralPanel`, all shown from that root `ui`, central LAST.
4. **Key handling**: read `ctx.input(|i| i.key_pressed(...))` at the TOP of
   `App::ui` and `consume_key` for arrows, so the focused search field doesn't
   swallow navigation keys.
5. **GNOME Wayland**: unfocused clipboard reads are impossible; daemon poll
   silently no-ops there, `history_picker` ingests on open instead. KDE/wlroots
   work via data-control. Global hotkeys don't exist on Wayland — users bind
   DE shortcuts to `timbits emoji` / `timbits clipboard`.
6. **OCR**: never link tesseract; shell out (`tesseract <png> stdout`) in a
   background thread, update `ocr_text` afterwards.

## Conventions

- anyhow::Result everywhere in app code; rusqlite errors converted with `?`.
- Keep dependencies minimal; prefer shelling out to standard tools
  (`wtype`, `ydotool`, `tesseract`) over heavy native bindings.
- Tests must not touch the user's real XDG data dirs (storage tests use
  `std::env::temp_dir()`).
- The clipboard roundtrip test needs X11 (XWayland); it skips if `DISPLAY`
  is unset.

## Layout

```
Cargo.toml  install.sh  README.md  AGENTS.md
assets/NotoEmoji.ttf  assets/logo.png
src/{main,config,storage,clip,ocr,paste,daemon,emoji_picker,history_picker,install,gnome_hotkeys,ui_common}.rs
```
