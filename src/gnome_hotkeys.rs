//! Install GNOME/Zorin custom keyboard shortcuts for the pickers.
//!
//! On Wayland there is no global hotkey grab API, so we register DE-level
//! shortcuts via `gsettings` (same approach as the sibling double-double app).

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::config::Config;

const SCHEMA_LIST: &str = "org.gnome.settings-daemon.plugins.media-keys";
const KEY_LIST: &str = "custom-keybindings";

const EMOJI_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/timbits-emoji/";
const CLIPBOARD_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/timbits-clipboard/";

fn gsettings(args: &[&str]) -> Result<String> {
    let output = Command::new("gsettings")
        .args(args)
        .output()
        .context("run gsettings")?;
    if !output.status.success() {
        bail!(
            "gsettings {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_path_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed == "@as []" || trimmed == "[]" {
        return Vec::new();
    }
    trimmed
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|s| {
            let s = s.trim().trim_matches('\'').trim_matches('"');
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

fn format_path_list(paths: &[String]) -> String {
    if paths.is_empty() {
        return "[]".into();
    }
    let inner = paths
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// Preferred installed binary: `~/.local/bin/timbits` when present, else current exe.
pub fn resolve_binary() -> PathBuf {
    if let Some(home_bin) = dirs::home_dir().map(|h| h.join(".local/bin/timbits")) {
        if home_bin.is_file() {
            return home_bin;
        }
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("timbits"))
}

/// Convert config-style hotkeys (`Super+Period`, `Ctrl+Shift+V`) to GNOME
/// accelerator form (`<Super>period`, `<Control><Shift>v`).
pub fn to_gnome_binding(s: &str) -> Option<String> {
    let mut mods = Vec::new();
    let mut key: Option<String> = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "super" | "meta" | "win" | "cmd" | "command" => mods.push("<Super>"),
            "ctrl" | "control" => mods.push("<Control>"),
            "shift" => mods.push("<Shift>"),
            "alt" | "option" => mods.push("<Alt>"),
            k => {
                let mapped = match k {
                    "." | "period" => "period",
                    "," | "comma" => "comma",
                    "/" | "slash" => "slash",
                    "\\" | "backslash" => "backslash",
                    "-" | "minus" => "minus",
                    "=" | "equal" => "equal",
                    ";" | "semicolon" => "semicolon",
                    "'" | "quote" => "apostrophe",
                    "`" | "backquote" => "grave",
                    "[" | "bracketleft" => "bracketleft",
                    "]" | "bracketright" => "bracketright",
                    "space" => "space",
                    "enter" | "return" => "Return",
                    "tab" => "Tab",
                    "escape" | "esc" => "Escape",
                    "backspace" => "BackSpace",
                    "delete" => "Delete",
                    "up" => "Up",
                    "down" => "Down",
                    "left" => "Left",
                    "right" => "Right",
                    "home" => "Home",
                    "end" => "End",
                    "pageup" => "Page_Up",
                    "pagedown" => "Page_Down",
                    other if other.len() == 1 => other,
                    other
                        if other.starts_with('f')
                            && other.len() <= 3
                            && other[1..].parse::<u8>().is_ok() =>
                    {
                        // f1..f12 → F1..F12 (GNOME wants capital F).
                        return Some(format!("{}F{}", mods.concat(), &other[1..]));
                    }
                    other => other,
                };
                key = Some(mapped.to_string());
            }
        }
    }
    let key = key?;
    Some(format!("{}{}", mods.concat(), key))
}

fn set_binding(path: &str, name: &str, command: &str, binding: &str) -> Result<()> {
    let schema =
        format!("org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{path}");
    gsettings(&["set", &schema, "name", name])?;
    gsettings(&["set", &schema, "command", command])?;
    gsettings(&["set", &schema, "binding", binding])?;
    Ok(())
}

/// Idempotently register emoji + clipboard GNOME shortcuts from config.
/// Returns `Ok(true)` if shortcuts were applied, `Ok(false)` if gsettings
/// isn't available / not a GNOME session.
pub fn install(cfg: &Config) -> Result<bool> {
    // Probe: if gsettings can't read the list, we're not on GNOME.
    let raw = match gsettings(&["get", SCHEMA_LIST, KEY_LIST]) {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };

    let bin = resolve_binary();
    let emoji_cmd = format!("{} emoji", bin.display());
    let clip_cmd = format!("{} clipboard", bin.display());

    let emoji_binding = to_gnome_binding(&cfg.emoji_hotkey)
        .unwrap_or_else(|| "<Super>period".into());
    let clip_binding = to_gnome_binding(&cfg.clipboard_hotkey)
        .unwrap_or_else(|| "<Super><Shift>c".into());

    let mut paths = parse_path_list(&raw);
    for p in [EMOJI_PATH, CLIPBOARD_PATH] {
        if !paths.iter().any(|x| x == p) {
            paths.push(p.to_string());
        }
    }
    gsettings(&["set", SCHEMA_LIST, KEY_LIST, &format_path_list(&paths)])?;

    set_binding(EMOJI_PATH, "Timbits Emoji", &emoji_cmd, &emoji_binding)?;
    set_binding(
        CLIPBOARD_PATH,
        "Timbits Clipboard",
        &clip_cmd,
        &clip_binding,
    )?;

    println!("GNOME shortcuts:");
    println!("  {emoji_binding} → `{emoji_cmd}`");
    println!("  {clip_binding} → `{clip_cmd}`");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_hotkeys() {
        assert_eq!(
            to_gnome_binding("Super+Period").as_deref(),
            Some("<Super>period")
        );
        assert_eq!(
            to_gnome_binding("Super+Shift+C").as_deref(),
            Some("<Super><Shift>c")
        );
        assert_eq!(
            to_gnome_binding("Ctrl+Shift+Alt+F5").as_deref(),
            Some("<Control><Shift><Alt>F5")
        );
        assert_eq!(
            to_gnome_binding("ctrl + space").as_deref(),
            Some("<Control>space")
        );
        assert!(to_gnome_binding("Super+").is_none());
    }
}
