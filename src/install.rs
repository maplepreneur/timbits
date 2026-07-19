//! `timbits install`: write a default config, autostart entry for the daemon,
//! application launcher entries, desktop icon, and (on GNOME/Zorin) hotkeys.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{self, Config};
use crate::gnome_hotkeys;
use crate::ui_common;

/// Icon theme sizes to install under hicolor.
const ICON_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

pub fn run() -> Result<()> {
    config::ensure_dirs()?;

    if !config::config_path().exists() {
        Config::default().save()?;
        println!("Wrote default config: {}", config::config_path().display());
    } else {
        println!("Config already exists: {}", config::config_path().display());
    }

    let cfg = Config::load().unwrap_or_default();
    let exe = gnome_hotkeys::resolve_binary();
    let exe_s = exe.display().to_string();

    // Desktop/app icons (hicolor theme → Icon=timbits).
    let icon_path = install_icons().context("installing app icons")?;
    println!("App icon: {}", icon_path.display());

    // Autostart the daemon on login.
    let autostart = xdg_autostart_dir();
    fs::create_dir_all(&autostart)?;
    let daemon_entry = desktop_entry(
        "Timbits Daemon",
        "Clipboard history watcher for Timbits",
        &format!("{exe_s} daemon"),
        false,
    );
    fs::write(autostart.join("timbits.desktop"), daemon_entry)?;
    println!(
        "Autostart entry: {}",
        autostart.join("timbits.desktop").display()
    );

    // Single app-menu launcher → settings (emoji/clipboard are hotkey-driven).
    let apps = dirs::data_dir()
        .unwrap_or_else(|| config::data_dir())
        .join("applications");
    fs::create_dir_all(&apps)?;
    // Drop legacy split launchers from earlier installs.
    for legacy in ["timbits-emoji.desktop", "timbits-clipboard.desktop"] {
        let p = apps.join(legacy);
        if p.exists() {
            let _ = fs::remove_file(&p);
            println!("Removed legacy launcher: {}", p.display());
        }
    }
    fs::write(
        apps.join("timbits.desktop"),
        desktop_entry(
            "Timbits",
            "Emoji picker, clipboard history, and settings",
            &format!("{exe_s} settings"),
            true,
        ),
    )?;
    println!("Launcher entry: {}", apps.join("timbits.desktop").display());

    // Refresh icon / desktop caches when tools are available.
    let _ = Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(icons_root())
        .status();
    let _ = Command::new("update-desktop-database")
        .arg(&apps)
        .status();

    // GNOME/Zorin: wire hotkeys automatically when gsettings is available.
    let gnome_ok = match gnome_hotkeys::install(&cfg) {
        Ok(true) => true,
        Ok(false) => {
            println!("(GNOME custom keybindings not available — skipping auto hotkeys)");
            false
        }
        Err(e) => {
            println!("Warning: could not register GNOME hotkeys: {e:#}");
            false
        }
    };

    println!(
        "
🍩 Timbits is installed!

Binary: {exe_s}
Config: {}
",
        config::config_path().display(),
    );

    if gnome_ok {
        println!(
            "Hotkeys (GNOME/Zorin) were registered from your config.
  Edit {} and re-run `timbits install` to change them.
",
            config::config_path().display()
        );
    } else {
        println!(
            "Set up hotkeys
--------------
On Wayland: Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → +
    Name: Timbits Emoji       Command: {exe_s} emoji       Shortcut: Super+.
    Name: Timbits Clipboard   Command: {exe_s} clipboard   Shortcut: Super+Shift+C

On X11 sessions the daemon grabs these hotkeys automatically (edit them in
{}). Make sure the daemon is running (it autostarts on login):
    {exe_s} daemon &
",
            config::config_path().display()
        );
    }

    println!(
        "Optional: for image OCR in history search, install tesseract:
    sudo apt install tesseract-ocr

For pasting on Wayland, `wtype` or a running `ydotoold` is required.
"
    );
    Ok(())
}

fn icons_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| config::data_dir())
        .join("icons")
        .join("hicolor")
}

/// Decode the bundled logo and write sized PNGs into the user hicolor theme.
/// Returns the path of the largest installed icon (useful for absolute Icon=).
fn install_icons() -> Result<PathBuf> {
    let root = icons_root();
    let rgba = image::load_from_memory(ui_common::LOGO_PNG)
        .context("decoding bundled logo.png")?
        .to_rgba8();
    let (src_w, src_h) = rgba.dimensions();

    let mut largest = root.join("512x512/apps/timbits.png");

    for &size in ICON_SIZES {
        let dir = root.join(format!("{size}x{size}")).join("apps");
        fs::create_dir_all(&dir)?;
        let dest = dir.join("timbits.png");
        if size == src_w && size == src_h {
            rgba.save(&dest)?;
        } else {
            let resized = image::imageops::resize(
                &rgba,
                size,
                size,
                image::imageops::FilterType::Lanczos3,
            );
            resized.save(&dest)?;
        }
        if size == *ICON_SIZES.last().unwrap() {
            largest = dest;
        }
    }

    // Also drop a scalable-friendly copy under pixmaps for older menus.
    let pixmaps = dirs::data_dir()
        .unwrap_or_else(|| config::data_dir())
        .join("pixmaps");
    fs::create_dir_all(&pixmaps)?;
    fs::copy(
        root.join("256x256/apps/timbits.png"),
        pixmaps.join("timbits.png"),
    )
    .ok();

    Ok(largest)
}

fn xdg_autostart_dir() -> PathBuf {
    config::config_dir()
        .parent()
        .map(|p| p.join("autostart"))
        .unwrap_or_else(|| config::config_dir().join("autostart"))
}

fn desktop_entry(name: &str, comment: &str, exec: &str, show_in_menu: bool) -> String {
    let no_display = if show_in_menu { "false" } else { "true" };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={comment}\n\
         Exec={exec}\n\
         Icon=timbits\n\
         Terminal=false\n\
         Categories=Utility;Accessibility;\n\
         StartupWMClass=timbits\n\
         NoDisplay={no_display}\n"
    )
}
