//! Pure-Rust paste for GNOME Wayland + keyd + ydotoold.
//!
//! Pipeline (configurable order via `Config::paste_methods`):
//!   1. `wl-copy` (+ optional primary)
//!   2. Short focus delay
//!   3. Try each paste method until one succeeds
//!
//! Default methods: ydotool auto chord → uinput auto → ydotool Ctrl+V → uinput Ctrl+V.
//! Do **not** use `ydotool type` for emoji (exits 0, often injects nothing).

use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{self, Config, PasteMethod};

const DEFAULT_YDOTOOL_SOCKET: &str = "/tmp/.ydotool_socket";
const CLAIM_TIMEOUT_MS: u64 = 400;
const CLAIM_POLL_MS: u64 = 20;

// linux/input-event-codes.h
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_V: u16 = 47;
const KEY_LEFTMETA: u16 = 125;

// linux/uinput.h
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_DEV_SETUP: libc::c_ulong = 0x405C_5503;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const BUS_USB: u16 = 0x03;
const UINPUT_MAX_NAME_SIZE: usize = 80;

const TERMINAL_NEEDLES: &[&str] = &[
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
    "hyper",
    "rio",
    "blackbox",
    "console",
    "gnome-terminal",
    "org-gnome-terminal",
    "org-gnome-ptyxis",
    "com-mitchellh-ghostty",
];

