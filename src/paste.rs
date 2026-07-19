//! Paste into the previously focused window.
//!
//! ## Proven path (user's emoji-picker.sh / inject-paste.py)
//!
//! 1. Put content on the clipboard with `wl-copy` (and `--primary`).
//! 2. Sleep ~350 ms so focus returns after the picker closes.
//! 3. Inject paste with **`ydotool key --delay 100 --key-delay 12 super+v`**
//!    (or `ctrl+shift+v` in terminals). keyd remaps Super+V → real paste.
//! 4. Fall back to `inject-paste.py` (raw uinput Super+V / Ctrl+V).
//!
//! Do **not** use `ydotool type` for emoji — it exits 0 but injects nothing
//! useful for multi-byte Unicode. Handy Direct typing is ASCII-only STT.

use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config;

/// Match emoji-picker.sh: sleep after picker before paste chord.
const FOCUS_RETURN_MS: u64 = 350;

/// Match emoji-picker.sh ydotool flags.
const YDO_KEY_DELAY_MS: &str = "100";
const YDO_KEY_STROKE_MS: &str = "12";

const CLAIM_TIMEOUT_MS: u64 = 2000;
const CLAIM_POLL_MS: u64 = 40;

const INJECT_PASTE: &str = "/home/maplepreneur/dotfiles/Zorin/keyd/inject-paste.py";
const KEYD_LAST_FOCUS: &str = "/run/user/1000/keyd-last-focus";

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

/// Paste text/emoji the same way emoji-picker.sh does after GTK selection.
pub fn paste_text(text: &str) -> Result<()> {
    log_paste(&format!(
        "paste_text len={} preview={:?}",
        text.len(),
        text.chars().take(12).collect::<String>()
    ));

    // Optional: caller may already have restored focus; still wait like the shell script.
    thread::sleep(Duration::from_millis(FOCUS_RETURN_MS));

    if !claim_clipboard_text(text) {
        log_paste("FAIL: wl-copy could not claim clipboard");
        return Ok(());
    }

    let chord = paste_chord_for_focus();
    log_paste(&format!("using chord={chord}"));

    if ydotool_paste_chord(chord) {
        log_paste(&format!("ok: ydotool key {chord} (emoji-picker.sh path)"));
        return Ok(());
    }
    log_paste("ydotool key failed; trying inject-paste.py");

    if inject_paste_script(chord) {
        log_paste("ok: inject-paste.py");
        return Ok(());
    }

    log_paste("FAIL: all paste chords failed — content is on clipboard, press Super+V");
    Ok(())
}

pub fn paste_staged(kind: &str) -> Result<()> {
    match kind {
        "image" => {
            thread::sleep(Duration::from_millis(FOCUS_RETURN_MS));
            if !claim_clipboard_image(&config::pending_image_path()) {
                log_paste("FAIL: image clipboard claim");
                return Ok(());
            }
            let chord = paste_chord_for_focus();
            if ydotool_paste_chord(chord) || inject_paste_script(chord) {
                log_paste("ok: image clipboard + chord");
            } else {
                log_paste("FAIL: image paste chord");
            }
            Ok(())
        }
        _ => {
            let text = fs::read_to_string(config::pending_text_path()).unwrap_or_default();
            paste_text(&text)
        }
    }
}

// ── Clipboard (emoji-picker.py style) ─────────────────────────────────────

fn claim_clipboard_text(text: &str) -> bool {
    // Primary + clipboard, same as emoji-picker.py select_and_exit.
    let ok_clip = wl_copy(text.as_bytes(), false);
    let ok_pri = wl_copy(text.as_bytes(), true);
    if !ok_clip && !ok_pri {
        return false;
    }
    // Best-effort verify (GNOME sometimes slow to advertise).
    let _ = wait_clipboard_text(text);
    true
}

