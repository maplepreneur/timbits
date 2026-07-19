//! Emoji picker window: search field focused on open, arrow-key navigation
//! over a grid, Enter to paste, Esc to cancel.

use anyhow::Result;
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, Vec2};
use std::sync::{Arc, Mutex};

use crate::config;
use crate::emoji_raster::EmojiAtlas;
use crate::ui_common::{self, Palette};

const COLS: usize = 10;
const MAX_SHOWN: usize = 500;
const MAX_RECENTS: usize = 20;
const CELL: f32 = 42.0;
const EMOJI_SIZE: f32 = 24.0;
const EMOJI_IMG: f32 = 28.0;

struct Shown {
    text: String,
    name: String,
    /// True when this row is a recent pick (search empty).
    recent: bool,
}

struct EmojiApp {
    search: String,
    /// (emoji, display name, lowercase search key)
    all: Vec<(&'static str, String, String)>,
    shown: Vec<Shown>,
    /// How many leading items are recents (for section header).
    recent_count: usize,
    selected: usize,
    chosen: Arc<Mutex<Option<String>>>,
    /// Selection moved via keyboard this frame → scroll it into view.
    moved: bool,
    focused_once: bool,
    atlas: EmojiAtlas,
}

impl EmojiApp {
    fn new(chosen: Arc<Mutex<Option<String>>>) -> Self {
        let mut all = Vec::new();
        for e in emojis::iter() {
            let name = e.name().to_string();
            let shortcodes: Vec<&str> = e.shortcodes().collect();
            let key = if shortcodes.is_empty() {
                name.clone()
            } else {
                format!("{} {}", name, shortcodes.join(" "))
            }
            .to_lowercase();
            all.push((e.as_str(), name, key));
        }
        let mut app = Self {
            search: String::new(),
            all,
            shown: Vec::new(),
            recent_count: 0,
            selected: 0,
            chosen,
            moved: false,
            focused_once: false,
            atlas: EmojiAtlas::new(),
        };
        app.refilter();
        app
    }

    fn load_recents() -> Vec<String> {
        std::fs::read_to_string(config::recents_path())
            .map(|s| {
                s.lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn save_recents(recents: &[String]) {
        if config::ensure_dirs().is_ok() {
            let _ = std::fs::write(config::recents_path(), recents.join("\n"));
        }
    }

    fn refilter(&mut self) {
        let query = self.search.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut shown: Vec<Shown> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut recent_count = 0;

        if words.is_empty() {
            for recent in Self::load_recents() {
                if !seen.insert(recent.clone()) {
                    continue;
                }
                let name = self
                    .all
                    .iter()
                    .find(|(e, _, _)| *e == recent)
                    .map(|(_, n, _)| n.clone())
                    .unwrap_or_else(|| "recent".into());
                shown.push(Shown {
                    text: recent,
                    name,
                    recent: true,
                });
                recent_count += 1;
            }
        }

        for (emoji, name, key) in &self.all {
            if shown.len() >= MAX_SHOWN {
                break;
            }
            if !words.iter().all(|w| key.contains(w)) {
                continue;
            }
            if !seen.insert((*emoji).to_string()) {
                continue;
            }
            shown.push(Shown {
                text: (*emoji).to_string(),
                name: name.clone(),
                recent: false,
            });
        }

        self.shown = shown;
        self.recent_count = recent_count;
        self.selected = 0;
        self.moved = true;
    }

    fn pick(&mut self, ctx: &egui::Context, text: String) {
        let mut recents = Self::load_recents();
        recents.retain(|r| r != &text);
        recents.insert(0, text.clone());
        recents.truncate(MAX_RECENTS);
        Self::save_recents(&recents);

        *self.chosen.lock().unwrap() = Some(text);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for EmojiApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui_common::clear_color_for_theme()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let p = Palette::current();

        // Grab key state before any widget can consume the events.
        let (esc, enter, up, down, left, right) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });

        if esc {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let len = self.shown.len();
        if len > 0 {
            let mut sel = self.selected;
            if right {
                sel = (sel + 1).min(len - 1);
            }
            if left {
                sel = sel.saturating_sub(1);
            }
            if down {
                sel = (sel + COLS).min(len - 1);
            }
            if up {
                sel = sel.saturating_sub(COLS);
            }
            if sel != self.selected {
                self.selected = sel;
                self.moved = true;
            }
            // Arrows are ours — don't let the search field move its caret.
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
            });

            if enter {
                let text = self.shown[self.selected].text.clone();
                self.pick(ctx, text);
                return;
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui_common::floating_shell(ui, "Emoji", ctx, |ui| {
                    // Search
                    ui_common::search_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Search").size(12.0).color(p.text_muted));
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .hint_text("Type to filter…")
                                    .font(egui::TextStyle::Heading)
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY),
                            );
                            if !self.focused_once {
                                resp.request_focus();
                                self.focused_once = true;
                            }
                            if resp.changed() {
                                self.refilter();
                            }
                        });
                    });
                    ui.add_space(8.0);

