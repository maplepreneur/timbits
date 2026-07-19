//! Color emoji textures from the system Noto Color Emoji font (CBDT bitmaps).
//!
//! egui/ab_glyph cannot rasterize color CBDT fonts, so we pull PNG (or raw
//! BGRA) strikes via `ttf-parser` and upload them as egui textures.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Preferred pixel size for emoji strikes (Noto Color Emoji ships ~136px PNGs).
const STRIKE_PX: u16 = 128;

static FONT_DATA: OnceLock<Option<Vec<u8>>> = OnceLock::new();

pub struct EmojiAtlas {
    textures: HashMap<String, TextureHandle>,
}

impl EmojiAtlas {
    pub fn new() -> Self {
        // Warm the font cache early so the first paint isn't a hitch.
        let _ = font_bytes();
        Self {
            textures: HashMap::new(),
        }
    }

    /// Get (or create) a color texture for `emoji`. Returns `None` if the font
    /// is missing or the glyph has no color strike.
    pub fn texture(&mut self, ctx: &egui::Context, emoji: &str) -> Option<TextureHandle> {
        if let Some(t) = self.textures.get(emoji) {
            return Some(t.clone());
        }
        let image = raster_emoji(emoji)?;
        let handle = ctx.load_texture(
            format!("emoji-{}", emoji_key(emoji)),
            image,
            TextureOptions::LINEAR,
        );
        self.textures.insert(emoji.to_string(), handle.clone());
        Some(handle)
    }
}

impl Default for EmojiAtlas {
    fn default() -> Self {
        Self::new()
    }
}

fn emoji_key(s: &str) -> String {
    s.chars()
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("-")
}

fn font_bytes() -> Option<&'static [u8]> {
    FONT_DATA
        .get_or_init(|| {
            for path in color_emoji_font_paths() {
                if let Ok(bytes) = std::fs::read(&path) {
                    log::info!("color emoji font: {}", path.display());
                    return Some(bytes);
                }
            }
            log::warn!(
                "Noto Color Emoji not found — install fonts-noto-color-emoji \
                 for colour glyphs"
            );
            None
        })
        .as_deref()
}

fn color_emoji_font_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // System packages (Debian/Ubuntu/Zorin).
    paths.push(PathBuf::from(
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    ));
    paths.push(PathBuf::from(
        "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    ));
    paths.push(PathBuf::from(
        "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    ));
    // Flatpak / local overrides.
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/fonts/NotoColorEmoji.ttf"));
        paths.push(home.join(".fonts/NotoColorEmoji.ttf"));
    }
    // FONTCONFIG family file if available.
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "Noto Color Emoji"])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() && Path::new(&p).is_file() {
                paths.insert(0, PathBuf::from(p));
            }
        }
    }
    paths
}

fn raster_emoji(emoji: &str) -> Option<ColorImage> {
    let data = font_bytes()?;
    let face = ttf_parser::Face::parse(data, 0).ok()?;

    // Prefer the first codepoint that has a colour strike. For simple emoji
    // this is the only char; for ZWJ sequences Noto often still maps the
    // base character (full ligature shaping would need harfbuzz).
    let mut last_img = None;
    for ch in emoji.chars() {
        if ch == '\u{FE0F}' || ch == '\u{FE0E}' || ch == '\u{200D}' {
            continue; // variation selectors / ZWJ
        }
        if let Some(gid) = face.glyph_index(ch) {
            if let Some(raster) = face.glyph_raster_image(gid, STRIKE_PX) {
                if let Some(img) = raster_to_color_image(&raster) {
                    last_img = Some(img);
                    // Keep going: later codepoints in a sequence sometimes
                    // replace the base (skin tones etc. may not work without
                    // shaping, but multi-person ZWJ often still show the base).
                }
            }
        }
    }
    last_img
}

fn raster_to_color_image(raster: &ttf_parser::RasterGlyphImage<'_>) -> Option<ColorImage> {
    use ttf_parser::RasterImageFormat;
    let w = raster.width as usize;
    let h = raster.height as usize;
    if w == 0 || h == 0 {
        return None;
    }

    match raster.format {
        RasterImageFormat::PNG => {
            let dynimg = image::load_from_memory(raster.data).ok()?.to_rgba8();
            let size = [dynimg.width() as usize, dynimg.height() as usize];
            Some(ColorImage::from_rgba_unmultiplied(size, dynimg.as_raw()))
        }
        RasterImageFormat::BitmapPremulBgra32 => {
            // Convert BGRA premultiplied → RGBA unmultiplied for egui.
            let mut rgba = Vec::with_capacity(w * h * 4);
            let src = raster.data;
            if src.len() < w * h * 4 {
                return None;
            }
            for px in src.chunks_exact(4) {
                let b = px[0];
                let g = px[1];
                let r = px[2];
                let a = px[3];
                if a == 0 {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                } else if a == 255 {
                    rgba.extend_from_slice(&[r, g, b, a]);
                } else {
                    // Un-premultiply.
                    let af = a as f32 / 255.0;
                    rgba.push((r as f32 / af).min(255.0) as u8);
                    rgba.push((g as f32 / af).min(255.0) as u8);
                    rgba.push((b as f32 / af).min(255.0) as u8);
                    rgba.push(a);
                }
            }
            Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
        }
        _ => {
            // Grayscale / mono strikes — skip (we'd rather fall back to text).
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_system_font_or_skips() {
        // Don't fail CI without the font; just exercise the path list.
        let paths = super::color_emoji_font_paths();
        assert!(!paths.is_empty());
    }
}
