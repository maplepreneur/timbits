//! Shared egui helpers — floating chrome + system GTK theme palette.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, RichText, Sense, Stroke, Vec2, Visuals,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use crate::storage::{now_ts, EntryKind};

/// Monochrome Noto Emoji fallback for text labels (egui/ab_glyph cannot draw
/// CBDT colour fonts). Colour glyphs are painted as textures via `emoji_raster`.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/NotoEmoji.ttf");

/// App logo (window decorations + install-time desktop icons + empty states).
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

// ── Live GNOME / Zorin GTK theme palette ──────────────────────────────────

/// Semantic colors taken from the **active GTK theme** (`@define-color` in
/// gtk.css), not a hard-coded Adwaita approximation.
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
        static CACHED: OnceLock<Palette> = OnceLock::new();
        *CACHED.get_or_init(load_system_palette)
    }
}

fn load_system_palette() -> Palette {
    let is_dark = system_prefers_dark();
    let vars = load_gtk_named_colors(is_dark);

    let bg = resolve_color(&vars, &["theme_bg_color", "bg_color"])
        .unwrap_or(if is_dark {
            Color32::from_rgb(0x24, 0x24, 0x24)
        } else {
            Color32::from_rgb(0xfa, 0xfa, 0xfa)
        });
    let view = resolve_color(
        &vars,
        &["theme_base_color", "content_view_bg", "base_color", "text_view_bg"],
    )
    .unwrap_or(if is_dark {
        Color32::from_rgb(0x1d, 0x1d, 0x1d)
    } else {
        Color32::from_rgb(0xff, 0xff, 0xff)
    });
    let text = resolve_color(&vars, &["theme_fg_color", "theme_text_color", "fg_color", "text_color"])
        .unwrap_or(if is_dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(0x2e, 0x34, 0x36)
        });
    let text_muted = resolve_color(
        &vars,
        &[
            "insensitive_fg_color",
            "placeholder_text_color",
            "theme_unfocused_fg_color",
            "unfocused_fg_color",
        ],
    )
    .unwrap_or(mix(text, bg, 0.45));
    let border_raw = resolve_color_rgba(&vars, &["borders", "unfocused_borders"]);
    let border = match border_raw {
        Some((c, a)) if a < 0.05 || (c.r() == 0 && c.g() == 0 && c.b() == 0 && a < 0.15) => {
            // "transparent" / invisible borders → subtle edge from text on bg
            mix(text, bg, 0.18)
        }
        Some((c, a)) if a < 0.99 => blend_over(c, a, bg),
        Some((c, _)) => c,
        None => mix(text, bg, 0.22),
    };
    let sel = resolve_color_rgba(
        &vars,
        &["theme_selected_bg_color", "selected_bg_color"],
    );
    let (selection, accent_from_sel) = match sel {
        Some((c, a)) if a < 0.99 => (blend_over(c, a, view), c),
        Some((c, _)) => (
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), if is_dark { 55 } else { 40 }),
            c,
        ),
        None => {
            let a = system_accent_gsettings();
            (
                Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), if is_dark { 55 } else { 40 }),
                a,
            )
        }
    };
    // Prefer theme selection as accent (Dracula pink/purple, Zorin accent, …);
    // fall back to GNOME accent-color gsetting for stock Adwaita.
    let accent = accent_from_sel;
    let accent_fg = resolve_color(
        &vars,
        &["theme_selected_fg_color", "selected_fg_color"],
    )
    .unwrap_or_else(|| contrast_fg(accent));

    let bg_raised = resolve_color(&vars, &["header_bg_color", "wm_bg_a"])
        .unwrap_or_else(|| lighten(view, if is_dark { 0.08 } else { 0.02 }));
    let bg_shade = resolve_color(&vars, &["insensitive_bg_color", "theme_unfocused_bg_color"])
        .unwrap_or_else(|| darken(bg, if is_dark { 0.06 } else { 0.04 }));
    let hover = resolve_color(&vars, &["wm_button_hover_color_a", "wm_button_hover_color_b"])
        .unwrap_or_else(|| lighten(view, if is_dark { 0.12 } else { 0.06 }));

    log::info!(
        "theme palette: dark={is_dark} bg={bg:?} view={view:?} text={text:?} accent={accent:?}"
    );

    Palette {
        is_dark,
        accent,
        accent_fg,
        bg,
        bg_raised,
        bg_shade,
        view,
        border,
        text,
        text_muted,
        selection,
        hover,
    }
}

