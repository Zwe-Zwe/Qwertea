//! Practice home: what the next lesson trains and why, your level at a
//! glance, and calibration progress. Also the phase intro and the
//! sitting-complete screens of the calibration flow.

use super::widgets::{
    card, eyebrow, meter, metric, muted, primary_button, quiet_button, sparkline, step_tracker,
    strong, target_chip, Step,
};
use crate::app::{App, Screen};
use crate::db::PendingLesson;
use egui::{vec2, Key, RichText, Ui};

fn start_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(Key::Enter))
}

fn phase_label(phase: &str) -> &str {
    match phase {
        "warmup" => "warm-up",
        "natural" => "prose",
        "coverage" => "coverage",
        "pseudo" => "rare pairs",
        "special" => "symbols",
        "probes" => "probes",
        "retest" => "retest",
        other => other,
    }
}

/// The phases of the lesson's calibration sitting, with progress.
fn sitting_steps(l: &PendingLesson) -> Vec<(String, Step)> {
    let Some(order) = &l.meta.phase_order else {
        return vec![];
    };
    let cur = l.meta.phase_no.unwrap_or(1) as usize;
    order
        .iter()
        .enumerate()
        .map(|(i, ph)| {
            let st = match (i + 1).cmp(&cur) {
                std::cmp::Ordering::Less => Step::Done,
                std::cmp::Ordering::Equal => Step::Current,
                std::cmp::Ordering::Greater => Step::Todo,
            };
            (phase_label(ph).to_string(), st)
        })
        .collect()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    if let Some(err) = &app.db_error {
        ui.label(RichText::new(err).color(p.error));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("home_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let full = ui.available_width().min(1040.0);
            ui.label(eyebrow(&p, "Practice"));
            ui.label(strong(greeting(app), 30.0).color(p.text));
            ui.add_space(14.0);
            let gap = 16.0;
            let left_w = ((full - gap) * 0.6).max(320.0);
            let right_w = (full - gap - left_w).max(240.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    next_card(app, ui, left_w);
                });
                ui.vertical(|ui| {
                    ui.set_width(right_w);
                    level_card(app, ui, right_w);
                });
            });
            ui.add_space(gap);
            if let Some(c) = app.calib.clone().filter(|c| !c.is_complete()) {
                card(ui, &p, |ui| {
                    ui.set_width(full - 40.0);
                    ui.horizontal(|ui| {
                        ui.label(eyebrow(
                            &p,
                            &format!("Calibration · sitting {} of {}", c.sitting, c.sittings),
                        ));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(muted(&p, format!("{:.0}% of all sittings", 100.0 * c.fraction())));
                        });
                    });
                    ui.add_space(6.0);
                    if let Some(l) = app.next.as_ref().filter(|l| l.is_calibration()) {
                        step_tracker(ui, &p, &sitting_steps(l));
                    }
                    meter(ui, &p, c.fraction(), p.accent, full - 40.0, 6.0);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "About {:.0} min left in this sitting. Calibration samples every key, letter pair and \
                             symbol once, so your lessons can be built around you. Stop between phases any time.",
                            app.minutes_left(c.chars_left_sitting).ceil()
                        ))
                        .size(13.5)
                        .color(p.sub),
                    );
                    if app.confirm_skip_calibration {
                        ui.horizontal(|ui| {
                            ui.label("Skip the rest and start adaptive lessons now?");
                            if ui.button("Skip calibration").clicked() {
                                app.skip_calibration();
                                app.confirm_skip_calibration = false;
                            }
                            if ui.button("Keep going").clicked() {
                                app.confirm_skip_calibration = false;
                            }
                        });
                    } else if quiet_button(ui, &p, "skip calibration").clicked() {
                        app.confirm_skip_calibration = true;
                    }
                });
            }
        });
}

fn greeting(app: &App) -> String {
    match (&app.next, app.calib.as_ref().map(|c| c.is_complete())) {
        (None, _) => "Brewing your next lesson".into(),
        (Some(l), _) if l.is_calibration() => "Let's get to know your typing".into(),
        (Some(_), _) if app.history.lessons.is_empty() => "Ready when you are".into(),
        _ => "Your next lesson is ready".into(),
    }
}

fn next_card(app: &mut App, ui: &mut Ui, width: f32) {
    let p = app.palette;
    card(ui, &p, |ui| {
        ui.set_width(width - 40.0);
        let Some(l) = app.next.clone() else {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(p.accent));
                ui.label(strong("Steeping…", 22.0));
            });
            ui.label(muted(
                &p,
                "The engine is analysing your last lesson and brewing the next one.",
            ));
            if !app.engine.status.is_ok() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(app.engine.status.label())
                        .color(p.error)
                        .size(13.0),
                );
                ui.label(
                    muted(
                        &p,
                        format!("Engine log: {}", app.engine.log_path().display()),
                    )
                    .size(12.5),
                );
            }
            if ui.button("Check again").clicked() {
                app.refresh_queue();
            }
            return;
        };
        ui.label(eyebrow(&p, "Up next"));
        ui.label(strong(l.title(), 26.0).color(p.text));
        if let Some(instr) = &l.meta.instruction {
            ui.label(muted(&p, instr));
        }
        let targets = app.next_targets.clone();
        if !targets.is_empty() {
            ui.add_space(10.0);
            ui.label(eyebrow(&p, "Training"));
            for (t, st) in targets.iter().take(6) {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(64.0, 30.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| target_chip(ui, &p, &t.item),
                    );
                    match st {
                        Some(s) => {
                            meter(ui, &p, (s.p_error / 0.25) as f32, p.accent2, 110.0, 6.0);
                            let why = if t.reason == "review" {
                                "review: you improved this".to_string()
                            } else {
                                let mut w = format!("{:.0}% slips", 100.0 * s.p_error);
                                if let Some(ms) = s.latency_ms {
                                    w.push_str(&format!(" · {ms:.0} ms"));
                                }
                                w
                            };
                            ui.label(muted(&p, why).size(13.0));
                        }
                        None => {
                            ui.label(muted(&p, "new pattern").size(13.0));
                        }
                    }
                });
            }
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if primary_button(ui, &p, "Start lesson", "Enter").clicked() || start_pressed(ui) {
                app.start_next();
            }
            let n = l.text.chars().count();
            ui.label(muted(
                &p,
                format!(
                    "{n} characters · about {}",
                    super::clock(app.minutes_left(n as i64) * 60.0)
                ),
            ));
        });
    });
}