fn claim_clipboard_image(png_path: &Path) -> bool {
    let Ok(bytes) = fs::read(png_path) else {
        return false;
    };
    let mut cmd = Command::new("wl-copy");
    cmd.arg("--type").arg("image/png");
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(&bytes).is_err() {
            return false;
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn wl_copy(data: &[u8], primary: bool) -> bool {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log_paste(&format!("wl-copy spawn: {e}"));
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(data).is_err() {
            return false;
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn wait_clipboard_text(expected: &str) -> bool {
    let deadline = Instant::now() + Duration::from_millis(CLAIM_TIMEOUT_MS);
    while Instant::now() < deadline {
        if let Ok(output) = Command::new("wl-paste").arg("--no-newline").output() {
            if output.status.success() {
                let got = String::from_utf8_lossy(&output.stdout);
                if got == expected || got.trim_end_matches('\n') == expected.trim_end_matches('\n')
                {
                    return true;
                }
            }
        }
        thread::sleep(Duration::from_millis(CLAIM_POLL_MS));
    }
    false
}

// ── Terminal detection (emoji-picker.sh) ──────────────────────────────────

fn paste_chord_for_focus() -> &'static str {
    let cls = read_last_focus_class();
    if is_terminal_class(&cls) {
        "ctrl+shift+v"
    } else {
        "super+v"
    }
}

fn read_last_focus_class() -> String {
    // Prefer live path for current user.
    let uid = users_uid();
    let path = format!("/run/user/{uid}/keyd-last-focus");
    let p = if Path::new(&path).is_file() {
        path
    } else {
        KEYD_LAST_FOCUS.to_string()
    };
    fs::read_to_string(p)
        .ok()
        .and_then(|s| s.lines().next().map(|l| l.split('\t').next().unwrap_or(l).to_string()))
        .unwrap_or_default()
}

fn users_uid() -> u32 {
    // nix-free: parse id -u
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1000)
}

fn is_terminal_class(raw: &str) -> bool {
    if raw.is_empty() {
        return false;
    }
    let cls: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    const NEEDLES: &[&str] = &[
        "terminal",
        "kitty",
        "alacritty",
        "wezterm",
        "foot",
        "ghostty",
        "xterm",
        "konsole",
        "terminator",
        "ptyxis",
        "warp",
        "tilix",
        "guake",
        "tilda",
        "console",
        "com-mitchellh-ghostty",
        "org-gnome-terminal",
        "org-gnome-ptyxis",
    ];
    NEEDLES.iter().any(|n| cls.contains(n))
}

// ── Chords ────────────────────────────────────────────────────────────────

/// Exactly emoji-picker.sh: `ydotool key --delay 100 --key-delay 12 super+v`
fn ydotool_paste_chord(chord: &str) -> bool {
    if !ydotool_ready() {
        log_paste("ydotool not ready");
        return false;
    }
    let ok = run_ok(
        ydotool_cmd()
            .args([
                "key",
                "--delay",
                YDO_KEY_DELAY_MS,
                "--key-delay",
                YDO_KEY_STROKE_MS,
                chord,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    );
    if !ok {
        log_paste(&format!("ydotool key {chord} failed"));
    }
    ok
}

fn inject_paste_script(chord: &str) -> bool {
    if !Path::new(INJECT_PASTE).is_file() {
        return false;
    }
    let mut cmd = Command::new("python3");
    cmd.arg(INJECT_PASTE);
    match chord {
        "ctrl+shift+v" => {
            cmd.arg("--terminal");
        }
        "ctrl+v" => {
            cmd.arg("--ctrl");
        }
        _ => {
            // default Super+V
        }
    }
    run_ok(
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
}

fn ydotool_cmd() -> Command {
    let mut cmd = Command::new("ydotool");
    if std::env::var_os("YDOTOOL_SOCKET").is_none() {
        if let Some(sock) = ydotool_socket() {
            cmd.env("YDOTOOL_SOCKET", sock);
        }
    }
    cmd
}

fn ydotool_socket() -> Option<String> {
    for c in [
        std::env::var("YDOTOOL_SOCKET").ok(),
        Some("/tmp/.ydotool_socket".into()),
        std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .map(|d| format!("{d}/.ydotool_socket")),
    ]
    .into_iter()
    .flatten()
    {
        if Path::new(&c).exists() {
            return Some(c);
        }
    }
    None
}

fn ydotool_ready() -> bool {
    which("ydotool").is_some() && ydotool_socket().is_some()
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let p = dir.join(bin);
            p.is_file().then_some(p)
        })
    })
}

fn run_ok(cmd: &mut Command) -> bool {
    cmd.status().map(|s| s.success()).unwrap_or(false)
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
    fn detects_terminals() {
        assert!(is_terminal_class("com.mitchellh.ghostty"));
        assert!(is_terminal_class("gnome-terminal-server"));
        assert!(!is_terminal_class("firefox-dev"));
    }
}