/// Load `@define-color` map from active GTK theme + user gtk.css overrides.
fn load_gtk_named_colors(is_dark: bool) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for path in gtk_css_candidates(is_dark) {
        if !path.is_file() {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(&path) {
            parse_define_colors(&raw, &mut map);
            log::debug!("gtk colors from {} ({} names)", path.display(), map.len());
        }
    }
    map
}

fn gtk_css_candidates(is_dark: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = dirs::home_dir();
    let theme = gsettings_string("org.gnome.desktop.interface", "gtk-theme");

    // Base → theme package → user overrides last (later files win on insert).

    // Stock Adwaita fallback (sparse; only used when nothing else defines a name).
    if is_dark {
        out.push(PathBuf::from("/usr/share/themes/Adwaita-dark/gtk-3.0/gtk.css"));
    }
    out.push(PathBuf::from("/usr/share/themes/Adwaita/gtk-3.0/gtk.css"));

    // Active theme package (Dracula, ZorinBlue-Dark, …).
    if let Some(name) = theme {
        let name = name.trim().trim_matches('\'').to_string();
        if !name.is_empty() {
            let mut roots = Vec::new();
            if let Some(ref h) = home {
                roots.push(h.join(".local/share/themes").join(&name));
                roots.push(h.join(".themes").join(&name));
            }
            roots.push(PathBuf::from("/usr/share/themes").join(&name));
            roots.push(PathBuf::from("/usr/local/share/themes").join(&name));

            for root in roots {
                // Prefer the dark/light sheet that matches the session, then the
                // generic gtk.css (many themes only ship one file).
                if is_dark {
                    out.push(root.join("gtk-4.0/gtk-dark.css"));
                    out.push(root.join("gtk-3.0/gtk-dark.css"));
                    out.push(root.join("gtk-3.20/gtk-dark.css"));
                } else {
                    out.push(root.join("gtk-4.0/gtk-light.css"));
                    out.push(root.join("gtk-3.0/gtk-light.css"));
                }
                out.push(root.join("gtk-4.0/gtk.css"));
                out.push(root.join("gtk-3.20/gtk.css"));
                out.push(root.join("gtk-3.0/gtk.css"));
            }
        }
    }

    // User GTK CSS last so hand-tuned colours win.
    if let Some(ref h) = home {
        if is_dark {
            out.push(h.join(".config/gtk-4.0/gtk-dark.css"));
            out.push(h.join(".config/gtk-3.0/gtk-dark.css"));
        } else {
            out.push(h.join(".config/gtk-4.0/gtk-light.css"));
        }
        out.push(h.join(".config/gtk-4.0/gtk.css"));
        out.push(h.join(".config/gtk-3.0/gtk.css"));
    }
    out
}

/// Parse `@define-color name value;` into the map (later files / later lines win).
fn parse_define_colors(css: &str, map: &mut HashMap<String, String>) {
    // Strip /* … */ comments so mid-line comments don't break values.
    let mut cleaned = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            cleaned.push(' ');
            continue;
        }
        cleaned.push(bytes[i] as char);
        i += 1;
    }

    for line in cleaned.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("@define-color") else {
            continue;
        };
        let rest = rest.trim().trim_end_matches(';').trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let Some(name) = parts.next() else { continue };
        let Some(value) = parts.next() else { continue };
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if !name.is_empty() && !value.is_empty() {
            map.insert(name, value);
        }
    }
}

fn resolve_color(map: &HashMap<String, String>, names: &[&str]) -> Option<Color32> {
    resolve_color_rgba(map, names).map(|(c, _)| c)
}

fn resolve_color_rgba(map: &HashMap<String, String>, names: &[&str]) -> Option<(Color32, f32)> {
    for name in names {
        if let Some(val) = map.get(*name) {
            if let Some(c) = parse_css_color(val, map, 0) {
                return Some(c);
            }
        }
    }
    None
}

