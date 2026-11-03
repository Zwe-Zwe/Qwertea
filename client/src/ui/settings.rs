//! Settings: saved to `config.toml` (comments and engine sections are kept).

use super::theme::THEMES;
use super::widgets::{card, muted};
use crate::app::App;
use crate::config::{PresentModePref, TimestampSourceKind};
use egui::{vec2, Grid, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let width = ui.available_width().min(760.0);
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                card(ui, &p, |ui| {
                    ui.set_width(width - 32.0);
                    ui.label(RichText::new("Appearance").size(18.0));
                    Grid::new("appearance")
                        .num_columns(2)
                        .spacing(vec2(24.0, 10.0))
                        .show(ui, |ui| {
                            let c = &mut app.cfg.client;
                            ui.label("Theme");
                            ui.horizontal(|ui| {
                                for t in THEMES {
                                    if ui.selectable_label(c.theme == t.name, t.name).clicked() {
                                        c.theme = t.name.to_string();
                                        app.settings_dirty = true;
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label("Text size");
                            if ui
                                .add(egui::Slider::new(&mut c.font_size, 16.0..=48.0).step_by(1.0))
                                .changed()
                            {
                                app.settings_dirty = true;
                            }
                            ui.end_row();
                            ui.label("Live WPM while typing");
                            if ui.checkbox(&mut c.live_wpm, "").changed() {
                                app.settings_dirty = true;
                            }
                            ui.end_row();
                        });
                });
                ui.add_space(12.0);
                card(ui, &p, |ui| {
                    ui.set_width(width - 32.0);
                    ui.label(RichText::new("Typing").size(18.0));
                    Grid::new("typing")
                        .num_columns(2)
                        .spacing(vec2(24.0, 10.0))
                        .show(ui, |ui| {
                            let c = &mut app.cfg.client;
                            ui.label("Stop on error");
                            if ui
                                .checkbox(&mut c.stop_on_error, "")
                                .on_hover_text(
                                    "The caret does not move until the right key is pressed",
                                )
                                .changed()
                            {
                                app.settings_dirty = true;
                            }
                            ui.end_row();
                            ui.label("Keyboard layout");
                            ui.label(muted(
                                &p,
                                format!(
                                    "{} (set general.layout in config.toml)",
                                    app.cfg.general.layout
                                ),
                            ));
                            ui.end_row();
                        });
                });
                ui.add_space(12.0);
                card(ui, &p, |ui| {
                    ui.set_width(width - 32.0);
                    ui.label(RichText::new("Timing").size(18.0));
                    ui.label(muted(&p, "Changes here apply after a restart."));
                    Grid::new("timing")
                        .num_columns(2)
                        .spacing(vec2(24.0, 10.0))
                        .show(ui, |ui| {
                            let c = &mut app.cfg.client;
                            ui.label("Present mode");
                            egui::ComboBox::from_id_salt("present")
                                .selected_text(c.present_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for m in PresentModePref::ALL {
                                        if ui
                                            .selectable_value(&mut c.present_mode, m, m.as_str())
                                            .changed()
                                        {
                                            app.settings_dirty = true;
                                        }
                                    }
                                });
                            ui.end_row();
                            ui.label("Timestamp source");
                            egui::ComboBox::from_id_salt("tsrc")
                                .selected_text(c.timestamp_source.as_str())
                                .show_ui(ui, |ui| {
                                    for m in
                                        [TimestampSourceKind::Winit, TimestampSourceKind::Evdev]
                                    {
                                        if ui
                                            .selectable_value(
                                                &mut c.timestamp_source,
                                                m,
                                                m.as_str(),
                                            )
                                            .changed()
                                        {
                                            app.settings_dirty = true;
                                        }
                                    }
                                });
                            ui.end_row();
                            ui.label("Keyboard device (evdev)");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut c.keyboard_device)
                                        .hint_text("auto, e.g. /dev/input/event3"),
                                )
                                .changed()
                            {
                                app.settings_dirty = true;
                            }
                            ui.end_row();
                        });
                });
                ui.add_space(12.0);
                card(ui, &p, |ui| {
                    ui.set_width(width - 32.0);
                    ui.label(RichText::new("Engine & data").size(18.0));
                    Grid::new("engine")
                        .num_columns(2)
                        .spacing(vec2(24.0, 10.0))
                        .show(ui, |ui| {
                            ui.label("Start engine automatically");
                            if ui.checkbox(&mut app.cfg.engine.autostart, "").changed() {
                                app.settings_dirty = true;
                            }
                            ui.end_row();
                            ui.label("Status");
                            ui.label(muted(&p, app.engine.status.label()));
                            ui.end_row();
                            ui.label("Database");
                            ui.label(muted(&p, app.cfg.db_path().display().to_string()));
                            ui.end_row();
                            ui.label("Engine log");
                            ui.label(muted(&p, app.engine.log_path().display().to_string()));
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open report").clicked() {
                            app.open_report();
                        }
                        let calibrating = app.calib.as_ref().is_some_and(|c| !c.is_complete());
                        if calibrating {
                            if app.confirm_skip_calibration {
                                if ui.button("Confirm: skip calibration").clicked() {
                                    app.skip_calibration();
                                    app.confirm_skip_calibration = false;
                                }
                            } else if ui.button("Skip remaining calibration").clicked() {
                                app.confirm_skip_calibration = true;
                            }
                        }
                    });
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let label = if app.settings_dirty {
                        "Save settings"
                    } else {
                        "Saved"
                    };
                    if ui
                        .add_enabled(app.settings_dirty, egui::Button::new(label))
                        .clicked()
                    {
                        app.save_settings();
                    }
                    ui.label(muted(&p, "Everything stays on this computer."));
                });
            });
        });
}
