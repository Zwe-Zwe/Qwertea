//! Home: next lesson, calibration progress, quick stats. Also the phase intro
//! and sitting-complete screens of the calibration flow.

use super::widgets::{card, chip, muted, primary_button, stat};
use crate::app::{App, Screen};
use egui::{Align, Key, Layout, RichText, Ui};

fn enter_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space))
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    if let Some(err) = &app.db_error {
        ui.label(RichText::new(err).color(p.error));
        return;
    }
    let width = ui.available_width().min(860.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(width);
        ui.add_space(12.0);

        // Calibration progress.
        if let Some(c) = app.calib.clone().filter(|c| !c.is_complete()) {
            card(ui, &p, |ui| {
                ui.set_width(width - 32.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Calibration").size(19.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(muted(&p, format!("{:.0}% complete", 100.0 * c.fraction())));
                    });
                });
                ui.add(
                    egui::ProgressBar::new(c.fraction())
                        .desired_width(ui.available_width())
                        .fill(p.accent)
                        .desired_height(6.0),
                );
                ui.label(muted(
                    &p,
                    format!(
                        "Sitting {} of {} · Phase {} of {} · ~{:.0} min left in this sitting",
                        c.sitting,
                        c.sittings,
                        c.phase_no,
                        c.phases,
                        app.minutes_left(c.chars_left_sitting).ceil()
                    ),
                ));
                ui.label(
                    RichText::new(
                        "A one-off set of sittings that samples every key, letter pair and symbol, \
                         so your lessons can be built around you. Stop between phases at any time; \
                         progress is kept.",
                    )
                    .size(13.0)
                    .color(p.sub),
                );
                ui.horizontal(|ui| {
                    if app.confirm_skip_calibration {
                        ui.label(RichText::new("Skip the rest of calibration?").color(p.text));
                        if ui.button("Yes, skip").clicked() {
                            app.skip_calibration();
                            app.confirm_skip_calibration = false;
                        }
                        if ui.button("Cancel").clicked() {
                            app.confirm_skip_calibration = false;
                        }
                    } else if ui
                        .add(egui::Button::new(muted(&p, "skip calibration")).frame(false))
                        .on_hover_text("Start adaptive lessons now from the data collected so far")
                        .clicked()
                    {
                        app.confirm_skip_calibration = true;
                    }
                });
            });
            ui.add_space(14.0);
        }

        // Next lesson.
        card(ui, &p, |ui| {
            ui.set_width(width - 32.0);
            match app.next.clone() {
                Some(l) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Next lesson").size(13.0).color(p.sub));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mins = app.minutes_left(l.text.chars().count() as i64);
                            ui.label(muted(
                                &p,
                                format!(
                                    "{} characters · ~{}",
                                    l.text.chars().count(),
                                    super::clock(mins * 60.0)
                                ),
                            ));
                        });
                    });
                    ui.label(RichText::new(l.title()).size(24.0).color(p.text));
                    if let Some(instr) = &l.meta.instruction {
                        ui.label(muted(&p, instr));
                    }
                    let targets = l.targets();
                    if !targets.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(muted(&p, "focus"));
                            for t in targets.iter().take(6) {
                                chip(ui, &p, &t.item.replace(' ', "·"));
                            }
                        });
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if primary_button(ui, &p, "Start").clicked() || enter_pressed(ui) {
                            app.start_next();
                        }
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(p.accent));
                        ui.label(RichText::new("Engine is preparing your next lesson…").size(19.0));
                    });
                    if !app.engine.status.is_ok() {
                        ui.label(
                            RichText::new(app.engine.status.label())
                                .color(p.error)
                                .size(13.0),
                        );
                        ui.label(muted(
                            &p,
                            format!("Engine log: {}", app.engine.log_path().display()),
                        ));
                    }
                    if ui.button("Check again").clicked() {
                        app.refresh_queue();
                    }
                }
            }
        });
        ui.add_space(14.0);

        // Overview.
        card(ui, &p, |ui| {
            ui.set_width(width - 32.0);
            ui.columns(4, |cols| {
                let recent = app.history.recent(10);
                let avg = |f: fn(&(usize, f64, f64, f64, String)) -> f64| {
                    (!recent.is_empty())
                        .then(|| recent.iter().map(f).sum::<f64>() / recent.len() as f64)
                };
                let fmt =
                    |v: Option<f64>, s: &str| v.map_or("–".to_string(), |v| format!("{v:.0}{s}"));
                stat(
                    &mut cols[0],
                    &p,
                    &fmt(avg(|l| l.1), ""),
                    "avg wpm (last 10)",
                    30.0,
                );
                stat(
                    &mut cols[1],
                    &p,
                    &fmt(avg(|l| 100.0 * l.2), "%"),
                    "avg accuracy",
                    30.0,
                );
                stat(
                    &mut cols[2],
                    &p,
                    &fmt(app.history.best_wpm(), ""),
                    "best wpm",
                    30.0,
                );
                stat(
                    &mut cols[3],
                    &p,
                    &app.history.lessons.len().to_string(),
                    "lessons",
                    30.0,
                );
            });
            if let Some(l) = app.history.lessons.last() {
                ui.label(muted(
                    &p,
                    format!("last lesson: {:.0} wpm · {:.1}% accuracy", l.1, 100.0 * l.2),
                ));
            }
        });
        ui.add_space(18.0);
        ui.label(
            RichText::new("enter — start   ·   esc — pause while typing")
                .size(12.0)
                .color(p.sub),
        );
    });
}

/// Calibration phase introduction ("Sitting 1 of 3 · Phase 4 of 7").
pub fn intro(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let Some(l) = app.intro.clone() else {
        app.screen = Screen::Home;
        return;
    };
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.18);
        if l.is_calibration() {
            let left = app
                .calib
                .as_ref()
                .map(|c| app.minutes_left(c.chars_left_sitting).ceil())
                .unwrap_or(0.0);
            ui.label(muted(
                &p,
                format!(
                    "Sitting {} of {} · Phase {} of {} · ~{:.0} min left",
                    l.meta.sitting.unwrap_or(1),
                    l.meta.sittings.unwrap_or(1),
                    l.meta.phase_no.unwrap_or(1),
                    l.meta.phases.unwrap_or(1),
                    left
                ),
            ));
        }
        ui.add_space(6.0);
        ui.label(RichText::new(l.title()).size(40.0).color(p.accent));
        if let Some(instr) = &l.meta.instruction {
            ui.add_space(4.0);
            ui.label(RichText::new(instr).size(18.0).color(p.text));
        }
        ui.add_space(28.0);
        if primary_button(ui, &p, "Begin").clicked() || enter_pressed(ui) {
            app.intro = None;
            app.begin(l);
        }
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new(muted(&p, "back to menu")).frame(false))
            .clicked()
            || ui.input(|i| i.key_pressed(Key::Escape))
        {
            app.intro = None;
            app.screen = Screen::Home;
        }
    });
}

pub fn sitting_done(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.18);
        ui.label(RichText::new("Sitting complete").size(40.0).color(p.accent));
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Nice work. Take a break and come back later; calibration resumes where you left off.",
            )
            .size(17.0),
        );
        if let Some(c) = &app.calib {
            ui.label(muted(
                &p,
                format!("Next: sitting {} of {}", c.sitting, c.sittings),
            ));
        }
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            let w = 360.0;
            ui.add_space((ui.available_width() - w) / 2.0);
            if primary_button(ui, &p, "Continue now").clicked() {
                app.start_next();
            }
            if ui.button("Back to menu").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                app.screen = Screen::Home;
            }
        });
    });
}
