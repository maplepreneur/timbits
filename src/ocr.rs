//! Optional OCR of clipboard images via the `tesseract` CLI (runtime
//! dependency only — no dev libraries needed). If tesseract is not installed,
//! OCR is silently skipped.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// Is the tesseract binary available on this system?
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("tesseract")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Run OCR on an image file, returning the extracted text (if any).
pub fn ocr_image(path: &Path) -> Option<String> {
    let out = Command::new("tesseract")
        .arg(path.as_os_str())
        .arg("stdout")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
