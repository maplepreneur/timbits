//! Settings window: General + Pasting tabs.

use anyhow::Result;
use eframe::egui::{self, RichText, ScrollArea};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::{self, Config, PasteMethod, SkinTone};
use crate::emoji_update::{self, UpdateReport};
use crate::gnome_hotkeys;
use crate::ui_common::{self, Palette};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Pasting,
}

struct SettingsApp {
    cfg: Config,
    tab: Tab,
    status: String,
    status_ok: bool,
    /// Background emoji catalogue download in flight.
    emoji_update_busy: bool,
    emoji_update_rx: Option<mpsc::Receiver<Result<UpdateReport, String>>>,
    _done: Arc<Mutex<bool>>,
}

impl SettingsApp {
    fn new(done: Arc<Mutex<bool>>) -> Self {
        let cfg = Config::load().unwrap_or_default();
        Self {
            cfg,
            tab: Tab::General,
            status: String::new(),
            status_ok: true,
            emoji_update_busy: false,
            emoji_update_rx: None,
            _done: done,
        }
    }

    fn start_emoji_update(&mut self) {
        if self.emoji_update_busy {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.emoji_update_busy = true;
        self.emoji_update_rx = Some(rx);
        self.status = "Updating emoji catalogue… (downloads from unicode.org)".into();
        self.status_ok = true;
        thread::spawn(move || {
            let result = emoji_update::update_user_catalogue()
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }

    fn poll_emoji_update(&mut self) {
        let Some(rx) = self.emoji_update_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(report)) => {
                self.emoji_update_busy = false;
                self.emoji_update_rx = None;
                self.status = format!(
                    "Emoji catalogue updated: Unicode {} · {} emoji ({} with keywords)\n{}",
                    report.version,
                    report.count,
                    report.with_keywords,
                    report.json_path.display()
                );
                self.status_ok = true;
            }
            Ok(Err(e)) => {
                self.emoji_update_busy = false;
                self.emoji_update_rx = None;
                self.status = format!("Emoji update failed: {e}");
                self.status_ok = false;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.emoji_update_busy = false;
                self.emoji_update_rx = None;
                self.status = "Emoji update failed: worker stopped".into();
                self.status_ok = false;
            }
        }
    }

    fn save(&mut self) {
        match self.cfg.save() {
            Ok(()) => {
                self.status = format!("Saved {}", config::config_path().display());
                self.status_ok = true;
            }
            Err(e) => {
                self.status = format!("Save failed: {e:#}");
                self.status_ok = false;
            }
        }
    }

    fn apply_hotkeys(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.status = format!("Could not save config: {e:#}");
            self.status_ok = false;
            return;
        }
        match gnome_hotkeys::install(&self.cfg) {
            Ok(true) => {
                self.status = "GNOME hotkeys updated.".into();
                self.status_ok = true;
            }
            Ok(false) => {
                self.status = "Saved (gsettings not available).".into();
                self.status_ok = true;
            }
            Err(e) => {
                self.status = format!("Hotkey apply failed: {e:#}");
                self.status_ok = false;
            }
        }
    }

    fn open_config_dir(&self) {
        let _ = std::process::Command::new("xdg-open")
            .arg(config::config_dir())
            .spawn();
    }

    fn move_method(&mut self, idx: usize, up: bool) {
        let n = self.cfg.paste_methods.len();
        if n == 0 {
            return;
        }
        if up {
            if idx == 0 {
                return;
            }
            self.cfg.paste_methods.swap(idx, idx - 1);
        } else {
            if idx + 1 >= n {
                return;
            }
            self.cfg.paste_methods.swap(idx, idx + 1);
        }
    }

