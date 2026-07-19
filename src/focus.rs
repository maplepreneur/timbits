//! Restore the previously focused window after the picker closes.
//!
//! Uses the same `WindowsExt` GNOME Shell extension as `focus-or-launch.py`.

use serde::Deserialize;
use std::io::Write;
use std::process::Command;

use crate::config;

/// Window id from WindowsExt (GNOME Shell extension).
#[derive(Debug, Clone, Copy)]
pub struct FocusedWindow {
    pub id: u64,
}

#[derive(Debug, Deserialize)]
struct WinInfo {
    id: u64,
    class: Option<String>,
    #[serde(default)]
    focus: bool,
}

/// Snapshot the currently focused window (call *before* the picker maps).
pub fn capture_focused() -> Option<FocusedWindow> {
    let wins = match list_windows() {
        Some(w) => w,
        None => {
            log_focus("capture FAILED: WindowsExt.List unavailable");
            return None;
        }
    };

    let hit = wins
        .into_iter()
        .find(|w| w.focus && !is_timbits_class(w.class.as_deref()));

    match hit {
        Some(w) => {
            log_focus(&format!(
                "capture ok id={} class={:?}",
                w.id, w.class
            ));
            Some(FocusedWindow { id: w.id })
        }
        None => {
            log_focus("capture FAILED: no non-timbits focused window");
            None
        }
    }
}

/// Activate a previously captured window. Retries until it reports focus.
pub fn restore(prev: Option<FocusedWindow>) -> bool {
    let Some(prev) = prev else {
        log_focus("restore SKIP: nothing captured");
        return false;
    };

    for attempt in 1..=4 {
        if let Err(e) = activate(prev.id) {
            log_focus(&format!("restore Activate attempt {attempt} err: {e}"));
            std::thread::sleep(std::time::Duration::from_millis(80));
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(100 + attempt * 40));

        if window_has_focus(prev.id) {
            log_focus(&format!(
                "restore ok id={} after attempt {attempt}",
                prev.id
            ));
            // Extra settle so the text widget actually has keyboard focus.
            std::thread::sleep(std::time::Duration::from_millis(120));
            return true;
        }
        log_focus(&format!(
            "restore attempt {attempt}: Activate ok but focus not on id={}",
            prev.id
        ));
    }

    log_focus(&format!("restore FAILED for id={}", prev.id));
    false
}

fn window_has_focus(id: u64) -> bool {
    list_windows()
        .map(|wins| wins.iter().any(|w| w.id == id && w.focus))
        .unwrap_or(false)
}

fn is_timbits_class(class: Option<&str>) -> bool {
    class
        .map(|c| c.to_lowercase().contains("timbits"))
        .unwrap_or(false)
}

fn list_windows() -> Option<Vec<WinInfo>> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell/Extensions/WindowsExt",
            "--method",
            "org.gnome.Shell.Extensions.WindowsExt.List",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    let json = raw[start..=end]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    serde_json::from_str(&json).ok()
}

fn activate(id: u64) -> Result<(), String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell/Extensions/WindowsExt",
            "--method",
            "org.gnome.Shell.Extensions.WindowsExt.Activate",
            &id.to_string(),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn log_focus(msg: &str) {
    log::info!("focus: {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config::data_dir().join("paste.log"))
    {
        let _ = writeln!(
            f,
            "{} focus: {msg}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
    }
}
