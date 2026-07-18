//! Emoji picker window: search field focused on open, arrow-key navigation
//! over a grid, Enter to paste, Esc to cancel.

use anyhow::Result;
use eframe::egui;
use std::sync::{Arc, Mutex};

use crate::config;
use crate::ui_common;

const COLS: usize = 10;
const MAX_SHOWN: usize = 500;
const MAX_RECENTS: usize = 20;

struct Shown {
    text: String,
    name: String,
}

struct EmojiApp {
    search: String,
    /// (emoji, display name, lowercase search key)
    all: Vec<(&'static str, String, String)>,
    shown: Vec<Shown>,
    selected: usize,
    chosen: Arc<Mutex<Option<String>>>,
    /// Selection moved via keyboard this frame → scroll it into view.
    moved: bool,
    focused_once: bool,
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
            selected: 0,
            chosen,
            moved: false,
            focused_once: false,
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

        if words.is_empty() {
            // Recent emojis first when nothing is typed (deduped against the full list below).
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
                });
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
            });
        }

        self.shown = shown;
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
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

        egui::Panel::top("search").show(ui, |ui| {
            ui.add_space(8.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search emojis…  (↑↓←→ navigate, Enter pastes, Esc closes)")
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            );
            if !self.focused_once {
                resp.request_focus();
                self.focused_once = true;
            }
            if resp.changed() {
                self.refilter();
            }
            ui.add_space(4.0);
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if let Some(s) = self.shown.get(self.selected) {
                    ui.label(egui::RichText::new(&s.text).size(28.0));
                    ui.label(egui::RichText::new(&s.name).size(15.0));
                    ui.separator();
                }
                ui.label(
                    egui::RichText::new(format!("{} emojis", self.shown.len()))
                        .small()
                        .weak(),
                );
            });
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("emoji_grid")
                        .num_columns(COLS)
                        .spacing([6.0, 6.0])
                        .show(ui, |ui| {
                            for i in 0..self.shown.len() {
                                let (text, name) = {
                                    let s = &self.shown[i];
                                    (s.text.clone(), s.name.clone())
                                };
                                let selected = i == self.selected;
                                let button = egui::Button::new(
                                    egui::RichText::new(&text).size(20.0),
                                )
                                .selected(selected);
                                let resp = ui
                                    .add_sized([34.0, 34.0], button)
                                    .on_hover_text(&name);
                                if selected && self.moved {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                                if resp.clicked() {
                                    self.pick(ctx, text);
                                }
                                if (i + 1) % COLS == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                    if self.shown.is_empty() {
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new("No emojis match your search 🍁").weak(),
                        );
                    }
                });
            self.moved = false;
        });
    }
}

/// Open the emoji picker; returns the chosen emoji (if any).
pub fn run() -> Result<Option<String>> {
    let chosen = Arc::new(Mutex::new(None));
    let chosen_in_app = chosen.clone();
    let options = ui_common::native_options("Timbits — Emoji Picker", 480.0, 560.0);

    eframe::run_native(
        "timbits-emoji",
        options,
        Box::new(move |cc| {
            ui_common::apply_fonts(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Focus);
            Ok(Box::new(EmojiApp::new(chosen_in_app)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("emoji picker failed: {e}"))?;

    Ok(chosen.lock().unwrap().clone())
}
