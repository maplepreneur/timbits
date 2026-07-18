//! Clipboard history window: search field focused on open, Up/Down to
//! navigate, Enter to paste, Ctrl+Delete to forget an entry, Esc to cancel.
//!
//! The window is focused when it opens, which on GNOME Wayland is the one
//! moment clipboard reads are allowed — so it ingests the current clipboard
//! on startup (continuous watching only works on X11 / wlroots / KDE).

use anyhow::Result;
use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::clip;
use crate::config::{self, Config};
use crate::storage::{Entry, EntryKind, Store};
use crate::ui_common;

const RESULT_LIMIT: usize = 300;
const PAGE_STEP: usize = 12;

struct HistoryApp {
    store: Store,
    search: String,
    entries: Vec<Entry>,
    selected: usize,
    chosen: Arc<Mutex<Option<Entry>>>,
    thumbs: HashMap<i64, egui::TextureHandle>,
    moved: bool,
    focused_once: bool,
}

impl HistoryApp {
    fn new(chosen: Arc<Mutex<Option<Entry>>>) -> Result<Self> {
        let cfg = Config::load().unwrap_or_default();
        let store = Store::open(&config::db_path())?;

        // We're focused right now — grab whatever is on the clipboard.
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Some(content) = clip::read_clipboard(&mut cb) {
                let _ = clip::ingest(&store, &cfg, content);
            }
        }

        let mut app = Self {
            store,
            search: String::new(),
            entries: Vec::new(),
            selected: 0,
            chosen,
            thumbs: HashMap::new(),
            moved: true,
            focused_once: false,
        };
        app.reload();
        Ok(app)
    }

    fn reload(&mut self) {
        self.entries = self
            .store
            .search(&self.search, RESULT_LIMIT)
            .unwrap_or_default();
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn forget_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            let id = entry.id;
            if let Ok(Some(path)) = self.store.delete(id) {
                std::fs::remove_file(path).ok();
            }
            self.thumbs.remove(&id);
            self.reload();
            self.moved = true;
        }
    }

    fn choose(&mut self, ctx: &egui::Context, entry: Entry) {
        *self.chosen.lock().unwrap() = Some(entry);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn load_thumb(&mut self, ctx: &egui::Context, entry: &Entry) -> Option<egui::TextureHandle> {
        if let Some(h) = self.thumbs.get(&entry.id) {
            return Some(h.clone());
        }
        let path = entry.image_path.as_ref()?;
        let rgba = image::open(path).ok()?.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let handle = ctx.load_texture(
            format!("thumb-{}", entry.id),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.thumbs.insert(entry.id, handle.clone());
        Some(handle)
    }
}

impl eframe::App for HistoryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Grab key state before any widget can consume the events.
        let (esc, enter, up, down, pgup, pgdn, ctrl_delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::PageUp),
                i.key_pressed(egui::Key::PageDown),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Delete),
            )
        });

        if esc {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let len = self.entries.len();
        if len > 0 {
            let mut sel = self.selected;
            if down {
                sel = (sel + 1).min(len - 1);
            }
            if up {
                sel = sel.saturating_sub(1);
            }
            if pgdn {
                sel = (sel + PAGE_STEP).min(len - 1);
            }
            if pgup {
                sel = sel.saturating_sub(PAGE_STEP);
            }
            if sel != self.selected {
                self.selected = sel;
                self.moved = true;
            }
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp);
                i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown);
            });

            if enter {
                let entry = self.entries[self.selected].clone();
                self.choose(ctx, entry);
                return;
            }
        }

        if ctrl_delete {
            self.forget_selected();
        }

        egui::Panel::top("search").show(ui, |ui| {
            ui.add_space(8.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search clipboard history (image text included)…")
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            );
            if !self.focused_once {
                resp.request_focus();
                self.focused_once = true;
            }
            if resp.changed() {
                self.selected = 0;
                self.moved = true;
                self.reload();
            }
            ui.add_space(4.0);
        });

        egui::Panel::bottom("hints").show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "↑↓ navigate · Enter paste · Ctrl+Del forget · Esc close — {} items",
                    self.entries.len()
                ))
                .small()
                .weak(),
            );
            ui.add_space(2.0);
        });

        egui::Panel::left("list")
            .default_size(340.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in 0..self.entries.len() {
                            let entry = self.entries[i].clone();
                            let label = format!(
                                "{}  {}   {}",
                                ui_common::kind_icon(entry.kind),
                                entry.preview.chars().take(60).collect::<String>(),
                                ui_common::rel_time(entry.last_used_at),
                            );
                            let selected = i == self.selected;
                            let resp = ui.selectable_label(selected, label);
                            if selected && self.moved {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            if resp.clicked() {
                                self.selected = i;
                            }
                            if resp.double_clicked() {
                                self.choose(ctx, entry);
                                return;
                            }
                        }
                        if self.entries.is_empty() {
                            ui.add_space(20.0);
                            ui.label(
                                egui::RichText::new(
                                    "Nothing here yet.\nCopy some text or a screenshot!",
                                )
                                .weak(),
                            );
                        }
                    });
                self.moved = false;
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let Some(entry) = self.entries.get(self.selected).cloned() else {
                return;
            };
            ui.add_space(6.0);
            match entry.kind {
                EntryKind::Image => {
                    if let Some(handle) = self.load_thumb(ctx, &entry) {
                        let avail = ui.available_size();
                        let [w, h] = [handle.size()[0] as f32, handle.size()[1] as f32];
                        let scale = (avail.x / w).min(avail.y / h).min(1.0).max(0.05);
                        let size = egui::vec2(w * scale, h * scale);
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(
                            handle.id(),
                            size,
                        )));
                    } else {
                        ui.label("(image file missing)");
                    }
                    if let Some(ocr) = &entry.ocr_text {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("OCR text:").small().weak());
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.label(ocr);
                        });
                    }
                }
                EntryKind::Text | EntryKind::Files => {
                    if let Some(text) = &entry.text {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add(
                                egui::Label::new(text)
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                    }
                }
            }
        });
    }
}

/// Open the clipboard history picker; returns the chosen entry (if any).
pub fn run() -> Result<Option<Entry>> {
    let chosen = Arc::new(Mutex::new(None));
    let chosen_in_app = chosen.clone();
    let options = ui_common::native_options("Timbits — Clipboard History", 720.0, 600.0);

    eframe::run_native(
        "timbits-clipboard",
        options,
        Box::new(move |cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
            ui_common::apply_fonts(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Focus);
            Ok(Box::new(HistoryApp::new(chosen_in_app)?))
        }),
    )
    .map_err(|e| anyhow::anyhow!("clipboard picker failed: {e}"))?;

    Ok(chosen.lock().unwrap().clone())
}
