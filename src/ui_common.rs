//! Shared egui helpers — floating GNOME-styled chrome for the pickers.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, RichText, Sense, Stroke, Vec2, Visuals,
};
use std::process::Command;
use std::sync::Arc;

use crate::storage::{now_ts, EntryKind};

/// Monochrome Noto Emoji fallback for text labels (egui/ab_glyph cannot draw
/// CBDT colour fonts). Colour glyphs are painted as textures via `emoji_raster`.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/NotoEmoji.ttf");

/// App logo (window decorations + install-time desktop icons + empty states).
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

// ── GNOME / Adwaita-inspired palette (follows system dark/light) ──────────

/// Semantic colors sampled once per process (matches GNOME color-scheme).
#[derive(Clone, Copy)]
pub struct Palette {
    pub is_dark: bool,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub bg: Color32,
    pub bg_raised: Color32,
    pub bg_shade: Color32,
    pub view: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub selection: Color32,
    pub hover: Color32,
}

impl Palette {
    pub fn current() -> Self {
        let is_dark = system_prefers_dark();
        let accent = system_accent();
        if is_dark {
            // Adwaita dark–ish (works with Zorin/Dracula sessions too)
            Self {
                is_dark: true,
                accent,
                accent_fg: Color32::WHITE,
                bg: Color32::from_rgb(0x24, 0x24, 0x24),
                bg_raised: Color32::from_rgb(0x30, 0x30, 0x30),
                bg_shade: Color32::from_rgb(0x1e, 0x1e, 0x1e),
                view: Color32::from_rgb(0x1d, 0x1d, 0x1d),
                border: Color32::from_rgb(0x3d, 0x3d, 0x3d),
                text: Color32::from_rgb(0xff, 0xff, 0xff),
                text_muted: Color32::from_rgb(0x9a, 0x9a, 0x9a),
                selection: Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 55),
                hover: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            }
        } else {
            Self {
                is_dark: false,
                accent,
                accent_fg: Color32::WHITE,
                bg: Color32::from_rgb(0xfa, 0xfa, 0xfa),
                bg_raised: Color32::from_rgb(0xff, 0xff, 0xff),
                bg_shade: Color32::from_rgb(0xf0, 0xf0, 0xf0),
                view: Color32::from_rgb(0xff, 0xff, 0xff),
                border: Color32::from_rgb(0xcd, 0xcd, 0xcd),
                text: Color32::from_rgb(0x2e, 0x34, 0x36),
                text_muted: Color32::from_rgb(0x77, 0x76, 0x7b),
                selection: Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 40),
                hover: Color32::from_rgb(0xeb, 0xeb, 0xeb),
            }
        }
    }
}

fn system_prefers_dark() -> bool {
    // org.gnome.desktop.interface color-scheme: prefer-dark | prefer-light | default
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if s.contains("prefer-dark") {
                return true;
            }
            if s.contains("prefer-light") {
                return false;
            }
        }
    }
    // Fallback: gtk-theme name often ends in -dark / -Dark
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if s.contains("dark") || s.contains("dracula") {
                return true;
            }
        }
    }
    // Safe default for always-on-top overlays on desktop photos
    true
}

/// Map GNOME accent-color names → Adwaita palette.
fn system_accent() -> Color32 {
    let name = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_lowercase())
        .unwrap_or_default();

    match name.as_str() {
        "teal" => Color32::from_rgb(0x21, 0x90, 0xa4),
        "green" => Color32::from_rgb(0x3a, 0x94, 0x4a),
        "yellow" => Color32::from_rgb(0xc8, 0x88, 0x00),
        "orange" => Color32::from_rgb(0xed, 0x5b, 0x00),
        "red" => Color32::from_rgb(0xe6, 0x2d, 0x42),
        "pink" => Color32::from_rgb(0xd5, 0x61, 0x99),
        "purple" => Color32::from_rgb(0x91, 0x41, 0xac),
        "slate" => Color32::from_rgb(0x6f, 0x83, 0x96),
        // "blue" or unknown
        _ => Color32::from_rgb(0x35, 0x84, 0xe4),
    }
}

/// Floating, always-on-top, undecorated popup (GNOME-style launcher / popover).
///
/// Opaque fill (not transparent): Wayland/GNOME + wgpu often draws a broken
/// empty surface when `with_transparent(true)` is combined with a nested card.
pub fn native_options(title: &str, width: f32, height: f32) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("timbits")
        .with_inner_size([width, height])
        .with_min_inner_size([360.0, 240.0])
        .with_decorations(false)
        .with_transparent(false)
        .with_taskbar(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_resizable(true);

    if let Ok(icon) = eframe::icon_data::from_png_bytes(LOGO_PNG) {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    }
}

