//! Clipboard access: reading, hashing, ingesting into storage, and the
//! `__serve-clip` helper which keeps a pasted selection alive after the
//! picker window has closed (on both X11 and Wayland the selection dies with
//! its owner process, so a tiny helper process serves it until replaced).

use anyhow::{Context, Result};
use arboard::Clipboard;
use std::fs;
use std::path::Path;
use std::thread;

use crate::config::{self, Config};
use crate::ocr;
use crate::storage::{EntryKind, Store};

/// Don't store text blobs larger than this.
const MAX_TEXT_BYTES: usize = 1_000_000;

pub enum ClipContent {
    Text(String),
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

/// Read the current clipboard, preferring text over images.
pub fn read_clipboard(cb: &mut Clipboard) -> Option<ClipContent> {
    if let Ok(text) = cb.get_text() {
        if !text.is_empty() {
            return Some(ClipContent::Text(text));
        }
    }
    if let Ok(img) = cb.get_image() {
        return Some(ClipContent::Image {
            width: img.width,
            height: img.height,
            rgba: img.bytes.into_owned(),
        });
    }
    None
}

pub fn content_hash(content: &ClipContent) -> String {
    match content {
        ClipContent::Text(t) => format!("t{:016x}", seahash::hash(t.as_bytes())),
        ClipContent::Image {
            width,
            height,
            rgba,
        } => format!("i{:016x}-{}x{}", seahash::hash(rgba), width, height),
    }
}

fn preview_of(content: &ClipContent) -> String {
    match content {
        ClipContent::Text(t) => {
            let one_line: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            one_line.chars().take(120).collect()
        }
        ClipContent::Image { width, height, .. } => format!("Image {}x{}", width, height),
    }
}

/// Store a clipboard item in history (deduped by content hash). For images,
/// saves a PNG and kicks off background OCR if enabled and available.
pub fn ingest(store: &Store, cfg: &Config, content: ClipContent) -> Result<Option<i64>> {
    let hash = content_hash(&content);
    let preview = preview_of(&content);

    let (kind, text, image_path) = match content {
        ClipContent::Text(t) => {
            if t.len() > MAX_TEXT_BYTES {
                return Ok(None);
            }
            let kind = if t.starts_with("file://") {
                EntryKind::Files
            } else {
                EntryKind::Text
            };
            (kind, Some(t), None)
        }
        ClipContent::Image {
            width,
            height,
            rgba,
        } => {
            let path = config::images_dir().join(format!("{}.png", &hash[1..17]));
            if !path.exists() {
                save_png(&path, width as u32, height as u32, &rgba)?;
            }
            (
                EntryKind::Image,
                None,
                Some(path.to_string_lossy().into_owned()),
            )
        }
    };

    let id = store.upsert(kind, text.as_deref(), image_path.as_deref(), &hash, &preview)?;

    // Background OCR for images so screenshots become searchable.
    if cfg.ocr_enabled {
        if let Some(png) = image_path {
            if ocr::available() {
                let db = config::db_path();
                thread::spawn(move || {
                    if let Some(text) = ocr::ocr_image(Path::new(&png)) {
                        if let Ok(store) = Store::open(&db) {
                            let _ = store.set_ocr(id, &text);
                        }
                    }
                });
            }
        }
    }

    Ok(Some(id))
}

fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .context("invalid image dimensions")?;
    img.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Roundtrip: serve text with `set().wait()` in a background thread
    /// (exactly like `serve_pending`), then read it back. Uses X11 (XWayland)
    /// because GNOME Wayland forbids unfocused clipboard reads.
    #[test]
    fn serve_text_roundtrip() {
        if std::env::var("DISPLAY").is_err() {
            return; // no X11 available
        }
        // SAFETY: single-purpose test binary; no other test reads this var.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };

        use arboard::SetExtLinux;
        let unique = format!("timbits-serve-test-{}", std::process::id());
        let serve = unique.clone();
        let server = std::thread::spawn(move || {
            let mut cb = arboard::Clipboard::new().unwrap();
            cb.set().wait().text(serve).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(700));

        let mut cb = arboard::Clipboard::new().unwrap();
        let got = cb.get_text().unwrap();
        assert_eq!(got, unique);

        // Overwrite the selection so the server's `wait()` returns.
        cb.set_text("timbits-test-release".to_string()).unwrap();
        server.join().unwrap();
    }
}

/// Block, serving the staged clipboard content until another client takes
/// over the selection. Runs as the hidden `timbits __serve-clip <kind>`
/// subcommand so the selection survives the picker process exiting.
pub fn serve_pending(kind: &str) -> Result<()> {
    use arboard::SetExtLinux;

    let mut cb = Clipboard::new().context("cannot access clipboard")?;
    if kind == "image" {
        let path = config::pending_image_path();
        let img = image::open(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        let data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: img.into_raw().into(),
        };
        cb.set().wait().image(data)?;
    } else {
        let text = fs::read_to_string(config::pending_text_path())
            .context("reading staged text")?;
        cb.set().wait().text(text)?;
    }
    Ok(())
}
