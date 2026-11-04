//! Settings: saved to `config.toml` (comments and engine sections are kept).

use super::theme::{Palette, THEMES};
use super::widgets::{card, eyebrow, muted, primary_button, strong};
use crate::app::App;
use crate::config::{PresentModePref, TimestampSourceKind};
use egui::{
    pos2, vec2, Align2, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};

/// A setting row: name and explanation on the left, the control on the right.
fn row(ui: &mut Ui, p: &Palette, name: &str, help: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() * 0.55).max(220.0));
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(name).size(15.5));
            if !help.is_empty() {
                ui.label(muted(p, help).size(12.5));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    ui.add_space(6.0);
}

/// A clickable theme preview: background, panel, text and the two accents.
fn theme_card(ui: &mut Ui, t: &Palette, selected: bool, active: &Palette) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 92.0), Sense::click());
    let pt = ui.painter();
    pt.rect_filled(rect, CornerRadius::same(12), t.bg);
    let inner = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(130.0, 44.0));
    pt.rect_filled(inner, CornerRadius::same(8), t.panel);
    pt.text(
        inner.min + vec2(10.0, 9.0),
        Align2::LEFT_TOP,
        "steep",
        FontId::monospace(14.0),
        t.text,
    );
    pt.text(
        inner.min + vec2(58.0, 9.0),
        Align2::LEFT_TOP,
        "ou",
        FontId::monospace(14.0),
        t.accent2,
    );
    pt.text(
        inner.min + vec2(80.0, 9.0),
        Align2::LEFT_TOP,
        "ve",
        FontId::monospace(14.0),
        t.sub,
    );
    pt.rect_filled(
        Rect::from_min_size(inner.min + vec2(10.0, 30.0), vec2(60.0, 3.0)),
        CornerRadius::same(2),
        t.accent,
    );
    pt.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        Align2::LEFT_BOTTOM,
        t.label,
        FontId::proportional(14.0),
        t.text,
    );
    let stroke = if selected {
        Stroke::new(2.5, active.accent)
    } else if resp.hovered() {
        Stroke::new(1.5, active.sub)
    } else {
        Stroke::new(1.0, active.panel_hi)
    };
    pt.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Outside);
    if selected {
        pt.circle_filled(
            pos2(rect.right() - 14.0, rect.bottom() - 16.0),
            5.0,
            active.accent,
        );
    }
    resp.clicked()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            super::widgets::page(ui, 920.0, |ui| {
                let width = ui.available_width();
                ui.label(eyebrow(&p, "Settings"));
                ui.label(strong("Make it yours", 30.0).color(p.text));
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Theme"));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                        for t in THEMES {
                            let selected =
                                super::theme::palette(&app.cfg.client.theme).name == t.name;
                            if theme_card(ui, &t, selected, &p) {
                                app.cfg.client.theme = t.name.to_string();
                                app.settings_dirty = true;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    row(ui, &p, "Text size", "Size of the lesson text.", |ui| {
                        if ui
                            .add(
                                egui::Slider::new(&mut app.cfg.client.font_size, 16.0..=48.0)
                                    .step_by(1.0),
                            )
                            .changed()
                        {
                            app.settings_dirty = true;
                        }
                    });
                    row(
                        ui,
                        &p,
                        "Live speed",
                        "Show WPM under the text while you type.",
                        |ui| {
                            if ui.checkbox(&mut app.cfg.client.live_wpm, "").changed() {
                                app.settings_dirty = true;
                            }
                        },
                    );
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Typing"));
                    row(
                        ui,
                        &p,
                        "Stop on error",
                        "The caret waits until you press the right key.",
                        |ui| {
                            if ui.checkbox(&mut app.cfg.client.stop_on_error, "").changed() {
                                app.settings_dirty = true;
                            }
                        },
                    );
                    let layout = app.cfg.general.layout.clone();
                    row(
                        ui,
                        &p,
                        "Keyboard layout",
                        "Set general.layout in config.toml.",
                        |ui| {
                            ui.label(RichText::new(layout).monospace());
                        },
                    );
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Timing · applies after a restart"));
                    let c = &mut app.cfg.client;
                    let mut dirty = false;
                    row(
                        ui,
                        &p,
                        "Present mode",
                        "mailbox is smooth with low latency; fifo saves power.",
                        |ui| {
                            egui::ComboBox::from_id_salt("present")
                                .selected_text(c.present_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for m in PresentModePref::ALL {
                                        dirty |= ui
                                            .selectable_value(&mut c.present_mode, m, m.as_str())
                                            .changed();
                                    }
                                });
                        },
                    );
                    row(
                        ui,
                        &p,
                        "Timestamp source",
                        "evdev reads kernel timestamps (Linux, input group).",
                        |ui| {
                            egui::ComboBox::from_id_salt("tsrc")
                                .selected_text(c.timestamp_source.as_str())
                                .show_ui(ui, |ui| {
                                    for m in
                                        [TimestampSourceKind::Winit, TimestampSourceKind::Evdev]
                                    {
                                        dirty |= ui
                                            .selectable_value(
                                                &mut c.timestamp_source,
                                                m,
                                                m.as_str(),
                                            )
                                            .changed();
                                    }
                                });
                        },
                    );
                    row(
                        ui,
                        &p,
                        "Keyboard device",
                        "For evdev; empty picks the first keyboard.",
                        |ui| {
                            dirty |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut c.keyboard_device)
                                        .hint_text("/dev/input/event3")
                                        .desired_width(200.0),
                                )
                                .changed();
                        },
                    );
                    if dirty {
                        app.settings_dirty = true;
                    }
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Engine & data"));
                    row(
                        ui,
                        &p,
                        "Start the engine automatically",
                        "The engine analyses lessons and brews new ones.",
                        |ui| {
                            if ui.checkbox(&mut app.cfg.engine.autostart, "").changed() {
                                app.settings_dirty = true;
                            }
                        },
                    );
                    let status = app.engine.status.label();
                    row(ui, &p, "Engine status", "", |ui| {
                        ui.label(muted(&p, status));
                    });
                    let db = app.cfg.db_path().display().to_string();
                    let log = app.engine.log_path().display().to_string();
                    ui.label(muted(&p, format!("Database  {db}")).size(12.5));
                    ui.label(muted(&p, format!("Engine log  {log}")).size(12.5));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open full report").clicked() {
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
                    if app.settings_dirty {
                        if primary_button(ui, &p, "Save settings", "").clicked() {
                            app.save_settings();
                        }
                    } else {
                        ui.label(muted(&p, "All changes saved."));
                    }
                    ui.add_space(16.0);
                    ui.label(
                        muted(
                            &p,
                            format!(
                                "Qwertea {} · everything stays on this computer.",
                                env!("CARGO_PKG_VERSION")
                            ),
                        )
                        .size(12.5),
                    );
                });
                ui.add_space(24.0);
            });
        });
}