fn parse_css_color(value: &str, map: &HashMap<String, String>, depth: u8) -> Option<(Color32, f32)> {
    if depth > 12 {
        return None;
    }
    let v = value.trim();
    if v.eq_ignore_ascii_case("transparent") {
        return Some((Color32::TRANSPARENT, 0.0));
    }
    // @other_name reference
    if let Some(ref_name) = v.strip_prefix('@') {
        let ref_name = ref_name.trim();
        if let Some(next) = map.get(ref_name) {
            return parse_css_color(next, map, depth + 1);
        }
        return None;
    }
    // shade(#abc, 1.2) / shade(@name, 0.9) — approximate
    if let Some(inner) = v.strip_prefix("shade(").and_then(|s| s.strip_suffix(')')) {
        let mut parts = inner.splitn(2, ',');
        let color_s = parts.next()?.trim();
        let factor: f32 = parts.next()?.trim().parse().ok()?;
        let (c, a) = parse_css_color(color_s, map, depth + 1)?;
        return Some((shade_color(c, factor), a));
    }
    // alpha(black, 0.35) / alpha(#fff, 0.1)
    if let Some(inner) = v.strip_prefix("alpha(").and_then(|s| s.strip_suffix(')')) {
        let mut parts = inner.splitn(2, ',');
        let color_s = parts.next()?.trim();
        let a: f32 = parts.next()?.trim().parse().ok()?;
        let (c, _) = parse_css_color(color_s, map, depth + 1)?;
        return Some((c, a.clamp(0.0, 1.0)));
    }
    // #rgb / #rrggbb / #rrggbbaa
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex(hex);
    }
    // rgb(r,g,b) / rgba(r,g,b,a)
    if let Some(inner) = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|s| s.strip_suffix(')'))
    {
        let nums: Vec<&str> = inner.split(',').map(str::trim).collect();
        if nums.len() >= 3 {
            let r = parse_css_channel(nums[0])?;
            let g = parse_css_channel(nums[1])?;
            let b = parse_css_channel(nums[2])?;
            let a = if nums.len() >= 4 {
                nums[3].parse::<f32>().ok()?.clamp(0.0, 1.0)
            } else {
                1.0
            };
            return Some((Color32::from_rgb(r, g, b), a));
        }
    }
    // Named CSS colors we might see
    match v.to_ascii_lowercase().as_str() {
        "black" => Some((Color32::BLACK, 1.0)),
        "white" => Some((Color32::WHITE, 1.0)),
        "red" => Some((Color32::from_rgb(255, 0, 0), 1.0)),
        _ => None,
    }
}

fn parse_css_channel(s: &str) -> Option<u8> {
    if let Some(p) = s.strip_suffix('%') {
        let f: f32 = p.parse().ok()?;
        return Some((f.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8);
    }
    // 0–1 float or 0–255 int
    if s.contains('.') {
        let f: f32 = s.parse().ok()?;
        if f <= 1.0 {
            return Some((f.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        return Some(f.clamp(0.0, 255.0).round() as u8);
    }
    s.parse().ok()
}

fn parse_hex(hex: &str) -> Option<(Color32, f32)> {
    let hex = hex.trim();
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            Some((Color32::from_rgb(r, g, b), 1.0))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((Color32::from_rgb(r, g, b), 1.0))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some((Color32::from_rgb(r, g, b), a as f32 / 255.0))
        }
        _ => None,
    }
}

fn blend_over(fg: Color32, a: f32, bg: Color32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    Color32::from_rgb(
        (fg.r() as f32 * a + bg.r() as f32 * inv).round() as u8,
        (fg.g() as f32 * a + bg.g() as f32 * inv).round() as u8,
        (fg.b() as f32 * a + bg.b() as f32 * inv).round() as u8,
    )
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 * t + b.r() as f32 * (1.0 - t)).round() as u8,
        (a.g() as f32 * t + b.g() as f32 * (1.0 - t)).round() as u8,
        (a.b() as f32 * t + b.b() as f32 * (1.0 - t)).round() as u8,
    )
}

fn lighten(c: Color32, amount: f32) -> Color32 {
    mix(Color32::WHITE, c, amount)
}

fn darken(c: Color32, amount: f32) -> Color32 {
    mix(Color32::BLACK, c, amount)
}

/// GTK `shade(color, k)`: k>1 lightens, k<1 darkens (rough HSL-L scale).
fn shade_color(c: Color32, factor: f32) -> Color32 {
    if (factor - 1.0).abs() < 0.001 {
        return c;
    }
    if factor > 1.0 {
        lighten(c, ((factor - 1.0) * 0.5).clamp(0.0, 0.9))
    } else {
        darken(c, ((1.0 - factor) * 0.6).clamp(0.0, 0.9))
    }
}