    fn reset_paste_defaults(&mut self) {
        let d = Config::default();
        self.cfg.paste_hotkey = d.paste_hotkey;
        self.cfg.paste_hotkey_terminal = d.paste_hotkey_terminal;
        self.cfg.paste_auto_terminal = d.paste_auto_terminal;
        self.cfg.paste_focus_delay_ms = d.paste_focus_delay_ms;
        self.cfg.paste_ydotool_delay_ms = d.paste_ydotool_delay_ms;
        self.cfg.paste_ydotool_key_delay_ms = d.paste_ydotool_key_delay_ms;
        self.cfg.paste_uinput_settle_ms = d.paste_uinput_settle_ms;
        self.cfg.paste_verify_clipboard = d.paste_verify_clipboard;
        self.cfg.paste_also_primary = d.paste_also_primary;
        self.cfg.paste_methods = d.paste_methods;
        self.status = "Paste settings reset to defaults (not saved yet).".into();
        self.status_ok = true;
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui_common::clear_color_for_theme()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let p = Palette::current();

        self.poll_emoji_update();
        if self.emoji_update_busy {
            // Keep the UI responsive while curl/wget downloads run.
            ctx.request_repaint();
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.emoji_update_busy {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                // Real GNOME title bar via decorations; no custom drag header.
                ui_common::app_shell(ui, |ui| {
                    // Tabs
                    ui.horizontal(|ui| {
                        tab_button(ui, &p, "General", self.tab == Tab::General, || {
                            self.tab = Tab::General;
                        });
                        tab_button(ui, &p, "Pasting", self.tab == Tab::Pasting, || {
                            self.tab = Tab::Pasting;
                        });
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.tab {
                            Tab::General => self.ui_general(ui, &p),
                            Tab::Pasting => self.ui_pasting(ui, &p),
                        });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Save").color(p.accent_fg).strong(),
                                )
                                .fill(p.accent),
                            )
                            .clicked()
                        {
                            self.save();
                        }
                        if self.tab == Tab::General
                            && ui.button("Save & apply GNOME hotkeys").clicked()
                        {
                            self.apply_hotkeys();
                        }
                        if ui.button("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    if !self.status.is_empty() {
                        ui.add_space(8.0);
                        let color = if self.status_ok {
                            p.accent
                        } else {
                            egui::Color32::from_rgb(0xe6, 0x2d, 0x42)
                        };
                        ui.label(RichText::new(&self.status).size(13.0).color(color));
                    }
                });
            });
    }
}

