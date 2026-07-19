//! Configuration and well-known paths for timbits.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hotkey for the emoji picker, e.g. "Super+Period".
    /// On X11 the daemon grabs this; on GNOME/Zorin Wayland, `timbits install`
    /// registers it as a custom shortcut (and clears the system Super+. emoji).
    pub emoji_hotkey: String,
    /// Hotkey for the clipboard history picker, e.g. "Super+Shift+C".
    /// On X11 the daemon grabs this; on GNOME/Zorin Wayland, `timbits install`
    /// registers the same binding via GNOME shortcuts.
    pub clipboard_hotkey: String,
    /// Maximum number of clipboard history entries kept.
    pub max_entries: i64,
    /// Run OCR (via the `tesseract` CLI, if installed) on clipboard images so
    /// they become searchable.
    pub ocr_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            emoji_hotkey: "Super+Period".into(),
            clipboard_hotkey: "Super+Shift+C".into(),
            max_entries: 500,
            ocr_enabled: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(config_dir())?;
        fs::write(config_path(), toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("timbits")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("timbits")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn db_path() -> PathBuf {
    data_dir().join("history.db")
}

pub fn images_dir() -> PathBuf {
    data_dir().join("images")
}

pub fn recents_path() -> PathBuf {
    data_dir().join("recent_emojis.txt")
}

pub fn pending_text_path() -> PathBuf {
    data_dir().join("pending_text")
}

pub fn pending_image_path() -> PathBuf {
    data_dir().join("pending_image.png")
}

/// Written by `__serve-clip` once the clipboard offer is live.
pub fn serve_ready_path() -> PathBuf {
    data_dir().join("serve_ready")
}

pub fn serve_log_path() -> PathBuf {
    data_dir().join("serve.log")
}

pub fn ensure_dirs() -> Result<()> {
    fs::create_dir_all(config_dir())?;
    fs::create_dir_all(images_dir())?;
    Ok(())
}

pub fn is_wayland() -> bool {
    if std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
    {
        return true;
    }
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}
