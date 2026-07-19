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
| `emoji_db.rs` | Load `emojis.json`, build search blobs (keywords + aliases + flags), ranked filter |
| `emoji_aliases.rs` | Hand-tuned Tims/slang keywords + optional user TOML |
| `emoji_picker.rs` | egui emoji grid: search, category chips, arrow-key nav, Enter pastes, recents |
| `emoji_raster.rs` | Colour emoji textures from system NotoColorEmoji (ttf-parser CBDT/PNG) |
| `history_picker.rs` | egui history list + preview pane; ingests clipboard on open (focused-window read works on GNOME Wayland) |
| `ui_common.rs` | Floating undecorated pickers; Adwaita-like theme from GNOME color-scheme/accent |
| `settings.rs` | Preferences UI (hotkeys, OCR, max entries); app-menu launcher target |
| `install.rs` | `timbits install`: default config, autostart, launcher entries, GNOME hotkeys |
| `gnome_hotkeys.rs` | GNOME/Zorin custom keybindings via `gsettings`; clears IBus Super+. conflict |

## Key design decisions (don't regress these!)

1. **Paste (pure Rust)**: `paste.rs` ports emoji-picker.sh + inject-paste.py.
   Flow: `wl-copy` → sleep 350 ms → `ydotool key super+v` (or `ctrl+shift+v`
   in terminals via keyd-last-focus) → uinput fallback. No Python at runtime.
   Do **not** use `ydotool type` for emoji.
2. **Emoji search**: full Unicode fully-qualified set in `assets/emojis.json`
   (regenerate via Settings → Update emoji catalogue, `timbits update-emojis`, or
   `cargo run --bin update-emojis` for shipping assets). Search blob =
   name/group/keywords + `emoji_aliases.rs` + optional user TOML + flag demonyms.
   Skin-tone variants hidden; `emoji_skin_tone` applies preferred tone on paste.
   Load order: env override → user data dir if version ≥ bundled → bundled.
   `emoji_update.rs` holds the download/parse logic shared by CLI and Settings.
3. **Emoji font**: colour glyphs via `emoji_raster` (NotoColorEmoji CBDT +
   rustybuzz shaping for flags/ZWJ/skin tones); monochrome `assets/NotoEmoji.ttf`
   remains a text fallback only.
4. **egui 0.35 API**: `eframe::App` uses `fn ui(&mut self, ui: &mut egui::Ui,
   frame)`. Panels are the unified `egui::Panel::{top,bottom,left,right}` +
   `egui::CentralPanel`, all shown from that root `ui`, central LAST.
5. **Key handling**: read `ctx.input(|i| i.key_pressed(...))` at the TOP of
   `App::ui` and `consume_key` for arrows, so the focused search field doesn't
   swallow navigation keys.
6. **GNOME Wayland**: unfocused clipboard reads are impossible; daemon poll
   silently no-ops there, `history_picker` ingests on open instead. KDE/wlroots
   work via data-control. Global hotkeys don't exist on Wayland — users bind
   DE shortcuts to `timbits emoji` / `timbits clipboard`.
7. **OCR**: never link tesseract; shell out (`tesseract <png> stdout`) in a
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
assets/emojis.json  assets/emojis.version  assets/NotoEmoji.ttf  assets/logo.png
src/bin/update_emojis.rs
src/{main,config,storage,clip,ocr,paste,daemon,emoji_db,emoji_aliases,emoji_picker,emoji_raster,history_picker,install,gnome_hotkeys,ui_common}.rs
```
