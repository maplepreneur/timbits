//! Paste the selected item into the previously focused window.
//!
//! Strategy: stage the content, spawn `timbits __serve-clip` (which claims
//! the clipboard and keeps serving it), wait a beat so the previously focused
//! window regains focus and the clipboard claim lands, then synthesize
//! Ctrl+V.
//!
//! Key synthesis:
//!   - Wayland: `wtype` (virtual-keyboard protocol) or `ydotool` (uinput).
//!   - X11: `enigo` (XTEST), falling back to `xdotool`.

use anyhow::Result;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::config;

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

/// Must be called after the picker window has closed.
pub fn paste_staged(kind: &str) -> Result<()> {
    // Let the window close and focus return to the original app.
    thread::sleep(Duration::from_millis(220));

    let exe = std::env::current_exe()?;
    Command::new(exe)
        .arg("__serve-clip")
        .arg(kind)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Give the helper a moment to claim the selection.
    thread::sleep(Duration::from_millis(250));

    simulate_ctrl_v();
    Ok(())
}

fn simulate_ctrl_v() {
    if config::is_wayland() {
        if run_ok(Command::new("wtype").args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])) {
            return;
        }
        // ydotool: key codes 29 = LEFTCTRL, 47 = V (press=1, release=0).
        if run_ok(Command::new("ydotool").args(["key", "29:1", "47:1", "47:0", "29:0"])) {
            return;
        }
        log::warn!(
            "timbits: could not simulate Ctrl+V on Wayland. \
             Install `wtype` or make sure `ydotoold` is running."
        );
    } else {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => {
                let _ = enigo.key(Key::Control, Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                let _ = enigo.key(Key::Control, Direction::Release);
                return;
            }
            Err(e) => log::warn!("enigo init failed: {e}"),
        }
        if run_ok(Command::new("xdotool").args(["key", "ctrl+v"])) {
            return;
        }
        log::warn!("timbits: could not simulate Ctrl+V on X11 (install xdotool).");
    }
}

fn run_ok(cmd: &mut Command) -> bool {
    cmd.status().map(|s| s.success()).unwrap_or(false)
}
