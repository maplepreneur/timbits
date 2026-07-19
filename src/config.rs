//! Configuration and well-known paths for timbits.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Backend used to inject a paste chord after the clipboard is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMethod {
    /// ydotool: uses `paste_hotkey` or `paste_hotkey_terminal` when the target
    /// looks like a terminal (if terminal auto is enabled).
    YdotoolAuto,
    /// uinput virtual keyboard with the same auto hotkey selection as YdotoolAuto.
    UinputAuto,
    /// ydotool with the primary paste hotkey only.
    YdotoolPrimary,
    /// ydotool with the terminal paste hotkey only.
    YdotoolTerminal,
    /// uinput with the primary paste hotkey only.
    UinputPrimary,
    /// uinput with the terminal paste hotkey only.
    UinputTerminal,
}

impl PasteMethod {
    pub const ALL: &[PasteMethod] = &[
        PasteMethod::YdotoolAuto,
        PasteMethod::UinputAuto,
        PasteMethod::YdotoolPrimary,
        PasteMethod::YdotoolTerminal,
        PasteMethod::UinputPrimary,
        PasteMethod::UinputTerminal,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::YdotoolAuto => "ydotool_auto",
            Self::UinputAuto => "uinput_auto",
            Self::YdotoolPrimary => "ydotool_primary",
            Self::YdotoolTerminal => "ydotool_terminal",
            Self::UinputPrimary => "uinput_primary",
            Self::UinputTerminal => "uinput_terminal",
        }
    }

    pub fn uses_ydotool(self) -> bool {
        matches!(
            self,
            Self::YdotoolAuto | Self::YdotoolPrimary | Self::YdotoolTerminal
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::YdotoolAuto => "ydotool (auto primary / terminal hotkey)",
            Self::UinputAuto => "uinput (auto primary / terminal hotkey)",
            Self::YdotoolPrimary => "ydotool + primary paste hotkey",
            Self::YdotoolTerminal => "ydotool + terminal paste hotkey",
            Self::UinputPrimary => "uinput + primary paste hotkey",
            Self::UinputTerminal => "uinput + terminal paste hotkey",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::YdotoolAuto => {
                "Inject via ydotoold using the primary paste hotkey, or the terminal \
                 hotkey when the focused app looks like a terminal."
            }
            Self::UinputAuto => {
                "Temporary virtual keyboard (uinput) with the same auto hotkey \
                 selection. Works well when a key remapper grabs uinput devices."
            }
            Self::YdotoolPrimary => {
                "Always use the primary paste hotkey through ydotool (default Ctrl+V)."
            }
            Self::YdotoolTerminal => {
                "Always use the terminal paste hotkey through ydotool (default Ctrl+Shift+V)."
            }
            Self::UinputPrimary => "Always uinput the primary paste hotkey.",
            Self::UinputTerminal => "Always uinput the terminal paste hotkey.",
        }
    }
}

fn default_paste_methods() -> Vec<PasteMethod> {
    // Generic defaults: Ctrl+V / Ctrl+Shift+V via ydotool, then uinput.
    // Users with keyd Super+V can set paste_hotkey = "Super+V" in settings.
    vec![
        PasteMethod::YdotoolAuto,
        PasteMethod::UinputAuto,
        PasteMethod::YdotoolPrimary,
        PasteMethod::UinputPrimary,
    ]
}

fn default_paste_hotkey() -> String {
    "Ctrl+V".into()
}
fn default_paste_hotkey_terminal() -> String {
    "Ctrl+Shift+V".into()
}
fn default_true() -> bool {
    true
}

fn default_focus_delay() -> u64 {
    60
}
fn default_ydotool_pre() -> u64 {
    0
}
fn default_ydotool_stroke() -> u64 {
    6
}
fn default_uinput_settle() -> u64 {
    40
}

/// Preferred Fitzpatrick skin tone for people/gesture emoji in the picker.
///
/// `None` (default) keeps the unmodified “yellow” forms and never lists the
/// five tone variants in the grid — only one row per emoji.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkinTone {
    #[default]
    None,
    Light,
    MediumLight,
    Medium,
    MediumDark,
    Dark,
}

