//! 🍩 Timbits — an emoji picker + clipboard history tool for Linux.

mod clip;
mod config;
mod daemon;
mod emoji_picker;
mod gnome_hotkeys;
mod history_picker;
mod install;
mod ocr;
mod paste;
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
            config::ensure_dirs()?;
            if let Some(emoji) = emoji_picker::run()? {
                paste::stage_text(&emoji)?;
                paste::paste_staged("text")?;
            }
        }
        Some("clipboard") | Some("clip") => {
            config::ensure_dirs()?;
            if let Some(entry) = history_picker::run()? {
                // Bump recency so re-pasted items stay near the top.
                if let Ok(store) = storage::Store::open(&config::db_path()) {
                    let _ = store.touch(entry.id);
                }
                match entry.kind {
                    EntryKind::Image => {
                        if let Some(path) = &entry.image_path {
                            paste::stage_image(Path::new(path))?;
                            paste::paste_staged("image")?;
                        } else if let Some(text) = &entry.text {
                            // Image entry that only has a path list (rare).
                            paste::stage_text(text)?;
                            paste::paste_staged("text")?;
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
                            paste::stage_text(text)?;
                            paste::paste_staged("text")?;
                        }
                    }
                }
            }
        }
        Some("daemon") => daemon::run()?,
        Some("install") => install::run()?,
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
    timbits daemon       Watch clipboard + register hotkeys (run at login)
    timbits install      Set up config, autostart and launcher entries

HOTKEYS:
    On X11 the daemon binds hotkeys from {} (default Super+. and Super+V).
    On GNOME/Zorin Wayland, `timbits install` registers custom shortcuts automatically.
    Otherwise bind your desktop shortcuts to:
        {exe} emoji
        {exe} clipboard

NOTES:
    - Install tesseract-ocr for searchable text inside copied images and image files.
    - On Wayland, pasting needs `wtype` or a running `ydotoold`.",
        config::config_path().display(),
    );
}
