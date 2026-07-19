//! 🍩 Timbits — an emoji picker + clipboard history tool for Linux.

mod clip;
mod config;
mod daemon;
mod emoji_picker;
mod emoji_raster;
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
use std::process::Command;

use crate::storage::EntryKind;

/// Path to the working GTK emoji picker wrapper (colour emoji + Super+V paste).
const LEGACY_EMOJI_SCRIPT: &str = "/home/maplepreneur/dotfiles/Zorin/keyd/emoji-picker.sh";

/// Run the proven shell/GTK emoji flow. Returns Ok(true) if the script ran
/// (even if the user cancelled with no selection).
fn try_run_legacy_emoji_script() -> Result<bool> {
    if !Path::new(LEGACY_EMOJI_SCRIPT).is_file() {
        log::info!("legacy emoji script not found; using egui picker");
        return Ok(false);
    }
    log::info!("launching proven emoji picker: {LEGACY_EMOJI_SCRIPT}");
    // Script is designed to run as root under keyd (uses sudo -u for the GUI).
    // When we are already the desktop user, still run it — user_env uses sudo -u.
    let status = Command::new(LEGACY_EMOJI_SCRIPT).status()?;
    if !status.success() {
        log::warn!("emoji-picker.sh exited with {status}");
    }
    Ok(true)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("emoji") => {
            // Prefer the proven GTK picker + shell paste path (colour emoji +
            // wl-copy + ydotool super+v). Falls back to the egui picker if the
            // script is missing.
            config::ensure_dirs()?;
            if try_run_legacy_emoji_script()? {
                // Script handled UI + paste.
            } else {
                let prev = focus::capture_focused();
                if let Some(emoji) = emoji_picker::run()? {
                    focus::restore(prev);
                    paste::paste_text(&emoji)?;
                }
            }
        }
        Some("clipboard") | Some("clip") => {
            config::ensure_dirs()?;
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
                            // Image entry that only has a path list (rare).
                            paste::paste_text(text)?;
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
                            paste::paste_text(text)?;
                        }
                    }
                }
            }
        }
        Some("daemon") => daemon::run()?,
        Some("install") => install::run()?,
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
