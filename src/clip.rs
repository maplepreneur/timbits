//! Clipboard access: reading, hashing, ingesting into storage, and the
//! `__serve-clip` helper which keeps a pasted selection alive after the
//! picker window has closed (on both X11 and Wayland the selection dies with
//! its owner process, so a tiny helper process serves it until replaced).

use anyhow::{Context, Result};
use arboard::Clipboard;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use crate::config::{self, Config};
use crate::ocr;
use crate::storage::{EntryKind, Store};

/// Don't store text blobs larger than this.
const MAX_TEXT_BYTES: usize = 1_000_000;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];

pub enum ClipContent {
    Text(String),
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
    /// Paths from a file-manager copy (`text/uri-list`).
    Files(Vec<PathBuf>),
}

/// Read the current clipboard. Prefer file lists (Nautilus/etc.), then text,
/// then raw image pixels (screenshots).
pub fn read_clipboard(cb: &mut Clipboard) -> Option<ClipContent> {
    if let Ok(paths) = cb.get().file_list() {
        if !paths.is_empty() {
            return Some(ClipContent::Files(paths));
        }
    }
    if let Ok(text) = cb.get_text() {
        if !text.is_empty() {
            // Some apps paste multi-line file:// lists as plain text.
            if let Some(paths) = parse_file_uri_text(&text) {
                return Some(ClipContent::Files(paths));
            }
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

/// Parse `file://…` URI lines into absolute paths. Returns `None` if the
/// text doesn't look like a file list.
fn parse_file_uri_text(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("file://") {
            // percent-decode minimal cases; paths are usually already clean.
            let decoded = percent_decode(rest);
            paths.push(PathBuf::from(decoded));
        } else if line.starts_with('/') && Path::new(line).exists() {
            paths.push(PathBuf::from(line));
        } else {
            // Not a pure file list.
            return None;
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
        .unwrap_or(false)
}

pub fn content_hash(content: &ClipContent) -> String {
    match content {
        ClipContent::Text(t) => format!("t{:016x}", seahash::hash(t.as_bytes())),
        ClipContent::Image {
            width,
            height,
            rgba,
        } => format!("i{:016x}-{}x{}", seahash::hash(rgba), width, height),
        ClipContent::Files(paths) => {
            let joined = paths
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n");
            format!("f{:016x}", seahash::hash(joined.as_bytes()))
        }
    }
}

fn preview_of(content: &ClipContent) -> String {
    match content {
        ClipContent::Text(t) => {
            let one_line: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            one_line.chars().take(120).collect()
        }
        ClipContent::Image { width, height, .. } => format!("Image {}x{}", width, height),
        ClipContent::Files(paths) => {
            if paths.len() == 1 {
                paths[0]
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| paths[0].display().to_string())
            } else {
                format!(
                    "{} files: {}",
                    paths.len(),
                    paths
                        .iter()
                        .take(3)
                        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

fn paths_as_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Store a clipboard item in history (deduped by content hash). For images
/// (screenshots or image files), saves a PNG and kicks off background OCR.
pub fn ingest(store: &Store, cfg: &Config, content: ClipContent) -> Result<Option<i64>> {
    let hash = content_hash(&content);
    let preview = preview_of(&content);

    // Image paths we should OCR after upsert (screenshot PNG or copied image files).
    let mut ocr_sources: Vec<PathBuf> = Vec::new();

    let (kind, text, image_path) = match content {
        ClipContent::Text(t) => {
            if t.len() > MAX_TEXT_BYTES {
                return Ok(None);
            }
            let kind = if t.lines().any(|l| l.trim().starts_with("file://")) {
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
            ocr_sources.push(path.clone());
            (
                EntryKind::Image,
                None,
                Some(path.to_string_lossy().into_owned()),
            )
        }
        ClipContent::Files(paths) => {
            let text = paths_as_text(&paths);
            if text.len() > MAX_TEXT_BYTES {
                return Ok(None);
            }

            // Single image file → treat as Image with a stored PNG copy for preview.
            if paths.len() == 1 && is_image_path(&paths[0]) && paths[0].is_file() {
                match materialize_image_file(&paths[0], &hash) {
                    Ok(stored) => {
                        ocr_sources.push(stored.clone());
                        (
                            EntryKind::Image,
                            Some(text),
                            Some(stored.to_string_lossy().into_owned()),
                        )
                    }
                    Err(e) => {
                        log::warn!("could not store image file {}: {e:#}", paths[0].display());
                        // Fall through to files entry + OCR source path.
                        ocr_sources.push(paths[0].clone());
                        (EntryKind::Files, Some(text), None)
                    }
                }
            } else {
                // Multi-file or non-image: store as Files; OCR any image files for search.
                for p in &paths {
                    if is_image_path(p) && p.is_file() {
                        ocr_sources.push(p.clone());
                    }
                }
                (EntryKind::Files, Some(text), None)
            }
        }
    };

    let id = store.upsert(kind, text.as_deref(), image_path.as_deref(), &hash, &preview)?;

    spawn_ocr(cfg, id, ocr_sources);

    Ok(Some(id))
}

/// Copy / re-encode an image file into the images dir for preview + OCR.
fn materialize_image_file(src: &Path, hash: &str) -> Result<PathBuf> {
    let dest = config::images_dir().join(format!("{}.png", &hash[1..17]));
    if dest.exists() {
        return Ok(dest);
    }
    // Re-encode via the image crate so we always store PNG (handles jpg etc.).
    let img = image::open(src)
        .with_context(|| format!("opening {}", src.display()))?
        .to_rgba8();
    img.save(&dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(dest)
}

fn spawn_ocr(cfg: &Config, id: i64, sources: Vec<PathBuf>) {
    if !cfg.ocr_enabled || sources.is_empty() || !ocr::available() {
        return;
    }
    let db = config::db_path();
    thread::spawn(move || {
        let mut parts = Vec::new();
        for path in sources {
            if let Some(text) = ocr::ocr_image(&path) {
                parts.push(text);
            }
        }
        if parts.is_empty() {
            return;
        }
        let combined = parts.join("\n");
        if let Ok(store) = Store::open(&db) {
            let _ = store.set_ocr(id, &combined);
        }
    });
}

fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .context("invalid image dimensions")?;
    img.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_file_uri_text_accepts_list() {
        let text = "file:///home/user/a.png\nfile:///tmp/b%20c.txt\n";
        let paths = parse_file_uri_text(text).expect("parsed");
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/a.png"));
        assert_eq!(paths[1], PathBuf::from("/tmp/b c.txt"));
    }

    #[test]
    fn parse_file_uri_text_rejects_plain() {
        assert!(parse_file_uri_text("hello world").is_none());
        assert!(parse_file_uri_text("file://a\nnot-a-path").is_none());
    }

    #[test]
    fn image_ext_detection() {
        assert!(is_image_path(Path::new("/tmp/shot.PNG")));
        assert!(is_image_path(Path::new("photo.jpeg")));
        assert!(!is_image_path(Path::new("notes.txt")));
    }

    #[test]
    fn files_hash_and_preview() {
        let content = ClipContent::Files(vec![
            PathBuf::from("/tmp/one.png"),
            PathBuf::from("/tmp/two.txt"),
        ]);
        assert!(content_hash(&content).starts_with('f'));
        let p = preview_of(&content);
        assert!(p.contains("2 files"));
        assert!(p.contains("one.png"));
    }

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
    match kind {
        "image" => {
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
        }
        "files" => {
            // Prefer re-offering a real uri-list so file managers accept the paste.
            let text = fs::read_to_string(config::pending_text_path())
                .context("reading staged file list")?;
            let paths: Vec<PathBuf> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect();
            if paths.is_empty() {
                cb.set().wait().text(text)?;
            } else if cb.set().wait().file_list(&paths).is_err() {
                // Fallback: plain paths as text.
                cb.set().wait().text(text)?;
            }
        }
        _ => {
            let text = fs::read_to_string(config::pending_text_path())
                .context("reading staged text")?;
            cb.set().wait().text(text)?;
        }
    }
    Ok(())
}