fn contrast_fg(bg: Color32) -> Color32 {
    // Relative luminance
    let l = 0.2126 * bg.r() as f32 + 0.7152 * bg.g() as f32 + 0.0722 * bg.b() as f32;
    if l > 140.0 {
        Color32::from_rgb(0x1e, 0x1e, 0x1e)
    } else {
        Color32::WHITE
    }
}

fn system_prefers_dark() -> bool {
    if let Some(s) = gsettings_string("org.gnome.desktop.interface", "color-scheme") {
        let s = s.to_lowercase();
        if s.contains("prefer-dark") {
            return true;
        }
        if s.contains("prefer-light") {
            return false;
        }
    }
    if let Some(s) = gsettings_string("org.gnome.desktop.interface", "gtk-theme") {
        let s = s.to_lowercase();
        if s.contains("dark") || s.contains("dracula") || s.contains("night") {
            return true;
        }
        if s.contains("light") {
            return false;
        }
    }
    true
}

fn system_accent_gsettings() -> Color32 {
    let name = gsettings_string("org.gnome.desktop.interface", "accent-color")
        .unwrap_or_default()
        .trim()
        .trim_matches('\'')
        .to_lowercase();
    match name.as_str() {
        "teal" => Color32::from_rgb(0x21, 0x90, 0xa4),
        "green" => Color32::from_rgb(0x3a, 0x94, 0x4a),
        "yellow" => Color32::from_rgb(0xc8, 0x88, 0x00),
        "orange" => Color32::from_rgb(0xed, 0x5b, 0x00),
        "red" => Color32::from_rgb(0xe6, 0x2d, 0x42),
        "pink" => Color32::from_rgb(0xd5, 0x61, 0x99),
        "purple" => Color32::from_rgb(0x91, 0x41, 0xac),
        "slate" => Color32::from_rgb(0x6f, 0x83, 0x96),
        _ => Color32::from_rgb(0x35, 0x84, 0xe4),
    }
}

fn gsettings_string(schema: &str, key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn parse_hex_and_rgba() {
        let map = HashMap::new();
        let (c, a) = parse_css_color("#1e1f29", &map, 0).unwrap();
        assert_eq!(c, Color32::from_rgb(0x1e, 0x1f, 0x29));
        assert!((a - 1.0).abs() < 0.01);
        let (c, a) = parse_css_color("rgba(189, 147, 249, 0.5)", &map, 0).unwrap();
        assert_eq!(c, Color32::from_rgb(189, 147, 249));
        assert!((a - 0.5).abs() < 0.01);
    }

    #[test]
    fn resolve_at_refs() {
        let mut map = HashMap::new();
        map.insert("bg_color".into(), "#1e1f29".into());
        map.insert("theme_bg_color".into(), "@bg_color".into());
        let c = resolve_color(&map, &["theme_bg_color"]).unwrap();
        assert_eq!(c, Color32::from_rgb(0x1e, 0x1f, 0x29));
    }

    #[test]
    fn parse_define_color_block() {
        let css = r#"
@define-color bg_color #1e1f29;
@define-color theme_bg_color @bg_color;
/*@define-color selected_bg_color #00b0ff;*/
@define-color selected_bg_color #ff79c6;
"#;
        let mut map = HashMap::new();
        parse_define_colors(css, &mut map);
        assert_eq!(map.get("bg_color").map(String::as_str), Some("#1e1f29"));
        assert_eq!(
            map.get("selected_bg_color").map(String::as_str),
            Some("#ff79c6")
        );
    }

    #[test]
    fn system_palette_loads() {
        // Smoke: must not panic; on a GNOME/Zorin box with a theme installed
        // we should get non-default text contrast (fg != pure black and bg).
        let p = load_system_palette();
        assert!(p.text != p.bg, "text and bg must differ");
    }

    #[test]
    fn parse_monitor_geom() {
        let line = " 0: +*DP-5 2560/700x1440/390+1440+581  DP-5";
        let g = parse_xrandr_geom(line).expect("geom");
        assert_eq!(g, (1440, 581, 2560, 1440));
        let q = "DP-5 connected primary 2560x1440+1440+581 (normal left inverted right x axis y axis)";
        let g = parse_xrandr_query_line(q).expect("query");
        assert_eq!(g, (1440, 581, 2560, 1440));
    }
}

