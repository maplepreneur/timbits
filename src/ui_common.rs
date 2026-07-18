//! Shared egui helpers for the picker windows.

use eframe::egui;
use std::sync::Arc;

use crate::storage::{now_ts, EntryKind};

/// Monochrome Noto Emoji — egui's rasterizer (ab_glyph) can't handle color
/// CBDT fonts like Noto Color Emoji, so we bundle the monochrome variant
/// which renders everywhere.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/NotoEmoji.ttf");

/// App logo (window decorations + install-time desktop icons).
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

pub fn native_options(title: &str, width: f32, height: f32) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("timbits")
        .with_inner_size([width, height])
        .with_min_inner_size([380.0, 260.0])
        .with_window_level(egui::WindowLevel::AlwaysOnTop);

    if let Ok(icon) = eframe::icon_data::from_png_bytes(LOGO_PNG) {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

/// Install the emoji font as a fallback for both font families.
pub fn apply_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto-emoji".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(EMOJI_FONT)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto-emoji".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn kind_icon(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Text => "📄",
        EntryKind::Image => "🖼",
        EntryKind::Files => "📁",
    }
}

/// "just now" / "5m" / "3h" / "12d"
pub fn rel_time(ts: i64) -> String {
    let secs = (now_ts() - ts).max(0);
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}
