//! `timbits install`: write a default config, autostart entry for the daemon,
//! application launcher entries, and (on GNOME/Zorin) register hotkeys.

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::config::{self, Config};
use crate::gnome_hotkeys;

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

    // Autostart the daemon on login.
    let autostart = xdg_autostart_dir();
    fs::create_dir_all(&autostart)?;
    let daemon_entry = desktop_entry(
        "Timbits Daemon",
        "Clipboard history watcher for Timbits",
        &format!("{exe_s} daemon"),
    );
    fs::write(autostart.join("timbits.desktop"), daemon_entry)?;
    println!(
        "Autostart entry: {}",
        autostart.join("timbits.desktop").display()
    );

    // Launcher entries (also handy for desktop shortcut pickers).
    let apps = dirs::data_dir()
        .unwrap_or_else(|| config::data_dir())
        .join("applications");
    fs::create_dir_all(&apps)?;
    fs::write(
        apps.join("timbits-emoji.desktop"),
        desktop_entry(
            "Timbits Emoji Picker",
            "Pick and paste an emoji",
            &format!("{exe_s} emoji"),
        ),
    )?;
    fs::write(
        apps.join("timbits-clipboard.desktop"),
        desktop_entry(
            "Timbits Clipboard History",
            "Search and paste clipboard history",
            &format!("{exe_s} clipboard"),
        ),
    )?;
    println!("Launcher entries: {}", apps.display());

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
    Name: Timbits Clipboard   Command: {exe_s} clipboard   Shortcut: Super+V

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

fn xdg_autostart_dir() -> PathBuf {
    config::config_dir()
        .parent()
        .map(|p| p.join("autostart"))
        .unwrap_or_else(|| config::config_dir().join("autostart"))
}

fn desktop_entry(name: &str, comment: &str, exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={comment}\n\
         Exec={exec}\n\
         Icon=face-smile\n\
         Terminal=false\n\
         Categories=Utility;\n"
    )
}