/// Floating quick-access popup (emoji / clipboard) — like 1Password Quick Access.
///
/// - Undecorated, always-on-top, no taskbar entry
/// - X11 `Utility` type so tiling WMs / GNOME extensions usually leave it floating
/// - Centered on the **monitor under the pointer** (not always the primary)
///
/// Opaque fill (not transparent): Wayland/GNOME + wgpu often draws a broken
/// empty surface when `with_transparent(true)` is combined with a nested card.
pub fn native_options(title: &str, width: f32, height: f32) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        // Distinct app id so compositors/tilers can match picker windows.
        .with_app_id("timbits.picker")
        .with_inner_size([width, height])
        .with_min_inner_size([320.0, 200.0])
        .with_max_inner_size([width * 1.5, height * 1.5])
        .with_decorations(false)
        .with_transparent(false)
        .with_taskbar(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        // Utility/Dialog: typically excluded from auto-tile / window snapping.
        .with_window_type(egui::X11WindowType::Utility)
        .with_resizable(true);

    // Center on the screen that currently has the cursor (multi-monitor).
    // eframe's `centered: true` only uses the primary monitor and ignores its
    // offset — wrong on setups where primary is not at (0,0).
    if let Some(pos) = center_on_pointer_monitor(width, height) {
        viewport = viewport.with_position(pos);
    }

    if let Ok(icon) = eframe::icon_data::from_png_bytes(LOGO_PNG) {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    eframe::NativeOptions {
        viewport,
        // We set position ourselves for the correct monitor.
        centered: false,
        ..Default::default()
    }
}

/// Re-apply centering after the window is mapped (Wayland/Mutter often ignores
/// pre-map `OuterPosition`). Call once from the picker’s first `ui` frame.
pub fn reapply_pointer_monitor_center(ctx: &egui::Context, width: f32, height: f32) {
    if let Some(pos) = center_on_pointer_monitor(width, height) {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}

/// Top-left outer position so a `width`×`height` window is centered on the
/// monitor that contains the mouse pointer.
pub fn center_on_pointer_monitor(width: f32, height: f32) -> Option<egui::Pos2> {
    let (px, py) = pointer_position()?;
    let monitors = list_monitors();
    if monitors.is_empty() {
        return None;
    }
    let mon = monitors
        .iter()
        .find(|m| px >= m.x && px < m.x + m.w && py >= m.y && py < m.y + m.h)
        .or_else(|| monitors.iter().find(|m| m.primary))
        .unwrap_or(&monitors[0]);

    // Clamp so the window stays fully on that monitor when possible.
    let max_x = (mon.x + mon.w) as f32 - width;
    let max_y = (mon.y + mon.h) as f32 - height;
    let x = (mon.x as f32 + (mon.w as f32 - width) / 2.0)
        .clamp(mon.x as f32, max_x.max(mon.x as f32));
    let y = (mon.y as f32 + (mon.h as f32 - height) / 2.0)
        .clamp(mon.y as f32, max_y.max(mon.y as f32));

    log::debug!(
        "center picker {width}x{height} on monitor {}+{} {}x{} (pointer {px},{py}) → ({x:.0},{y:.0})",
        mon.x,
        mon.y,
        mon.w,
        mon.h
    );
    Some(egui::pos2(x, y))
}

#[derive(Debug, Clone)]
struct MonitorGeom {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    primary: bool,
}

fn pointer_position() -> Option<(i32, i32)> {
    // xdotool works on X11 and typically on GNOME XWayland for cursor coords.
    let out = Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut x = None;
    let mut y = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("X=") {
            x = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("Y=") {
            y = v.trim().parse().ok();
        }
    }
    Some((x?, y?))
}

fn list_monitors() -> Vec<MonitorGeom> {
    // Prefer `xrandr --listmonitors` (compact, reliable on X11/XWayland).
    if let Ok(out) = Command::new("xrandr").args(["--listmonitors"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut mons = Vec::new();
            // " 0: +*DP-5 2560/700x1440/390+1440+581  DP-5"
            // " 1: +DP-1 1440/700x2560/390+0+0  DP-1"
            for line in s.lines().skip(1) {
                let primary = line.contains("*");
                // Find "WxH+X+Y" with optional /mm parts: 2560/700x1440/390+1440+581
                if let Some(geom) = parse_xrandr_geom(line) {
                    mons.push(MonitorGeom {
                        x: geom.0,
                        y: geom.1,
                        w: geom.2,
                        h: geom.3,
                        primary,
                    });
                }
            }
            if !mons.is_empty() {
                return mons;
            }
        }
    }

    // Fallback: `xrandr --query`
    if let Ok(out) = Command::new("xrandr").args(["--query"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut mons = Vec::new();
            for line in s.lines() {
                if !line.contains(" connected") {
                    continue;
                }
                let primary = line.contains(" primary ");
                // "DP-5 connected primary 2560x1440+1440+581 ..."
                if let Some(geom) = parse_xrandr_query_line(line) {
                    mons.push(MonitorGeom {
                        x: geom.0,
                        y: geom.1,
                        w: geom.2,
                        h: geom.3,
                        primary,
                    });
                }
            }
            return mons;
        }
    }
    Vec::new()
}

/// Parse geometry from listmonitors: `2560/700x1440/390+1440+581` or `2560x1440+1440+581`.
fn parse_xrandr_geom(line: &str) -> Option<(i32, i32, i32, i32)> {
    // Find the last `+X+Y` and work backwards for WxH.
    let plus = line.rfind('+')?;
    let rest = &line[..plus];
    let plus2 = rest.rfind('+')?;
    let y: i32 = line[plus + 1..]
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let x: i32 = rest[plus2 + 1..].parse().ok()?;
    let before = &rest[..plus2];
    // before ends with "WxH" possibly with /mm: "2560/700x1440/390"
    let x_char = before.rfind('x')?;
    let h_part = &before[x_char + 1..];
    let h: i32 = h_part.split('/').next()?.parse().ok()?;
    let w_part = before[..x_char].split_whitespace().last()?;
    let w: i32 = w_part.split('/').next()?.parse().ok()?;
    Some((x, y, w, h))
}

fn parse_xrandr_query_line(line: &str) -> Option<(i32, i32, i32, i32)> {
    // Look for token like 2560x1440+1440+581
    for tok in line.split_whitespace() {
        if let Some((wh, rest)) = tok.split_once('+') {
            if let Some((w, h)) = wh.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>()) {
                    let mut parts = rest.split('+');
                    if let (Some(xs), Some(ys)) = (parts.next(), parts.next()) {
                        if let (Ok(x), Ok(y)) = (xs.parse::<i32>(), ys.parse::<i32>()) {
                            return Some((x, y, w, h));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Normal app window with **real** GNOME/libadwaita decorations (title bar,
/// close/min/max, themed by the desktop shell). Use for Settings and other
/// persistent windows — not for transient pickers.
pub fn app_window_options(title: &str, width: f32, height: f32) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("timbits")
        .with_inner_size([width, height])
        .with_min_inner_size([420.0, 360.0])
        // Let the compositor draw the real CSD/SSD title bar (GNOME Shell theme).
        .with_decorations(true)
        .with_transparent(false)
        .with_taskbar(true)
        .with_window_level(egui::WindowLevel::Normal)
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

/// Apply egui visuals from the live GTK theme palette.
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

/// Full-window floating popup chrome for undecorated pickers (no title bar).
///
/// Just theme fill + padding — close with Esc. Call from a CentralPanel.
pub fn floating_shell(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let p = Palette::current();

    let full = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(full, CornerRadius::same(12), p.bg);

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

/// Content area for a **decorated** app window (real GNOME title bar outside).
///
/// No fake header/close — the shell draws those. Body fills the window with
/// Adwaita-matching padding and fill from the current palette.
pub fn app_shell(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let p = Palette::current();
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, CornerRadius::ZERO, p.bg);

    let body_size = ui.available_size();
    ui.allocate_ui_with_layout(body_size, egui::Layout::top_down(egui::Align::Min), |ui| {
        Frame::new()
            .fill(p.bg)
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                add_contents(ui);
            });
    });
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

/// Small keycap-style label for footers. Prefer plain ASCII in `label`
/// (arrows/emoji often render as □ with default fonts).
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
                    .color(p.text)
                    .strong(),
            );
        });
}

pub fn muted_label(ui: &mut egui::Ui, text: impl Into<String>) {
    let p = Palette::current();
    ui.label(RichText::new(text.into()).size(12.0).color(p.text_muted));
}

/// Compact footer hint line using only ASCII (no missing-glyph boxes).
pub fn footer_hints(ui: &mut egui::Ui, left: &str) {
    let p = Palette::current();
    ui.horizontal(|ui| {
        ui.label(RichText::new(left).size(12.0).color(p.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Arrows move  ·  Enter paste  ·  Esc close")
                    .size(11.0)
                    .color(p.text_muted),
            );
        });
    });
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
