//! Clipboard history window: search field focused on open, Up/Down to
//! navigate, Enter to paste, Ctrl+Delete to forget an entry, Esc to cancel.
//!
//! The window is focused when it opens, which on GNOME Wayland is the one
//! moment clipboard reads are allowed — so it ingests the current clipboard
//! on startup (continuous watching only works on X11 / wlroots / KDE).

use anyhow::Result;
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::clip;
use crate::config::{self, Config};
use crate::storage::{Entry, EntryKind, Store};
use crate::ui_common::{self, Palette};

const RESULT_LIMIT: usize = 300;
const PAGE_STEP: usize = 12;
const THUMB: f32 = 36.0;
const LIST_WIDTH: f32 = 340.0;
/// Fixed row height for virtualized list (card + spacing).
const ROW_HEIGHT: f32 = 56.0;
const WIN_W: f32 = 720.0;
const WIN_H: f32 = 560.0;

struct HistoryApp {
    store: Store,
    search: String,
    entries: Vec<Entry>,
    selected: usize,
    chosen: Arc<Mutex<Option<Entry>>>,
    thumbs: HashMap<i64, egui::TextureHandle>,
    moved: bool,
    focused_once: bool,
    /// Wayland/Mutter often ignores pre-map position; re-center once mapped.
    positioned: bool,
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
            positioned: false,
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
        self.load_thumb_path(ctx, entry.id, entry.image_path.as_deref())
    }

    fn load_thumb_path(
        &mut self,
        ctx: &egui::Context,
        id: i64,
        path: Option<&str>,
    ) -> Option<egui::TextureHandle> {
        if let Some(h) = self.thumbs.get(&id) {
            return Some(h.clone());
        }
        let path = path?;
        let rgba = image::open(path).ok()?.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let handle = ctx.load_texture(
            format!("thumb-{id}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.thumbs.insert(id, handle.clone());
        Some(handle)
    }
}

impl eframe::App for HistoryApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui_common::clear_color_for_theme()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let p = Palette::current();

        if !self.positioned {
            ui_common::reapply_pointer_monitor_center(ctx, WIN_W, WIN_H);
            self.positioned = true;
        }

        // Grab key state before any widget can consume the events.
        // num_presses includes OS key-repeat so holding an arrow feels snappy.
        let (esc, enter, up_n, down_n, pgup_n, pgdn_n, ctrl_delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.num_presses(egui::Key::ArrowUp),
                i.num_presses(egui::Key::ArrowDown),
                i.num_presses(egui::Key::PageUp),
                i.num_presses(egui::Key::PageDown),
                i.modifiers.ctrl && i.key_pressed(egui::Key::Delete),
            )
        });

        if esc {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let len = self.entries.len();
        if len > 0 {
            let mut sel = self.selected as isize;
            if down_n > 0 {
                sel = (sel + down_n as isize).min(len as isize - 1);
            }
            if up_n > 0 {
                sel = (sel - up_n as isize).max(0);
            }
            if pgdn_n > 0 {
                sel = (sel + (pgdn_n * PAGE_STEP) as isize).min(len as isize - 1);
            }
            if pgup_n > 0 {
                sel = (sel - (pgup_n * PAGE_STEP) as isize).max(0);
            }
            let sel = sel as usize;
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

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui_common::floating_shell(ui, |ui| {
                    // Search
                    ui_common::search_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Search").size(12.0).color(p.text_muted));
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .hint_text("Filter history (OCR included)…")
                                    .font(egui::TextStyle::Heading)
                                    .frame(egui::Frame::NONE)
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
                        });
                    });
                    ui.add_space(8.0);

                    let body_h = (ui.available_height() - 36.0).max(160.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(body_h);
                        // List
                        ui.allocate_ui_with_layout(
                            Vec2::new(LIST_WIDTH, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                if self.entries.is_empty() {
                                    ui.add_space(40.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new("🍩").size(36.0));
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new("Nothing here yet")
                                                .size(15.0)
                                                .color(p.text),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(
                                                "Copy text, files, or a screenshot —\nit’ll land here.",
                                            )
                                            .size(12.0)
                                            .color(p.text_muted),
                                        );
                                    });
                                } else {
                                    // Virtualized rows: only paint visible cards (keeps arrows snappy).
                                    let n = self.entries.len();
                                    let selected = self.selected;
                                    let moved = self.moved;
                                    let mut clicked: Option<usize> = None;
                                    let mut double: Option<usize> = None;

                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show_rows(ui, ROW_HEIGHT, n, |ui, row_range| {
                                            for i in row_range {
                                                // Copy only small display fields (not full body text).
                                                let (id, kind, preview, use_count, last_used_at, path) = {
                                                    let e = &self.entries[i];
                                                    (
                                                        e.id,
                                                        e.kind,
                                                        truncate_chars(&e.preview, 60),
                                                        e.use_count,
                                                        e.last_used_at,
                                                        e.image_path.clone(),
                                                    )
                                                };
                                                let is_sel = i == selected;
                                                let thumb = if kind == EntryKind::Image {
                                                    self.load_thumb_path(
                                                        ctx,
                                                        id,
                                                        path.as_deref(),
                                                    )
                                                } else {
                                                    None
                                                };

                                                let resp = ui_common::card_frame(is_sel)
                                                    .show(ui, |ui| {
                                                        ui.set_min_width(ui.available_width());
                                                        ui.set_height(ROW_HEIGHT - 6.0);
                                                        ui.horizontal(|ui| {
                                                            if is_sel {
                                                                let (rect, _) = ui
                                                                    .allocate_exact_size(
                                                                        Vec2::new(3.0, THUMB),
                                                                        Sense::hover(),
                                                                    );
                                                                ui.painter().rect_filled(
                                                                    rect, 2.0, p.accent,
                                                                );
                                                                ui.add_space(4.0);
                                                            }

                                                            if let Some(handle) = thumb {
                                                                let size = Vec2::splat(THUMB);
                                                                ui.add(
                                                                    egui::Image::new(
                                                                        egui::load::SizedTexture::new(
                                                                            handle.id(),
                                                                            size,
                                                                        ),
                                                                    )
                                                                    .corner_radius(6.0)
                                                                    .bg_fill(p.bg_shade),
                                                                );
                                                            } else {
                                                                let (rect, _) = ui
                                                                    .allocate_exact_size(
                                                                        Vec2::splat(THUMB),
                                                                        Sense::hover(),
                                                                    );
                                                                ui.painter().rect_filled(
                                                                    rect, 6.0, p.hover,
                                                                );
                                                                ui.painter().text(
                                                                    rect.center(),
                                                                    egui::Align2::CENTER_CENTER,
                                                                    ui_common::kind_icon(kind),
                                                                    egui::FontId::proportional(18.0),
                                                                    p.text,
                                                                );
                                                            }

                                                            ui.add_space(6.0);

                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(preview)
                                                                        .size(13.0)
                                                                        .color(p.text)
                                                                        .strong(),
                                                                );
                                                                ui.horizontal(|ui| {
                                                                    ui.label(
                                                                        RichText::new(
                                                                            ui_common::kind_label(
                                                                                kind,
                                                                            ),
                                                                        )
                                                                        .size(11.0)
                                                                        .color(p.text_muted),
                                                                    );
                                                                    if use_count > 1 {
                                                                        ui.label(
                                                                            RichText::new(format!(
                                                                                "· used {use_count}×"
                                                                            ))
                                                                            .size(11.0)
                                                                            .color(p.text_muted),
                                                                        );
                                                                    }
                                                                });
                                                            });

                                                            ui.with_layout(
                                                                Layout::right_to_left(Align::Center),
                                                                |ui| {
                                                                    ui.label(
                                                                        RichText::new(
                                                                            ui_common::rel_time(
                                                                                last_used_at,
                                                                            ),
                                                                        )
                                                                        .size(11.0)
                                                                        .color(p.text_muted),
                                                                    );
                                                                },
                                                            );
                                                        });
                                                    })
                                                    .response
                                                    .interact(Sense::click());

                                                if is_sel && moved {
                                                    resp.scroll_to_me(Some(Align::Center));
                                                }
                                                if resp.clicked() {
                                                    clicked = Some(i);
                                                }
                                                if resp.double_clicked() {
                                                    double = Some(i);
                                                }
                                            }
                                        });

                                    if let Some(i) = double {
                                        let entry = self.entries[i].clone();
                                        self.choose(ctx, entry);
                                        return;
                                    }
                                    if let Some(i) = clicked {
                                        self.selected = i;
                                        self.moved = true;
                                    }
                                }
                            },
                        );

                        ui.separator();

                        // Preview
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                let Some(entry) = self.entries.get(self.selected).cloned() else {
                                    return;
                                };

                                ui_common::preview_frame().show(ui, |ui| {
                                    ui.set_min_height(body_h - 8.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} {}",
                                                ui_common::kind_icon(entry.kind),
                                                ui_common::kind_label(entry.kind)
                                            ))
                                            .size(13.0)
                                            .color(p.text)
                                            .strong(),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(ui_common::rel_time(
                                                        entry.last_used_at,
                                                    ))
                                                    .size(12.0)
                                                    .color(p.text_muted),
                                                );
                                                if entry.use_count > 0 {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "used {}×",
                                                            entry.use_count
                                                        ))
                                                        .size(12.0)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.separator();
                                                }
                                            },
                                        );
                                    });
                                    ui.add_space(6.0);
                                    ui.separator();
                                    ui.add_space(6.0);

                                    match entry.kind {
                                        EntryKind::Image => {
                                            if let Some(handle) = self.load_thumb(ctx, &entry) {
                                                let avail = ui.available_size();
                                                let [w, h] = [
                                                    handle.size()[0] as f32,
                                                    handle.size()[1] as f32,
                                                ];
                                                let scale = (avail.x / w)
                                                    .min(avail.y * 0.7 / h)
                                                    .min(1.0)
                                                    .max(0.05);
                                                let size = egui::vec2(w * scale, h * scale);
                                                ui.centered_and_justified(|ui| {
                                                    ui.add(
                                                        egui::Image::new(
                                                            egui::load::SizedTexture::new(
                                                                handle.id(),
                                                                size,
                                                            ),
                                                        )
                                                        .corner_radius(8.0)
                                                        .bg_fill(p.bg_shade),
                                                    );
                                                });
                                            } else {
                                                ui.label(
                                                    RichText::new("(image file missing)")
                                                        .color(p.text_muted),
                                                );
                                            }
                                            if let Some(ocr) = &entry.ocr_text {
                                                ui.add_space(10.0);
                                                ui.label(
                                                    RichText::new("Text in image")
                                                        .size(12.0)
                                                        .color(p.accent)
                                                        .strong(),
                                                );
                                                ui.add_space(4.0);
                                                egui::ScrollArea::vertical().show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new(ocr)
                                                            .size(13.0)
                                                            .color(p.text_muted),
                                                    );
                                                });
                                            }
                                        }
                                        EntryKind::Text | EntryKind::Files => {
                                            if let Some(text) = &entry.text {
                                                egui::ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .show(ui, |ui| {
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(text)
                                                                    .size(13.0)
                                                                    .color(p.text),
                                                            )
                                                            .wrap()
                                                            .selectable(true),
                                                        );
                                                    });
                                            }
                                        }
                                    }
                                });
                            },
                        );
                    });

                    ui.add_space(4.0);
                    ui_common::footer_hints(
                        ui,
                        &format!(
                            "{} items  ·  Ctrl+Del forget",
                            self.entries.len()
                        ),
                    );
                });
            });

        self.moved = false;
    }
}

/// Truncate to `max` Unicode scalars without panicking on multi-byte chars.
fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Open the clipboard history picker; returns the chosen entry (if any).
pub fn run() -> Result<Option<Entry>> {
    let chosen = Arc::new(Mutex::new(None));
    let chosen_in_app = chosen.clone();
    let options = ui_common::native_options("Timbits — Clipboard", WIN_W, WIN_H);

    eframe::run_native(
        "timbits-clipboard",
        options,
        Box::new(move |cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
            ui_common::apply_fonts(&cc.egui_ctx);
            ui_common::apply_theme(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Focus);
            Ok(Box::new(HistoryApp::new(chosen_in_app)?))
        }),
    )
    .map_err(|e| anyhow::anyhow!("clipboard picker failed: {e}"))?;

    Ok(chosen.lock().unwrap().clone())
}
