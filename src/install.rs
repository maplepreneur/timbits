//! `timbits install`: write a default config, autostart entry for the daemon,
//! and application launcher entries — then print hotkey setup instructions.

use anyhow::Result;
use std::fs;

use crate::config::{self, Config};

pub fn run() -> Result<()> {
    config::ensure_dirs()?;

    if !config::config_path().exists() {
        Config::default().save()?;
        println!("Wrote default config: {}", config::config_path().display());
    } else {
        println!("Config already exists: {}", config::config_path().display());
    }

    let exe = std::env::current_exe()?;
    let exe = exe.display();

    // Autostart the daemon on login.
    let autostart = config::config_dir()
        .parent()
        .map(|p| p.join("autostart"))
        .unwrap_or_else(|| config::config_dir().join("autostart"));
    fs::create_dir_all(&autostart)?;
    let daemon_entry = desktop_entry(
        "Timbits Daemon",
        "Clipboard history watcher for Timbits",
        &format!("{exe} daemon"),
    );
    fs::write(autostart.join("timbits.desktop"), daemon_entry)?;
    println!("Autostart entry: {}", autostart.join("timbits.desktop").display());

    // Launcher entries (also handy for desktop shortcut pickers).
    let apps = dirs::data_dir()
        .unwrap_or_else(|| config::data_dir())
        .join("applications");
    fs::create_dir_all(&apps)?;
    fs::write(
        apps.join("timbits-emoji.desktop"),
        desktop_entry("Timbits Emoji Picker", "Pick and paste an emoji", &format!("{exe} emoji")),
    )?;
    fs::write(
        apps.join("timbits-clipboard.desktop"),
        desktop_entry(
            "Timbits Clipboard History",
            "Search and paste clipboard history",
            &format!("{exe} clipboard"),
        ),
    )?;
    println!("Launcher entries: {}", apps.display());

    println!(
        "
🍩 Timbits is installed!

Set up hotkeys
--------------
On Wayland (Zorin OS default): Settings → Keyboard → Keyboard Shortcuts →
  Custom Shortcuts → + and add:
    Name: Timbits Emoji       Command: {exe} emoji       Shortcut: Super+.
    Name: Timbits Clipboard   Command: {exe} clipboard   Shortcut: Super+V

On X11 sessions the daemon grabs these hotkeys automatically (edit them in
{}). Make sure the daemon is running (it autostarts on login):
    {exe} daemon &

Optional: for image OCR in history search, install tesseract:
    sudo apt install tesseract-ocr

For pasting on Wayland, `wtype` or a running `ydotoold` is required
(you already have wtype installed — you're set).
",
        config::config_path().display(),
    );
    Ok(())
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