impl SettingsApp {
    fn ui_general(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(
            RichText::new("General")
                .size(16.0)
                .color(p.text)
                .strong(),
        );
        ui.add_space(4.0);
        ui_common::muted_label(
            ui,
            "Hotkeys on GNOME/Zorin can be applied via gsettings. keyd users should also bind Super+. / Super+Shift+C in keyd.",
        );
        ui.add_space(10.0);

        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Hotkeys").size(14.0).strong().color(p.accent));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new("Emoji picker").color(p.text));
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.emoji_hotkey)
                        .desired_width(220.0)
                        .hint_text("Super+Period"),
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new("Clipboard history").color(p.text));
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.clipboard_hotkey)
                        .desired_width(220.0)
                        .hint_text("Super+Shift+C"),
                );
            });
        });

        ui.add_space(10.0);
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Clipboard history")
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Max entries").color(p.text));
                ui.add(
                    egui::DragValue::new(&mut self.cfg.max_entries)
                        .range(50..=5000)
                        .speed(10.0),
                );
            });
            ui.add_space(6.0);
            ui.checkbox(
                &mut self.cfg.ocr_enabled,
                RichText::new("OCR images (tesseract) for search").color(p.text),
            );
        });

        ui.add_space(10.0);
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Emoji skin tone")
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );
            ui.add_space(4.0);
            ui_common::muted_label(
                ui,
                "The picker lists each emoji once (no five-tone rows). Choose a \
                 preferred tone to paste people and hand gestures in that tone.",
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for tone in SkinTone::ALL {
                    let selected = self.cfg.emoji_skin_tone == *tone;
                    let fill = if selected { p.selection } else { p.bg_shade };
                    let stroke = if selected {
                        egui::Stroke::new(1.5, p.accent)
                    } else {
                        egui::Stroke::new(1.0, p.border)
                    };
                    let label = format!("{} {}", tone.sample(), tone.label());
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(13.0).color(p.text))
                                .fill(fill)
                                .stroke(stroke)
                                .corner_radius(egui::CornerRadius::same(10)),
                        )
                        .clicked()
                    {
                        self.cfg.emoji_skin_tone = *tone;
                    }
                }
            });
        });

        ui.add_space(10.0);
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Emoji catalogue")
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );
            ui.add_space(4.0);
            ui_common::muted_label(
                ui,
                "Download the latest Unicode emoji list and search keywords \
                 (needs network + curl or wget). The picker uses the new data \
                 on next open — no reinstall required.",
            );
            ui.add_space(8.0);
            let label = if self.emoji_update_busy {
                "Updating emoji catalogue…"
            } else {
                "⬇  Update emoji catalogue"
            };
            let btn = if self.emoji_update_busy {
                egui::Button::new(RichText::new(label).color(p.text_muted))
                    .fill(p.bg_shade)
            } else {
                egui::Button::new(RichText::new(label).color(p.accent_fg).strong())
                    .fill(p.accent)
            };
            if ui
                .add_enabled(!self.emoji_update_busy, btn)
                .on_hover_text("Fetches unicode.org emoji-test + emojilib keywords")
                .clicked()
            {
                self.start_emoji_update();
            }
            ui.add_space(4.0);
            let ver = std::fs::read_to_string(config::data_dir().join("emojis.version"))
                .unwrap_or_else(|_| "(bundled)".into());
            ui_common::muted_label(
                ui,
                format!("Installed catalogue version: {}", ver.trim()),
            );
        });

        ui.add_space(10.0);
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Paths").size(14.0).strong().color(p.accent));
            ui.add_space(6.0);
            ui_common::muted_label(ui, format!("Config: {}", config::config_path().display()));
            ui_common::muted_label(ui, format!("Data:   {}", config::data_dir().display()));
            ui.add_space(6.0);
            if ui.button("Open config folder").clicked() {
                self.open_config_dir();
            }
        });
    }

    fn ui_pasting(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(
            RichText::new("Pasting")
                .size(16.0)
                .color(p.text)
                .strong(),
        );
        ui.add_space(4.0);
        ui_common::muted_label(
            ui,
            "After you pick an emoji or history item, Timbits copies it with wl-copy, \
             waits briefly for focus, then tries paste methods in order until one works.",
        );
        ui.add_space(10.0);

        // Custom paste hotkeys (generic defaults: Ctrl+V / Ctrl+Shift+V)
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Paste hotkeys")
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );
            ui.add_space(4.0);
            ui_common::muted_label(
                ui,
                "Chord injected after copy. Defaults work for most desktops. \
                 If you use keyd Super+V for paste, set Primary to Super+V.",
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.set_min_width(150.0);
                ui.label(RichText::new("Primary").color(p.text));
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.paste_hotkey)
                        .desired_width(180.0)
                        .hint_text("Ctrl+V"),
                );
            });
            ui.label(
                RichText::new("Used for normal apps (and always for *Primary* methods).")
                    .size(11.0)
                    .color(p.text_muted),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.set_min_width(150.0);
                ui.label(RichText::new("Terminal").color(p.text));
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.paste_hotkey_terminal)
                        .desired_width(180.0)
                        .hint_text("Ctrl+Shift+V"),
                );
            });
            ui.label(
                RichText::new(
                    "Used when Auto methods detect a terminal (Ghostty, etc.).",
                )
                .size(11.0)
                .color(p.text_muted),
            );
            ui.add_space(6.0);

            ui.checkbox(
                &mut self.cfg.paste_auto_terminal,
                RichText::new("Auto-detect terminals for *Auto* methods").color(p.text),
            );
            ui.add_space(4.0);
            ui_common::muted_label(
                ui,
                "Examples: Ctrl+V, Super+V, Ctrl+Shift+V, Alt+V  ·  modifiers: Ctrl Shift Alt Super",
            );
        });

        ui.add_space(10.0);

        // Timing
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Speed / timing").size(14.0).strong().color(p.accent));
            ui.add_space(6.0);
            ui_common::muted_label(
                ui,
                "Lower delays = faster paste. If paste lands in the wrong window, raise Focus delay.",
            );
            ui.add_space(8.0);

            row_ms(
                ui,
                p,
                "Focus delay",
                "Wait after picker closes (ms). Default 60.",
                &mut self.cfg.paste_focus_delay_ms,
                0,
                400,
            );
            row_ms(
                ui,
                p,
                "ydotool pre-delay",
                "ydotool --delay before the chord (ms). 0 is fastest.",
                &mut self.cfg.paste_ydotool_delay_ms,
                0,
                200,
            );
            row_ms(
                ui,
                p,
                "ydotool key delay",
                "Gap between key down/up events (ms).",
                &mut self.cfg.paste_ydotool_key_delay_ms,
                0,
                40,
            );
            row_ms(
                ui,
                p,
                "uinput settle",
                "Wait after creating virtual keyboard so keyd can grab it (ms).",
                &mut self.cfg.paste_uinput_settle_ms,
                0,
                250,
            );

            ui.add_space(6.0);
            ui.checkbox(
                &mut self.cfg.paste_also_primary,
                RichText::new("Also set primary selection (wl-copy --primary)").color(p.text),
            );
            ui.checkbox(
                &mut self.cfg.paste_verify_clipboard,
                RichText::new("Verify clipboard with wl-paste before inject (slower)")
                    .color(p.text),
            );
        });

        ui.add_space(10.0);

        // Method order
        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Method priority")
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );
            ui.add_space(4.0);
            ui_common::muted_label(
                ui,
                "Tried top-to-bottom; first success wins. Use Up/Down to reorder. \
                 Toggle to include/exclude a method.",
            );
            ui.add_space(8.0);

            // Active ordered list
            let n = self.cfg.paste_methods.len();
            let mut remove_at: Option<usize> = None;
            let mut move_at: Option<(usize, bool)> = None;
            for i in 0..n {
                let method = self.cfg.paste_methods[i];
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}.", i + 1))
                            .size(12.0)
                            .color(p.text_muted)
                            .strong(),
                    );
                    ui.label(RichText::new(method.label()).size(13.0).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(i + 1 < n, egui::Button::new("Down"))
                            .clicked()
                        {
                            move_at = Some((i, false));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                            move_at = Some((i, true));
                        }
                        if ui.button("Remove").clicked() {
                            remove_at = Some(i);
                        }
                    });
                });
                ui.label(
                    RichText::new(method.description())
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.add_space(6.0);
            }
            if let Some(i) = remove_at {
                if i < self.cfg.paste_methods.len() {
                    self.cfg.paste_methods.remove(i);
                }
            } else if let Some((i, up)) = move_at {
                self.move_method(i, up);
            }

            if self.cfg.paste_methods.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(0xe6, 0x2d, 0x42),
                    "No methods enabled — paste will only set the clipboard.",
                );
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(RichText::new("Add method").size(12.0).strong().color(p.text));
            ui.add_space(4.0);

            for &m in PasteMethod::ALL {
                let already = self.cfg.paste_methods.contains(&m);
                ui.horizontal(|ui| {
                    let btn = ui.add_enabled(!already, egui::Button::new(format!("+ {}", m.label())));
                    if btn.clicked() {
                        self.cfg.paste_methods.push(m);
                    }
                });
                ui.label(
                    RichText::new(m.description())
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.add_space(4.0);
            }
        });

        ui.add_space(10.0);

        ui_common::card_frame(false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("How it works").size(14.0).strong().color(p.accent));
            ui.add_space(6.0);
            for line in [
                "1. Content is placed on the clipboard with wl-copy (not typed character-by-character).",
                "2. Focus delay lets the previous app accept keys after the picker closes.",
                "3. Each backend injects your configured paste hotkey (default Ctrl+V).",
                "4. *Auto* methods switch to the terminal hotkey (default Ctrl+Shift+V) for terminals.",
                "5. keyd users: set Primary to Super+V if that is how your desktop pastes.",
                "6. Logs: ~/.local/share/timbits/paste.log",
                "Requires: wl-copy; ydotoold for ydotool methods; membership in group input for uinput.",
            ] {
                ui.label(RichText::new(line).size(12.0).color(p.text_muted));
                ui.add_space(2.0);
            }
            ui.add_space(8.0);
            if ui.button("Reset paste settings to defaults").clicked() {
                self.reset_paste_defaults();
            }
        });
    }
}