                    // Grid fills remaining height above the footer.
                    let grid_h = (ui.available_height() - 48.0).max(80.0);
                    egui::ScrollArea::vertical()
                        .max_height(grid_h)
                        .min_scrolled_height(grid_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.shown.is_empty() {
                                ui.add_space(28.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new("🍁").size(40.0));
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("No emojis match")
                                            .size(15.0)
                                            .color(p.text),
                                    );
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        ui.add_space(
                                            (ui.available_width() - 220.0).max(0.0) / 2.0,
                                        );
                                        for chip in ["coffee", "heart", "maple", "donut"] {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(chip)
                                                            .size(12.0)
                                                            .color(p.text),
                                                    )
                                                    .fill(p.bg_shade)
                                                    .stroke(Stroke::new(1.0, p.border))
                                                    .corner_radius(CornerRadius::same(14)),
                                                )
                                                .clicked()
                                            {
                                                self.search = chip.to_string();
                                                self.refilter();
                                            }
                                        }
                                    });
                                });
                                return;
                            }

                            let mut i = 0;
                            if self.recent_count > 0 && self.search.trim().is_empty() {
                                ui.label(
                                    RichText::new("RECENT")
                                        .size(11.0)
                                        .color(p.accent)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                                self.draw_grid_range(ui, ctx, &p, 0, self.recent_count);
                                i = self.recent_count;
                                if i < self.shown.len() {
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("ALL")
                                            .size(11.0)
                                            .color(p.text_muted)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                }
                            }

                            if i < self.shown.len() {
                                self.draw_grid_range(ui, ctx, &p, i, self.shown.len());
                            }
                        });

                    ui.add_space(6.0);
                    // Footer: selection preview + clean ASCII hints (no tofu boxes).
                    ui.horizontal(|ui| {
                        if let Some(s) = self.shown.get(self.selected) {
                            let text = s.text.clone();
                            let name = s.name.clone();
                            let recent = s.recent;
                            if let Some(tex) = self.atlas.texture(ctx, &text) {
                                ui.add(
                                    egui::Image::new(egui::load::SizedTexture::new(
                                        tex.id(),
                                        Vec2::splat(28.0),
                                    ))
                                    .sense(Sense::hover()),
                                );
                            }
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&name)
                                        .size(13.0)
                                        .color(p.text)
                                        .strong(),
                                );
                                ui.horizontal(|ui| {
                                    if recent {
                                        ui.label(
                                            RichText::new("Recent")
                                                .size(11.0)
                                                .color(p.accent),
                                        );
                                        ui.label(
                                            RichText::new("·")
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(format!("{} emojis", self.shown.len()))
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                            });
                        } else {
                            ui.label(
                                RichText::new(format!("{} emojis", self.shown.len()))
                                    .size(12.0)
                                    .color(p.text_muted),
                            );
                        }
                    });
                    ui.add_space(4.0);
                    ui_common::footer_hints(ui, "");
                });
            });

        self.moved = false;
    }
}

impl EmojiApp {
    fn draw_grid_range(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        p: &Palette,
        start: usize,
        end: usize,
    ) {
        egui::Grid::new(format!("emoji_grid_{start}"))
            .num_columns(COLS)
            .spacing([6.0, 6.0])
            .show(ui, |ui| {
                for i in start..end {
                    let (text, name) = {
                        let s = &self.shown[i];
                        (s.text.clone(), s.name.clone())
                    };
                    let selected = i == self.selected;
                    let fill = if selected {
                        p.selection
                    } else {
                        Color32::TRANSPARENT
                    };
                    let stroke = if selected {
                        Stroke::new(1.5, p.accent)
                    } else {
                        Stroke::new(1.0, Color32::TRANSPARENT)
                    };

                    // Prefer colour bitmap from Noto Color Emoji; fall back to mono text.
                    let tex = self.atlas.texture(ctx, &text);
                    let resp = if let Some(tex) = tex {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::click());
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(8),
                            fill,
                            stroke,
                            egui::StrokeKind::Inside,
                        );
                        let img_rect = egui::Rect::from_center_size(
                            rect.center(),
                            Vec2::splat(EMOJI_IMG),
                        );
                        egui::Image::new(egui::load::SizedTexture::new(tex.id(), img_rect.size()))
                            .paint_at(ui, img_rect);
                        resp.on_hover_text(&name)
                    } else {
                        let button = egui::Button::new(RichText::new(&text).size(EMOJI_SIZE))
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(8));
                        ui.add_sized([CELL, CELL], button).on_hover_text(&name)
                    };

                    if selected && self.moved {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                    if resp.clicked() {
                        self.pick(ctx, text);
                    }
                    if (i - start + 1) % COLS == 0 {
                        ui.end_row();
                    }
                }
            });
    }
}

/// Open the emoji picker; returns the chosen emoji (if any).
pub fn run() -> Result<Option<String>> {
    let chosen = Arc::new(Mutex::new(None));
    let chosen_in_app = chosen.clone();
    let options = ui_common::native_options("Timbits — Emoji", 480.0, 560.0);

    eframe::run_native(
        "timbits-emoji",
        options,
        Box::new(move |cc| {
            ui_common::apply_fonts(&cc.egui_ctx);
            ui_common::apply_theme(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Focus);
            Ok(Box::new(EmojiApp::new(chosen_in_app)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("emoji picker failed: {e}"))?;

    Ok(chosen.lock().unwrap().clone())
}
