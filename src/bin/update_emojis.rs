//! Dev CLI: regenerate `assets/emojis.json` (Unicode emoji-test + emojilib).
//!
//! Delegates to the main `timbits` binary so the download/parse logic lives in
//! one place (`emoji_update` module).
//!
//! Usage:
//!   cargo build --bin timbits --bin update-emojis
//!   cargo run --bin update-emojis
//!
//! End users: **Settings → Update emoji catalogue** (writes XDG data dir).

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    let timbits = find_timbits_bin()?;
    let root = workspace_root()?;
    let status = Command::new(&timbits)
        .args(["update-emojis", "--assets"])
        .env("CARGO_MANIFEST_DIR", &root)
        .status()
        .with_context(|| format!("spawn {}", timbits.display()))?;
    if !status.success() {
        bail!("timbits update-emojis --assets failed ({status})");
    }
    Ok(())
}

fn find_timbits_bin() -> Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("timbits");
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }
    Ok(PathBuf::from("timbits"))
}

fn workspace_root() -> Result<PathBuf> {
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        return Ok(PathBuf::from(m));
    }
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("assets").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find workspace root");
        }
    }
}
