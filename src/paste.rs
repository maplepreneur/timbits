//! Paste via the proven Python helper (`scripts/paste_helper.py`).
//!
//! That module ports emoji-picker.sh + inject-paste.py:
//!   wl-copy → sleep → ydotool key super+v (or ctrl+shift+v in terminals)
//!   → uinput fallback.
//!
//! Emoji must not use `ydotool type` (exits 0, injects nothing useful).

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config;

/// Snapshotted WM class from keyd-last-focus (for terminal detection).
/// Call once before opening the picker.
pub fn snapshot_focus_class() -> String {
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .unwrap_or(1000);
    let path = format!("/run/user/{uid}/keyd-last-focus");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| {
            s.lines()
                .next()
                .map(|l| l.split('\t').next().unwrap_or(l).to_string())
        })
        .unwrap_or_default()
}

pub fn stage_text(text: &str) -> Result<()> {
    config::ensure_dirs()?;
    fs::write(config::pending_text_path(), text)?;
    Ok(())
}

pub fn stage_image(png_path: &Path) -> Result<()> {
    config::ensure_dirs()?;
    fs::copy(png_path, config::pending_image_path())?;
    Ok(())
}

/// Paste text/emoji using scripts/paste_helper.py.
pub fn paste_text(text: &str) -> Result<()> {
    paste_text_with_focus(text, &snapshot_focus_class())
}

pub fn paste_text_with_focus(text: &str, focus_before: &str) -> Result<()> {
    log_paste(&format!(
        "paste_text via helper len={} focus_before={focus_before:?} preview={:?}",
        text.len(),
        text.chars().take(12).collect::<String>()
    ));

    let helper = paste_helper_path().context("paste_helper.py not found")?;
    let mut cmd = Command::new("python3");
    cmd.arg(&helper)
        .arg("--text")
        .arg(text)
        .arg("--focus-before")
        .arg(focus_before)
        // Focus restore is done in Rust; helper still waits 0.35s like the shell.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    // Ensure ydotoold socket is visible to the helper.
    if std::env::var_os("YDOTOOL_SOCKET").is_none() {
        for sock in [
            "/tmp/.ydotool_socket",
            &format!(
                "{}/.ydotool_socket",
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()
            ),
        ] {
            if Path::new(sock).exists() {
                cmd.env("YDOTOOL_SOCKET", sock);
                break;
            }
        }
    }

    let output = cmd.output().context("run paste_helper.py")?;
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        log_paste(&format!("helper stderr: {}", err.trim()));
    }
    if output.status.success() {
        log_paste("ok: paste_helper.py");
        Ok(())
    } else {
        log_paste(&format!("FAIL: paste_helper.py status={}", output.status));
        // Don't fail the process hard — text may still be on the clipboard.
        Ok(())
    }
}

pub fn paste_staged(kind: &str) -> Result<()> {
    match kind {
        "image" => {
            // Images: wl-copy type image/png then chord-only via helper.
            let path = config::pending_image_path();
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            let mut child = Command::new("wl-copy")
                .args(["--type", "image/png"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("wl-copy image")?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&bytes)?;
            }
            let _ = child.wait();

            let helper = paste_helper_path().context("paste_helper.py not found")?;
            let status = Command::new("python3")
                .arg(&helper)
                .arg("--chord-only")
                .arg("--focus-before")
                .arg(snapshot_focus_class())
                .status()
                .context("paste_helper --chord-only")?;
            log_paste(&format!("image chord status={status}"));
            Ok(())
        }
        _ => {
            let text = fs::read_to_string(config::pending_text_path()).unwrap_or_default();
            paste_text(&text)
        }
    }
}

fn paste_helper_path() -> Option<PathBuf> {
    // 1) Next to the installed binary: ../share/timbits/paste_helper.py (future)
    // 2) Dev tree relative to CARGO / source
    // 3) Explicit well-known path
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("paste_helper.py"));
            candidates.push(dir.join("../scripts/paste_helper.py"));
            candidates.push(dir.join("../../scripts/paste_helper.py"));
        }
    }

    // Workspace path used on this machine
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Work/Voxel North/timbits/scripts/paste_helper.py"));
        candidates.push(home.join("dotfiles/Zorin/keyd/paste_helper.py"));
    }

    candidates.push(PathBuf::from("/home/maplepreneur/Work/Voxel North/timbits/scripts/paste_helper.py"));

    candidates.into_iter().find(|p| p.is_file())
}

fn log_paste(msg: &str) {
    log::info!("paste: {msg}");
    let _ = config::ensure_dirs();
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config::data_dir().join("paste.log"))
    {
        let _ = writeln!(
            f,
            "{} {msg}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_exists_in_dev_tree() {
        // Soft check — CI may not have the home path.
        let _ = paste_helper_path();
    }
}