fn level_card(app: &App, ui: &mut Ui, width: f32) {
    let p = app.palette;
    card(ui, &p, |ui| {
        ui.set_width(width - 40.0);
        ui.label(eyebrow(&p, "Your level"));
        let recent = app.history.recent(10);
        if recent.is_empty() {
            ui.label(strong("–", 34.0));
            ui.label(muted(
                &p,
                "Finish a few lessons and your level appears here.",
            ));
            return;
        }
        let n = recent.len() as f64;
        let wpm = recent.iter().map(|l| l.1).sum::<f64>() / n;
        let acc = recent.iter().map(|l| l.2).sum::<f64>() / n;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            metric(ui, &p, "speed", &format!("{wpm:.0}"), "wpm", 34.0);
            metric(
                ui,
                &p,
                "accuracy",
                &format!("{:.0}", 100.0 * acc),
                "%",
                34.0,
            );
        });
        let series: Vec<f64> = app.history.recent(20).iter().map(|l| l.1).collect();
        ui.add_space(4.0);
        sparkline(ui, &p, &series, vec2(width - 40.0, 46.0), p.accent);
        ui.label(
            muted(
                &p,
                format!(
                    "last {} lessons · best {:.0} wpm · {} total",
                    series.len(),
                    app.history.best_wpm().unwrap_or(0.0),
                    app.history.lessons.len()
                ),
            )
            .size(12.5),
        );
    });
}

/// Calibration phase introduction.
pub fn intro(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let Some(l) = app.intro.clone() else {
        app.screen = Screen::Home;
        return;
    };
    let width = ui.available_width().min(640.0);
    ui.add_space((ui.available_height() * 0.14).max(10.0));
    ui.vertical_centered(|ui| {
        ui.set_max_width(width);
        card(ui, &p, |ui| {
            ui.set_width(width - 40.0);
            if l.is_calibration() {
                ui.label(eyebrow(
                    &p,
                    &format!(
                        "Calibration · sitting {} of {}",
                        l.meta.sitting.unwrap_or(1),
                        l.meta.sittings.unwrap_or(1)
                    ),
                ));
                ui.add_space(4.0);
                step_tracker(ui, &p, &sitting_steps(&l));
            } else {
                ui.label(eyebrow(&p, "Checkpoint"));
            }
            ui.add_space(8.0);
            ui.label(strong(l.title(), 32.0).color(p.text));
            if let Some(instr) = &l.meta.instruction {
                ui.label(RichText::new(instr).size(17.0).color(p.text));
            }
            if let Some(c) = app.calib.as_ref().filter(|_| l.is_calibration()) {
                ui.label(muted(
                    &p,
                    format!(
                        "About {:.0} min left in this sitting.",
                        app.minutes_left(c.chars_left_sitting).ceil()
                    ),
                ));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if primary_button(ui, &p, "Begin", "Enter").clicked() || start_pressed(ui) {
                    app.intro = None;
                    app.begin(l.clone());
                }
                if quiet_button(ui, &p, "back").clicked()
                    || ui.input(|i| i.key_pressed(Key::Escape))
                {
                    app.intro = None;
                    app.screen = Screen::Home;
                }
            });
        });
    });
}

pub fn sitting_done(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let width = ui.available_width().min(640.0);
    ui.add_space((ui.available_height() * 0.16).max(10.0));
    ui.vertical_centered(|ui| {
        ui.set_max_width(width);
        card(ui, &p, |ui| {
            ui.set_width(width - 40.0);
            ui.label(eyebrow(&p, "Calibration"));
            ui.label(strong("Sitting complete", 32.0).color(p.text));
            ui.label(
                RichText::new(
                    "Nicely done. Step away for a while: the next sitting is more useful with fresh hands. \
                     Calibration picks up where you left off.",
                )
                .size(16.0),
            );
            if let Some(c) = &app.calib {
                ui.add_space(6.0);
                meter(ui, &p, c.fraction(), p.accent, width - 40.0, 6.0);
                ui.label(muted(&p, format!("Next: sitting {} of {}", c.sitting, c.sittings)));
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if primary_button(ui, &p, "Continue now", "").clicked() {
                    app.start_next();
                }
                if quiet_button(ui, &p, "later").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    app.screen = Screen::Home;
                }
            });
        });
    });
}
