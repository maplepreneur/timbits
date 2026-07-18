//! Background daemon: watches the clipboard and (on X11) registers global
//! hotkeys that launch the pickers.
//!
//! Wayland notes: there is no display-server-agnostic global hotkey API, so
//! on Wayland you should bind `timbits emoji` / `timbits clipboard` as custom
//! shortcuts in your desktop environment's settings. Also, GNOME Wayland only
//! lets the *focused* client read the clipboard, so the watcher can't see
//! copies made while unfocused there — the history picker therefore also
//! ingests the clipboard every time it opens (it is focused then). On
//! wlroots/KDE Wayland the watcher works via the data-control protocol.

use anyhow::Result;
use arboard::Clipboard;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::clip;
use crate::config::{self, Config};
use crate::storage::Store;

const POLL_INTERVAL: Duration = Duration::from_millis(700);

pub fn run() -> Result<()> {
    config::ensure_dirs()?;
    let cfg = Config::load()?;
    let store = Store::open(&config::db_path())?;
    let mut cb = Clipboard::new()?;

    let hotkeys = setup_hotkeys(&cfg);
    if hotkeys.is_some() {
        log::info!(
            "global hotkeys registered: emoji={} clipboard={}",
            cfg.emoji_hotkey,
            cfg.clipboard_hotkey
        );
    } else if config::is_wayland() {
        log::info!(
            "Wayland session: bind `timbits emoji` and `timbits clipboard` as custom \
             keyboard shortcuts in your desktop settings (global hotkey grabbing is \
             only supported on X11)."
        );
    } else {
        log::warn!("could not register global hotkeys");
    }
    log::info!("timbits daemon started (watching clipboard)");

    let mut last_hash: Option<String> = None;
    loop {
        drain_hotkey_events(&hotkeys);

        if let Some(content) = clip::read_clipboard(&mut cb) {
            let hash = clip::content_hash(&content);
            if last_hash.as_deref() != Some(hash.as_str()) {
                last_hash = Some(hash);
                if let Err(e) = clip::ingest(&store, &cfg, content) {
                    log::warn!("failed to store clipboard item: {e:#}");
                }
                match store.trim(cfg.max_entries) {
                    Ok(paths) => {
                        for p in paths {
                            std::fs::remove_file(p).ok();
                        }
                    }
                    Err(e) => log::warn!("trim failed: {e:#}"),
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

struct HotkeyState {
    _manager: global_hotkey::GlobalHotKeyManager,
    emoji_id: u32,
    clipboard_id: u32,
}

fn setup_hotkeys(cfg: &Config) -> Option<HotkeyState> {
    use global_hotkey::GlobalHotKeyManager;

    if config::is_wayland() {
        return None;
    }
    let manager = GlobalHotKeyManager::new().ok()?;
    let emoji = parse_hotkey(&cfg.emoji_hotkey)?;
    let clipboard = parse_hotkey(&cfg.clipboard_hotkey)?;
    if let Err(e) = manager.register(emoji) {
        log::warn!("could not register emoji hotkey: {e}");
    }
    if let Err(e) = manager.register(clipboard) {
        log::warn!("could not register clipboard hotkey: {e}");
    }
    Some(HotkeyState {
        _manager: manager,
        emoji_id: emoji.id(),
        clipboard_id: clipboard.id(),
    })
}

fn drain_hotkey_events(hotkeys: &Option<HotkeyState>) {
    use global_hotkey::GlobalHotKeyEvent;

    let Some(hk) = hotkeys else { return };
    while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
        let which = if event.id == hk.emoji_id {
            Some("emoji")
        } else if event.id == hk.clipboard_id {
            Some("clipboard")
        } else {
            None
        };
        if let Some(which) = which {
            match std::env::current_exe() {
                Ok(exe) => {
                    if let Err(e) = Command::new(exe).arg(which).spawn() {
                        log::warn!("failed to launch picker: {e}");
                    }
                }
                Err(e) => log::warn!("current_exe failed: {e}"),
            }
        }
    }
}

/// Parse a human hotkey string like "Super+Shift+Period" or "Ctrl+Alt+V".
pub fn parse_hotkey(s: &str) -> Option<global_hotkey::hotkey::HotKey> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};

    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "super" | "meta" | "win" | "cmd" | "command" => mods |= Modifiers::SUPER,
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            key => code = Some(parse_code(key)?),
        }
    }
    Some(HotKey::new(Some(mods), code?))
}

fn parse_code(key: &str) -> Option<global_hotkey::hotkey::Code> {
    use global_hotkey::hotkey::Code;

    Some(match key {
        "a" => Code::KeyA, "b" => Code::KeyB, "c" => Code::KeyC, "d" => Code::KeyD,
        "e" => Code::KeyE, "f" => Code::KeyF, "g" => Code::KeyG, "h" => Code::KeyH,
        "i" => Code::KeyI, "j" => Code::KeyJ, "k" => Code::KeyK, "l" => Code::KeyL,
        "m" => Code::KeyM, "n" => Code::KeyN, "o" => Code::KeyO, "p" => Code::KeyP,
        "q" => Code::KeyQ, "r" => Code::KeyR, "s" => Code::KeyS, "t" => Code::KeyT,
        "u" => Code::KeyU, "v" => Code::KeyV, "w" => Code::KeyW, "x" => Code::KeyX,
        "y" => Code::KeyY, "z" => Code::KeyZ,
        "0" => Code::Digit0, "1" => Code::Digit1, "2" => Code::Digit2,
        "3" => Code::Digit3, "4" => Code::Digit4, "5" => Code::Digit5,
        "6" => Code::Digit6, "7" => Code::Digit7, "8" => Code::Digit8,
        "9" => Code::Digit9,
        "f1" => Code::F1, "f2" => Code::F2, "f3" => Code::F3, "f4" => Code::F4,
        "f5" => Code::F5, "f6" => Code::F6, "f7" => Code::F7, "f8" => Code::F8,
        "f9" => Code::F9, "f10" => Code::F10, "f11" => Code::F11, "f12" => Code::F12,
        "space" => Code::Space,
        "enter" | "return" => Code::Enter,
        "tab" => Code::Tab,
        "escape" | "esc" => Code::Escape,
        "backspace" => Code::Backspace,
        "delete" => Code::Delete,
        "period" | "." => Code::Period,
        "comma" | "," => Code::Comma,
        "slash" | "/" => Code::Slash,
        "backslash" | "\\" => Code::Backslash,
        "minus" | "-" => Code::Minus,
        "equal" | "=" => Code::Equal,
        "semicolon" | ";" => Code::Semicolon,
        "quote" | "'" => Code::Quote,
        "backquote" | "`" => Code::Backquote,
        "bracketleft" | "[" => Code::BracketLeft,
        "bracketright" | "]" => Code::BracketRight,
        "up" => Code::ArrowUp,
        "down" => Code::ArrowDown,
        "left" => Code::ArrowLeft,
        "right" => Code::ArrowRight,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_hotkeys() {
        assert!(super::parse_hotkey("Super+Period").is_some());
        assert!(super::parse_hotkey("Super+Shift+C").is_some());
        assert!(super::parse_hotkey("Ctrl+Shift+Alt+F5").is_some());
        assert!(super::parse_hotkey("ctrl + space").is_some());
        assert!(super::parse_hotkey("NotARealKey").is_none());
        assert!(super::parse_hotkey("Super+").is_none());
    }
}
