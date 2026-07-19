//! 🍩 Timbits — an emoji picker + clipboard history tool for Linux.

mod clip;
mod config;
mod daemon;
mod emoji_aliases;
mod emoji_db;
mod emoji_picker;
mod emoji_raster;
mod emoji_update;
mod focus;
mod gnome_hotkeys;
mod history_picker;
mod install;
mod ocr;
mod paste;
mod settings;
mod storage;
mod ui_common;

use anyhow::Result;
use std::path::Path;

use crate::storage::EntryKind;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("emoji") => {
            // egui UI + pure-Rust paste (wl-copy + ydotool super+v / uinput).
            config::ensure_dirs()?;
            let focus_before = paste::snapshot_focus_class();
            let prev = focus::capture_focused();
            if let Some(emoji) = emoji_picker::run()? {
                focus::restore(prev);
                paste::paste_text_with_focus(&emoji, &focus_before)?;
            }
        }
        Some("clipboard") | Some("clip") => {
            config::ensure_dirs()?;
            let focus_before = paste::snapshot_focus_class();
            let prev = focus::capture_focused();
            if let Some(entry) = history_picker::run()? {
                // Bump recency so re-pasted items stay near the top.
                if let Ok(store) = storage::Store::open(&config::db_path()) {
                    let _ = store.touch(entry.id);
                }
                focus::restore(prev);
                match entry.kind {
                    EntryKind::Image => {
                        if let Some(path) = &entry.image_path {
                            paste::stage_image(Path::new(path))?;
                            paste::paste_staged("image")?;
                        } else if let Some(text) = &entry.text {
                            paste::paste_text_with_focus(text, &focus_before)?;
                        }
                    }
                    EntryKind::Files => {
                        if let Some(text) = &entry.text {
                            paste::stage_text(text)?;
                            paste::paste_staged("files")?;
                        }
                    }
                    EntryKind::Text => {
                        if let Some(text) = &entry.text {
                            paste::paste_text_with_focus(text, &focus_before)?;
                        }
                    }
                }
            }
        }
        Some("daemon") => daemon::run()?,
        Some("install") => install::run()?,
        Some("update-emojis") => {
            let assets = std::env::args().any(|a| a == "--assets");
            if assets {
                let root = std::env::var("CARGO_MANIFEST_DIR")
                    .map(std::path::PathBuf::from)
                    .or_else(|_| {
                        // Walk up from cwd looking for the repo root.
                        let mut dir = std::env::current_dir()?;
                        loop {
                            if dir.join("Cargo.toml").is_file() && dir.join("assets").is_dir() {
                                return Ok(dir);
                            }
                            if !dir.pop() {
                                anyhow::bail!(
                                    "could not find workspace root for --assets \
                                     (set CARGO_MANIFEST_DIR or run from the repo)"
                                );
                            }
                        }
                    })?;
                let report = emoji_update::update_workspace_assets(&root)?;
                println!(
                    "Wrote {} ({} emoji, Unicode {})",
                    report.json_path.display(),
                    report.count,
                    report.version
                );
                println!("Rebuild (`cargo build --release`) to ship the new catalogue in the binary.");
            } else {
                let report = emoji_update::update_user_catalogue()?;
                println!(
                    "Updated {} ({} emoji, Unicode {})",
                    report.json_path.display(),
                    report.count,
                    report.version
                );
                println!("Open the emoji picker to use the new catalogue.");
            }
        }
        Some("settings") | Some("prefs") | Some("preferences") => {
            config::ensure_dirs()?;
            settings::run()?;
        }
        // Hidden helper: serves staged clipboard content until the selection
        // is taken over, so pastes survive the picker process exiting.
        Some("__serve-clip") => {
            let kind = std::env::args().nth(2).unwrap_or_else(|| "text".into());
            clip::serve_pending(&kind)?;
        }
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "timbits".into());
    println!(
        "🍩 Timbits — emoji picker & clipboard history for Linux

USAGE:
    timbits emoji        Open the emoji picker (search, arrows, Enter to paste)
    timbits clipboard    Open clipboard history (search incl. OCR'd images)
    timbits settings     Open preferences (hotkeys, OCR, history size)
    timbits daemon       Watch clipboard + register hotkeys (run at login)
    timbits install      Set up config, autostart and launcher entries
    timbits update-emojis  Download latest Unicode emoji catalogue (network)

HOTKEYS:
    On X11 the daemon binds hotkeys from {} (default Super+. and Super+Shift+C).
    On GNOME/Zorin Wayland, `timbits install` registers custom shortcuts automatically.
    Otherwise bind your desktop shortcuts to:
        {exe} emoji
        {exe} clipboard

NOTES:
    - Install tesseract-ocr for searchable text inside copied images and image files.
    - On GNOME Wayland pasting uses wl-copy + ydotoold (wtype is not supported).",
        config::config_path().display(),
    );
}