fn tab_button(
    ui: &mut egui::Ui,
    p: &Palette,
    label: &str,
    active: bool,
    on_click: impl FnOnce(),
) {
    let fill = if active { p.accent } else { p.bg_shade };
    let text_c = if active { p.accent_fg } else { p.text };
    if ui
        .add(
            egui::Button::new(RichText::new(label).color(text_c).strong())
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .clicked()
    {
        on_click();
    }
}

fn row_ms(
    ui: &mut egui::Ui,
    p: &Palette,
    label: &str,
    hint: &str,
    value: &mut u64,
    min: u64,
    max: u64,
) {
    ui.horizontal(|ui| {
        ui.set_min_width(150.0);
        ui.label(RichText::new(label).color(p.text));
        ui.add(egui::DragValue::new(value).range(min..=max).suffix(" ms"));
    });
    ui.label(RichText::new(hint).size(11.0).color(p.text_muted));
    ui.add_space(6.0);
}

/// Open the settings window.
pub fn run() -> Result<()> {
    let done = Arc::new(Mutex::new(false));
    let done_in = done.clone();
    // Decorated window so GNOME Shell draws the real themed title bar (CSD).
    let options = ui_common::app_window_options("Timbits Settings", 560.0, 640.0);

    eframe::run_native(
        "timbits-settings",
        options,
        Box::new(move |cc| {
            ui_common::apply_fonts(&cc.egui_ctx);
            ui_common::apply_theme(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Focus);
            Ok(Box::new(SettingsApp::new(done_in)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("settings window failed: {e}"))?;

    Ok(())
}