/// Solid window clear colour matching the GNOME-derived palette.
pub fn clear_color_for_theme() -> [f32; 4] {
    let p = Palette::current();
    [
        p.bg.r() as f32 / 255.0,
        p.bg.g() as f32 / 255.0,
        p.bg.b() as f32 / 255.0,
        1.0,
    ]
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

/// Apply Adwaita-inspired visuals matching the system dark/light preference.
pub fn apply_theme(ctx: &egui::Context) {
    let p = Palette::current();
    let theme = if p.is_dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };

    let mut visuals = if p.is_dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.override_text_color = Some(p.text);
    visuals.hyperlink_color = p.accent;
    visuals.warn_fg_color = p.accent;
    visuals.error_fg_color = Color32::from_rgb(0xe6, 0x2d, 0x42);
    visuals.window_fill = p.bg;
    visuals.panel_fill = p.bg;
    visuals.faint_bg_color = p.bg_shade;
    visuals.extreme_bg_color = p.bg_shade;
    visuals.code_bg_color = p.bg_raised;
    visuals.window_stroke = Stroke::NONE;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);

    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.0, p.accent);

    let widget = |bg: Color32, weak: Color32, stroke: Color32, fg: Color32| {
        egui::style::WidgetVisuals {
            bg_fill: bg,
            weak_bg_fill: weak,
            bg_stroke: Stroke::new(1.0, stroke),
            fg_stroke: Stroke::new(1.0, fg),
            corner_radius: CornerRadius::same(8),
            expansion: 0.0,
        }
    };

    visuals.widgets.noninteractive = widget(p.bg_raised, p.bg_shade, p.border, p.text_muted);
    visuals.widgets.inactive = widget(p.bg_raised, p.bg_shade, p.border, p.text);
    visuals.widgets.hovered = widget(p.hover, p.hover, p.accent, p.text);
    visuals.widgets.active = widget(p.accent, p.accent, p.accent, p.accent_fg);
    visuals.widgets.open = widget(p.hover, p.hover, p.accent, p.text);

    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals);

    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.window_margin = Margin::ZERO;
        style.spacing.interact_size = egui::vec2(40.0, 28.0);

        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
    });
}

/// Full-window floating popup chrome: header + body that expands to fill.
///
/// Call from a CentralPanel (any frame). Content must use the remaining
/// height (ScrollArea / expand) — the body gets `ui.available_size()` after
/// the header so it cannot collapse to zero.
pub fn floating_shell(
    ui: &mut egui::Ui,
    title: &str,
    ctx: &egui::Context,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let p = Palette::current();

    // Paint solid surface (window is already opaque Adwaita bg).
    let full = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(full, CornerRadius::same(12), p.bg);

    // ── Header (drag to move) ──────────────────────────────────────────
    let header = Frame::new()
        .fill(p.bg_raised)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").size(10.0).color(p.text_muted));
                ui.label(RichText::new(title).size(14.0).color(p.text).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui
                        .add_sized(
                            [28.0, 24.0],
                            egui::Button::new(RichText::new("x").size(14.0).color(p.text_muted))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE),
                        )
                        .on_hover_text("Close (Esc)");
                    if close.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

    let header_resp = header.response.interact(Sense::click_and_drag());
    if header_resp.dragged() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    // Separator
    let (sep, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(sep, 0.0, p.border);

    // ── Body: consume ALL remaining space ──────────────────────────────
    let body_size = ui.available_size();
    ui.allocate_ui_with_layout(
        body_size,
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(p.bg)
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    add_contents(ui);
                });
        },
    );
}

/// Search field chrome (inset entry, Adwaita style).
pub fn search_frame() -> Frame {
    let p = Palette::current();
    Frame::new()
        .inner_margin(Margin::symmetric(12, 8))
        .corner_radius(CornerRadius::same(10))
        .fill(p.view)
        .stroke(Stroke::new(1.0, p.border))
}

/// Soft card for list rows / preview panes.
pub fn card_frame(selected: bool) -> Frame {
    let p = Palette::current();
    let (fill, stroke) = if selected {
        (p.selection, Stroke::new(1.0, p.accent))
    } else {
        (p.bg_raised, Stroke::new(1.0, p.border))
    };
    Frame::new()
        .inner_margin(Margin::symmetric(10, 8))
        .corner_radius(CornerRadius::same(10))
        .fill(fill)
        .stroke(stroke)
}

/// Preview pane chrome.
pub fn preview_frame() -> Frame {
    let p = Palette::current();
    Frame::new()
        .inner_margin(Margin::same(12))
        .corner_radius(CornerRadius::same(10))
        .fill(p.view)
        .stroke(Stroke::new(1.0, p.border))
}

/// Small keycap-style label for footers.
pub fn keycap(ui: &mut egui::Ui, label: &str) {
    let p = Palette::current();
    Frame::new()
        .inner_margin(Margin::symmetric(6, 2))
        .corner_radius(CornerRadius::same(4))
        .fill(p.bg_shade)
        .stroke(Stroke::new(1.0, p.border))
        .show(ui, |ui| {
            ui.label(
                RichText::new(label)
                    .size(11.0)
                    .color(p.text_muted)
                    .strong(),
            );
        });
}

pub fn muted_label(ui: &mut egui::Ui, text: impl Into<String>) {
    let p = Palette::current();
    ui.label(RichText::new(text.into()).size(12.0).color(p.text_muted));
}

pub fn kind_icon(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Text => "📄",
        EntryKind::Image => "🖼",
        EntryKind::Files => "📁",
    }
}

pub fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Text => "Text",
        EntryKind::Image => "Image",
        EntryKind::Files => "Files",
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
