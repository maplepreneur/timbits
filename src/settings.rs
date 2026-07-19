//! Settings window: edit hotkeys, history size, OCR, and re-apply install.

use anyhow::Result;
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

use crate::config::{self, Config};
use crate::gnome_hotkeys;
use crate::ui_common::{self, Palette};

struct SettingsApp {
    cfg: Config,
    status: String,
    status_ok: bool,
    /// Set when the user closes after a successful save (unused for now).
    _done: Arc<Mutex<bool>>,
}

impl SettingsApp {
    fn new(done: Arc<Mutex<bool>>) -> Self {
        let cfg = Config::load().unwrap_or_default();
        Self {
            cfg,
            status: String::new(),
            status_ok: true,
            _done: done,
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
                self.status =
                    "GNOME hotkeys updated. Super+Shift+C / Super+. (and keyd) still apply."
                        .into();
                self.status_ok = true;
            }
            Ok(false) => {
                self.status =
                    "Saved config (gsettings not available — set DE shortcuts manually).".into();
                self.status_ok = true;
            }
            Err(e) => {
                self.status = format!("Hotkey apply failed: {e:#}");
                self.status_ok = false;
            }
        }
    }

    fn open_config_dir(&self) {
        let dir = config::config_dir();
        let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
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

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui_common::floating_shell(ui, "Timbits Settings", ctx, |ui| {
                    ui.label(
                        RichText::new("Preferences")
                            .size(18.0)
                            .color(p.text)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    ui_common::muted_label(
                        ui,
                        "Hotkeys on GNOME/Zorin are applied via gsettings. \
                         If you use keyd, also keep Super+Shift+C / Super+. there.",
                    );
                    ui.add_space(12.0);

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
                        ui.add_space(4.0);
                        ui_common::muted_label(
                            ui,
                            "Examples: Super+Period, Super+Shift+C, Ctrl+Alt+E",
                        );
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
                        ui.label(RichText::new("Paths").size(14.0).strong().color(p.accent));
                        ui.add_space(6.0);
                        ui_common::muted_label(
                            ui,
                            format!("Config: {}", config::config_path().display()),
                        );
                        ui_common::muted_label(
                            ui,
                            format!("Data:   {}", config::data_dir().display()),
                        );
                        ui.add_space(6.0);
                        if ui.button("Open config folder").clicked() {
                            self.open_config_dir();
                        }
                    });

                    ui.add_space(14.0);

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
                        if ui.button("Save & apply GNOME hotkeys").clicked() {
                            self.apply_hotkeys();
                        }
                        if ui.button("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    if !self.status.is_empty() {
                        ui.add_space(10.0);
                        let color = if self.status_ok {
                            p.accent
                        } else {
                            egui::Color32::from_rgb(0xe6, 0x2d, 0x42)
                        };
                        ui.label(RichText::new(&self.status).size(13.0).color(color));
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui_common::footer_hints(ui, "Pickers:");
                });
            });
    }
}

/// Open the settings window.
pub fn run() -> Result<()> {
    let done = Arc::new(Mutex::new(false));
    let done_in = done.clone();
    let options = ui_common::native_options("Timbits — Settings", 520.0, 560.0);

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