impl SkinTone {
    pub const ALL: &[SkinTone] = &[
        SkinTone::None,
        SkinTone::Light,
        SkinTone::MediumLight,
        SkinTone::Medium,
        SkinTone::MediumDark,
        SkinTone::Dark,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Default (none)",
            Self::Light => "Light",
            Self::MediumLight => "Medium-light",
            Self::Medium => "Medium",
            Self::MediumDark => "Medium-dark",
            Self::Dark => "Dark",
        }
    }

    /// Unicode name suffix used in fully-qualified emoji names, e.g.
    /// `thumbs up: medium skin tone`.
    pub fn name_suffix(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Light => Some("light skin tone"),
            Self::MediumLight => Some("medium-light skin tone"),
            Self::Medium => Some("medium skin tone"),
            Self::MediumDark => Some("medium-dark skin tone"),
            Self::Dark => Some("dark skin tone"),
        }
    }

    /// Sample waving hand for the settings UI.
    pub fn sample(self) -> &'static str {
        match self {
            Self::None => "👋",
            Self::Light => "👋🏻",
            Self::MediumLight => "👋🏼",
            Self::Medium => "👋🏽",
            Self::MediumDark => "👋🏾",
            Self::Dark => "👋🏿",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hotkey for the emoji picker, e.g. "Super+Period".
    pub emoji_hotkey: String,
    /// Hotkey for the clipboard history picker, e.g. "Super+Shift+C".
    pub clipboard_hotkey: String,
    /// Maximum number of clipboard history entries kept.
    pub max_entries: i64,
    /// Run OCR (via the `tesseract` CLI, if installed) on clipboard images so
    /// they become searchable.
    pub ocr_enabled: bool,
    /// Preferred skin tone for people/gesture emoji. Default: none (no variants
    /// listed; paste the base form).
    #[serde(default)]
    pub emoji_skin_tone: SkinTone,

    // ── Pasting ──────────────────────────────────────────────────────────
    /// Chord injected into normal apps after copy, e.g. "Ctrl+V" or "Super+V".
    #[serde(default = "default_paste_hotkey")]
    pub paste_hotkey: String,
    /// Chord for terminal apps when auto-detect is used, e.g. "Ctrl+Shift+V".
    #[serde(default = "default_paste_hotkey_terminal")]
    pub paste_hotkey_terminal: String,
    /// When true, *Auto* methods pick the terminal hotkey for terminal windows.
    #[serde(default = "default_true")]
    pub paste_auto_terminal: bool,
    /// Wait after the picker closes / focus restore before injecting keys (ms).
    #[serde(default = "default_focus_delay")]
    pub paste_focus_delay_ms: u64,
    /// `ydotool key --delay` (ms before the chord starts). 0 is fastest.
    #[serde(default = "default_ydotool_pre")]
    pub paste_ydotool_delay_ms: u64,
    /// `ydotool key --key-delay` between key events (ms).
    #[serde(default = "default_ydotool_stroke")]
    pub paste_ydotool_key_delay_ms: u64,
    /// Wait after creating the virtual keyboard so keyd can grab it (ms).
    #[serde(default = "default_uinput_settle")]
    pub paste_uinput_settle_ms: u64,
    /// Wait for wl-paste to echo claimed text before injecting (slower if true).
    #[serde(default)]
    pub paste_verify_clipboard: bool,
    /// Try clipboard primary selection as well as clipboard.
    #[serde(default = "default_true")]
    pub paste_also_primary: bool,
    /// Ordered list of paste injection backends (first success wins).
    #[serde(default = "default_paste_methods")]
    pub paste_methods: Vec<PasteMethod>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            emoji_hotkey: "Super+Period".into(),
            clipboard_hotkey: "Super+Shift+C".into(),
            max_entries: 500,
            ocr_enabled: true,
            emoji_skin_tone: SkinTone::None,
            paste_hotkey: default_paste_hotkey(),
            paste_hotkey_terminal: default_paste_hotkey_terminal(),
            paste_auto_terminal: true,
            paste_focus_delay_ms: default_focus_delay(),
            paste_ydotool_delay_ms: default_ydotool_pre(),
            paste_ydotool_key_delay_ms: default_ydotool_stroke(),
            paste_uinput_settle_ms: default_uinput_settle(),
            paste_verify_clipboard: false,
            paste_also_primary: true,
            paste_methods: default_paste_methods(),
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
        let mut cfg: Self =
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        if cfg.paste_methods.is_empty() {
            cfg.paste_methods = default_paste_methods();
        }
        // Deduplicate while preserving order.
        let mut seen = std::collections::HashSet::new();
        cfg.paste_methods
            .retain(|m| seen.insert(std::mem::discriminant(m)));
        // Migrate empty hotkeys from older configs.
        if cfg.paste_hotkey.trim().is_empty() {
            cfg.paste_hotkey = default_paste_hotkey();
        }
        if cfg.paste_hotkey_terminal.trim().is_empty() {
            cfg.paste_hotkey_terminal = default_paste_hotkey_terminal();
        }
        // Older configs may still list removed method variants; drop unknowns via serde,
        // and if nothing remains after load of broken data, restore defaults.
        if cfg.paste_methods.is_empty() {
            cfg.paste_methods = default_paste_methods();
        }
        Ok(cfg)
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

/// Optional user search keywords: `~/.config/timbits/emoji_aliases.toml`.
pub fn emoji_aliases_path() -> PathBuf {
    config_dir().join("emoji_aliases.toml")
}

/// Optional override / installed copy of the emoji catalogue.
pub fn emojis_json_path() -> PathBuf {
    data_dir().join("emojis.json")
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