/// Snapshotted WM class from keyd-last-focus (call before opening the picker).
pub fn snapshot_focus_class() -> String {
    keyd_last_focus_class()
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

pub fn paste_text(text: &str) -> Result<()> {
    paste_text_with_focus(text, &snapshot_focus_class())
}

pub fn paste_text_with_focus(text: &str, focus_before: &str) -> Result<()> {
    let cfg = Config::load().unwrap_or_default();
    paste_text_with_cfg(text, focus_before, &cfg)
}

fn paste_text_with_cfg(text: &str, focus_before: &str, cfg: &Config) -> Result<()> {
    let t0 = Instant::now();
    log_paste(&format!(
        "paste_text len={} focus_before={focus_before:?} preview={:?}",
        text.len(),
        text.chars().take(12).collect::<String>()
    ));

    if text.is_empty() {
        return Ok(());
    }

    if !claim_clipboard_text(text, cfg) {
        log_paste("FAIL: wl-copy claim");
        return Ok(());
    }
    log_paste(&format!(
        "clipboard claimed in {}ms",
        t0.elapsed().as_millis()
    ));

    let delay = cfg.paste_focus_delay_ms.min(500);
    if delay > 0 {
        thread::sleep(Duration::from_millis(delay));
    }

    let focus_after = keyd_last_focus_class();
    let primary = parse_hotkey(&cfg.paste_hotkey).unwrap_or_else(Chord::ctrl_v);
    let terminal = parse_hotkey(&cfg.paste_hotkey_terminal).unwrap_or_else(Chord::ctrl_shift_v);
    let use_term = cfg.paste_auto_terminal
        && (is_terminal_class(focus_before) || is_terminal_class(&focus_after));
    let auto = if use_term {
        terminal.clone()
    } else {
        primary.clone()
    };

    log_paste(&format!(
        "primary={} terminal={} auto={} focus_after={focus_after:?} methods={}",
        primary.ydotool,
        terminal.ydotool,
        auto.ydotool,
        cfg.paste_methods.len()
    ));

    for method in &cfg.paste_methods {
        let chord = method_chord(*method, &primary, &terminal, &auto);
        let ok = run_method(*method, &chord, cfg);
        if ok {
            log_paste(&format!(
                "ok: {} ({}) total {}ms",
                method.id(),
                chord.ydotool,
                t0.elapsed().as_millis()
            ));
            return Ok(());
        }
        log_paste(&format!("skip: {} ({})", method.id(), chord.ydotool));
    }

    log_paste(&format!(
        "FAIL: all methods failed in {}ms — text is on clipboard; press {}",
        t0.elapsed().as_millis(),
        cfg.paste_hotkey
    ));
    Ok(())
}

fn method_chord(
    method: PasteMethod,
    primary: &Chord,
    terminal: &Chord,
    auto: &Chord,
) -> Chord {
    match method {
        PasteMethod::YdotoolAuto | PasteMethod::UinputAuto => auto.clone(),
        PasteMethod::YdotoolPrimary | PasteMethod::UinputPrimary => primary.clone(),
        PasteMethod::YdotoolTerminal | PasteMethod::UinputTerminal => terminal.clone(),
    }
}

fn run_method(method: PasteMethod, chord: &Chord, cfg: &Config) -> bool {
    if method.uses_ydotool() {
        ydotool_key(&chord.ydotool, cfg)
    } else {
        match inject_uinput_chord(&chord.keys, cfg.paste_uinput_settle_ms) {
            Ok(()) => true,
            Err(e) => {
                log_paste(&format!("uinput failed: {e}"));
                false
            }
        }
    }
}

pub fn paste_staged(kind: &str) -> Result<()> {
    let cfg = Config::load().unwrap_or_default();
    match kind {
        "image" => {
            let path = config::pending_image_path();
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            if !wl_copy_bytes(&bytes, Some("image/png"), false) {
                log_paste("FAIL: image wl-copy");
                return Ok(());
            }
            let delay = cfg.paste_focus_delay_ms.min(500);
            if delay > 0 {
                thread::sleep(Duration::from_millis(delay));
            }
            let focus = keyd_last_focus_class();
            let primary = parse_hotkey(&cfg.paste_hotkey).unwrap_or_else(Chord::ctrl_v);
            let terminal =
                parse_hotkey(&cfg.paste_hotkey_terminal).unwrap_or_else(Chord::ctrl_shift_v);
            let use_term = cfg.paste_auto_terminal && is_terminal_class(&focus);
            let auto = if use_term {
                terminal.clone()
            } else {
                primary.clone()
            };
            for method in &cfg.paste_methods {
                let chord = method_chord(*method, &primary, &terminal, &auto);
                if run_method(*method, &chord, &cfg) {
                    log_paste(&format!("ok image: {}", method.id()));
                    return Ok(());
                }
            }
            log_paste("FAIL: image chord");
            Ok(())
        }
        _ => {
            let text = fs::read_to_string(config::pending_text_path()).unwrap_or_default();
            paste_text(&text)
        }
    }
}

// ── Hotkey parsing ────────────────────────────────────────────────────────

/// A paste chord in both ydotool and uinput forms.
#[derive(Debug, Clone)]
pub struct Chord {
    /// e.g. `ctrl+v`, `super+v`, `ctrl+shift+v`
    pub ydotool: String,
    /// Linux keycodes pressed in order, released reverse.
    pub keys: Vec<u16>,
}

impl Chord {
    fn ctrl_v() -> Self {
        Self {
            ydotool: "ctrl+v".into(),
            keys: vec![KEY_LEFTCTRL, KEY_V],
        }
    }
    fn ctrl_shift_v() -> Self {
        Self {
            ydotool: "ctrl+shift+v".into(),
            keys: vec![KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V],
        }
    }
}

/// Parse user hotkeys like `Ctrl+V`, `Super+V`, `Ctrl+Shift+V`.
pub fn parse_hotkey(s: &str) -> Option<Chord> {
    let mut mods = Vec::new();
    let mut key: Option<u16> = None;
    let mut ydo_parts: Vec<String> = Vec::new();

    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => {
                mods.push(KEY_LEFTCTRL);
                ydo_parts.push("ctrl".into());
            }
            "shift" => {
                mods.push(KEY_LEFTSHIFT);
                ydo_parts.push("shift".into());
            }
            "alt" | "option" => {
                // ydotool uses "alt"; uinput left alt = 56
                mods.push(56);
                ydo_parts.push("alt".into());
            }
            "super" | "meta" | "win" | "cmd" | "command" | "logo" => {
                mods.push(KEY_LEFTMETA);
                ydo_parts.push("super".into());
            }
            "v" => {
                key = Some(KEY_V);
                ydo_parts.push("v".into());
            }
            other if other.len() == 1 => {
                // Map a-z to KEY_A=30 …
                let c = other.chars().next()?;
                if c.is_ascii_lowercase() {
                    let code = 30 + (c as u16 - b'a' as u16);
                    key = Some(code);
                    ydo_parts.push(other.to_string());
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }

    let key = key?;
    let mut keys = mods;
    keys.push(key);
    Some(Chord {
        ydotool: ydo_parts.join("+"),
        keys,
    })
}

// ── Clipboard ─────────────────────────────────────────────────────────────

fn claim_clipboard_text(text: &str, cfg: &Config) -> bool {
    let ok = wl_copy_bytes(text.as_bytes(), None, false);
    let ok_p = if cfg.paste_also_primary {
        wl_copy_bytes(text.as_bytes(), None, true)
    } else {
        false
    };
    if !ok && !ok_p {
        return false;
    }
    if cfg.paste_verify_clipboard {
        let _ = wait_clipboard_text(text);
    }
    true
}

fn wl_copy_bytes(data: &[u8], mime: Option<&str>, primary: bool) -> bool {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    if let Some(m) = mime {
        cmd.arg("--type").arg(m);
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

// ── Focus / terminal ──────────────────────────────────────────────────────

fn keyd_last_focus_class() -> String {
    let uid = nix_uid();
    let path = format!("/run/user/{uid}/keyd-last-focus");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| {
            s.lines()
                .next()
                .map(|l| l.split('\t').next().unwrap_or(l).to_string())
        })
        .unwrap_or_default()
}

fn nix_uid() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1000)
}

fn normalize_class(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

pub fn is_terminal_class(raw: &str) -> bool {
    if raw.is_empty() {
        return false;
    }
    let cls = normalize_class(raw);
    TERMINAL_NEEDLES.iter().any(|n| cls.contains(n))
}

// ── ydotool ───────────────────────────────────────────────────────────────

fn ydotool_socket() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("YDOTOOL_SOCKET").map(PathBuf::from),
        Some(PathBuf::from(DEFAULT_YDOTOOL_SOCKET)),
        std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join(".ydotool_socket")),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

fn ensure_ydotoold() {
    if ydotool_socket().is_some() {
        return;
    }
    let _ = Command::new("systemctl")
        .args(["start", "ydotoold.service"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if ydotool_socket().is_some() {
        return;
    }
    if Path::new("/usr/bin/ydotoold").is_file() {
        let _ = Command::new("/usr/bin/ydotoold")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        for _ in 0..6 {
            if ydotool_socket().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

fn ydotool_key(chord: &str, cfg: &Config) -> bool {
    ensure_ydotoold();
    let bin = which("ydotool").or_else(|| {
        let p = PathBuf::from("/usr/bin/ydotool");
        p.is_file().then_some(p)
    });
    let Some(bin) = bin else {
        return false;
    };
    let mut cmd = Command::new(bin);
    if let Some(sock) = ydotool_socket() {
        cmd.env("YDOTOOL_SOCKET", sock);
    }
    let pre = cfg.paste_ydotool_delay_ms.to_string();
    let stroke = cfg.paste_ydotool_key_delay_ms.to_string();
    cmd.args(["key", "--delay", &pre, "--key-delay", &stroke, chord])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// note: inject_uinput_chord_name removed — callers use inject_uinput_chord directly

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let p = dir.join(bin);
            p.is_file().then_some(p)
        })
    })
}

// ── uinput ────────────────────────────────────────────────────────────────

fn inject_uinput_chord(keys_down: &[u16], settle_ms: u64) -> io::Result<()> {
    if keys_down.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty chord"));
    }
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    if !Path::new("/dev/uinput").exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "/dev/uinput missing"));
    }

    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/uinput")?;
    let fd = file.as_raw_fd();

    unsafe {
        if libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_ulong) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::ioctl(fd, UI_SET_EVBIT, EV_SYN as libc::c_ulong) != 0 {
            return Err(io::Error::last_os_error());
        }
        // Register every key we might press (mods + letter).
        for &key in keys_down {
            if libc::ioctl(fd, UI_SET_KEYBIT, key as libc::c_ulong) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // Also enable common mods so remappers see a full keyboard.
        for &key in &[KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V, KEY_LEFTMETA, 56] {
            let _ = libc::ioctl(fd, UI_SET_KEYBIT, key as libc::c_ulong);
        }

        let setup = pack_uinput_setup("timbits-paste");
        if libc::ioctl(fd, UI_DEV_SETUP, setup.as_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::ioctl(fd, UI_DEV_CREATE) != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    thread::sleep(Duration::from_millis(settle_ms.min(300)));

    let write_ev = |etype: u16, code: u16, value: i32| -> io::Result<()> {
        let bytes = pack_input_event(etype, code, value);
        (&file).write_all(&bytes)?;
        Ok(())
    };

    for &key in keys_down {
        write_ev(EV_KEY, key, 1)?;
    }
    write_ev(EV_SYN, SYN_REPORT, 0)?;
    thread::sleep(Duration::from_millis(12));
    for &key in keys_down.iter().rev() {
        write_ev(EV_KEY, key, 0)?;
    }
    write_ev(EV_SYN, SYN_REPORT, 0)?;
    thread::sleep(Duration::from_millis(20));

    unsafe {
        let _ = libc::ioctl(fd, UI_DEV_DESTROY);
    }
    Ok(())
}

fn pack_input_event(etype: u16, code: u16, value: i32) -> [u8; 24] {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let sec = now.as_secs() as i64;
    let usec = now.subsec_micros() as i64;
    let mut buf = [0u8; 24];
    buf[0..8].copy_from_slice(&sec.to_ne_bytes());
    buf[8..16].copy_from_slice(&usec.to_ne_bytes());
    buf[16..18].copy_from_slice(&etype.to_ne_bytes());
    buf[18..20].copy_from_slice(&code.to_ne_bytes());
    buf[20..24].copy_from_slice(&value.to_ne_bytes());
    buf
}

fn pack_uinput_setup(name: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(92);
    buf.extend_from_slice(&BUS_USB.to_ne_bytes());
    buf.extend_from_slice(&0x0001u16.to_ne_bytes());
    buf.extend_from_slice(&0x0001u16.to_ne_bytes());
    buf.extend_from_slice(&1u16.to_ne_bytes());
    let mut name_bytes = [0u8; UINPUT_MAX_NAME_SIZE];
    let raw = name.as_bytes();
    let n = raw.len().min(UINPUT_MAX_NAME_SIZE - 1);
    name_bytes[..n].copy_from_slice(&raw[..n]);
    buf.extend_from_slice(&name_bytes);
    buf.extend_from_slice(&0u32.to_ne_bytes());
    buf
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
    fn terminal_detection() {
        assert!(is_terminal_class("com.mitchellh.ghostty"));
        assert!(!is_terminal_class("firefox-dev"));
    }

    #[test]
    fn parse_common_hotkeys() {
        let c = parse_hotkey("Ctrl+V").unwrap();
        assert_eq!(c.ydotool, "ctrl+v");
        assert_eq!(c.keys, vec![KEY_LEFTCTRL, KEY_V]);

        let c = parse_hotkey("Super+V").unwrap();
        assert_eq!(c.ydotool, "super+v");
        assert_eq!(c.keys, vec![KEY_LEFTMETA, KEY_V]);

        let c = parse_hotkey("Ctrl+Shift+V").unwrap();
        assert_eq!(c.ydotool, "ctrl+shift+v");
        assert_eq!(c.keys, vec![KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V]);
    }

    #[test]
    fn method_picks_configured_hotkeys() {
        let primary = parse_hotkey("Super+V").unwrap();
        let terminal = parse_hotkey("Ctrl+Shift+V").unwrap();
        let auto = primary.clone();
        assert_eq!(
            method_chord(PasteMethod::YdotoolPrimary, &primary, &terminal, &auto).ydotool,
            "super+v"
        );
        assert_eq!(
            method_chord(PasteMethod::YdotoolTerminal, &primary, &terminal, &auto).ydotool,
            "ctrl+shift+v"
        );
    }
}
