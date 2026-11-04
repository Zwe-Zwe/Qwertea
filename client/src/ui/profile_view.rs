//! Profile: who is typing, today's practice against their daily goal, their
//! streak and goals, and the profiles on this computer (switch, create,
//! delete, export and import). Each profile has its own database, so its
//! progress, model, lessons and settings are its own.

use super::widgets::{avatar, card_h, eyebrow, meter, metric, muted, primary_button, strong};
use crate::app::App;
use crate::profile;
use egui::{vec2, RichText, Ui};

const GAP: f32 = 16.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::ScrollArea::vertical()
        .id_salt("profile_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            super::widgets::page(ui, 1200.0, |ui| {
                let full = ui.available_width();
                ui.label(eyebrow(&p, "Profile"));
                header(app, ui);
                ui.add_space(14.0);
                let goals = std::cell::Cell::new(None);
                if full >= 820.0 {
                    super::widgets::split(
                        ui,
                        "profile_top",
                        full,
                        0.5,
                        GAP,
                        |ui, w, h| today_card(app, ui, w, h),
                        |ui, w, h| goals_card(app, ui, w, h, &goals),
                    );
                } else {
                    today_card(app, ui, full, 0.0);
                    ui.add_space(GAP);
                    goals_card(app, ui, full, 0.0, &goals);
                }
                if let Some((wpm, acc, mins)) = goals.take() {
                    app.profile.goal_wpm = wpm;
                    app.profile.goal_accuracy = acc;
                    app.profile.daily_minutes = mins;
                    app.mark_profile_dirty();
                }
                ui.add_space(GAP);
                profiles_card(app, ui, full);
                ui.add_space(24.0);
            });
        });
}

fn header(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        avatar(ui, &p, &app.profile.name, 56.0);
        ui.add_space(6.0);
        if let Some(mut name) = app.profile_ui.rename.take() {
            let r = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .font(egui::FontId::new(24.0, super::theme::bold()))
                    .desired_width(320.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if ui.button("Save").clicked() || enter {
                app.rename_profile(&name);
            } else if ui.button("Cancel").clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
            } else {
                app.profile_ui.rename = Some(name);
            }
        } else {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(strong(app.profile.name.clone(), 30.0).color(p.text));
                let lessons = app.history.lessons.len();
                ui.label(muted(
                    &p,
                    format!(
                        "{lessons} lesson{} · {} typed · best {:.0} wpm",
                        if lessons == 1 { "" } else { "s" },
                        hours(app.history.total_seconds()),
                        app.history.best_wpm().unwrap_or(0.0)
                    ),
                ));
            });
            ui.add_space(10.0);
            if ui.button("Rename").clicked() {
                app.profile_ui.rename = Some(app.profile.name.clone());
            }
        }
    });
}

fn hours(secs: f64) -> String {
    let m = (secs / 60.0).round() as u64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{}h {:02}m", m / 60, m % 60)
    }
}

fn today_card(app: &App, ui: &mut Ui, w: f32, min_h: f32) -> f32 {
    let p = app.palette;
    let today = profile::today();
    let done_min = app.history.seconds_on(today) / 60.0;
    let goal = app.profile.daily_minutes.max(1) as f64;
    let streak = app.history.streak(today);
    card_h(ui, &p, min_h, |ui| {
        ui.set_width(w - 40.0);
        ui.label(eyebrow(&p, "Today"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            metric(
                ui,
                &p,
                "practised",
                &format!("{done_min:.0}"),
                &format!("of {goal:.0} min"),
                34.0,
            );
            metric(
                ui,
                &p,
                "streak",
                &streak.to_string(),
                if streak == 1 { "day" } else { "days" },
                34.0,
            );
        });
        ui.add_space(4.0);
        let frac = (done_min / goal) as f32;
        meter(
            ui,
            &p,
            frac,
            if frac >= 1.0 { p.good } else { p.accent },
            w - 40.0,
            8.0,
        );
        ui.add_space(4.0);
        let msg = if frac >= 1.0 {
            "Daily goal reached. Anything more is a bonus.".to_string()
        } else if done_min < 0.5 {
            format!("A {goal:.0}-minute session keeps your streak going.")
        } else {
            format!(
                "{:.0} min to go for today's goal.",
                (goal - done_min).ceil()
            )
        };
        ui.label(muted(&p, msg).size(13.0));
        // Last 14 days as small bars.
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w - 40.0, 44.0), egui::Sense::hover());
        let pt = ui.painter();
        let days = 14;
        let slot = rect.width() / days as f32;
        for i in 0..days {
            let day = today - chrono::Days::new((days - 1 - i) as u64);
            let m = app.history.seconds_on(day) / 60.0;
            let f = (m / goal).min(1.0) as f32;
            let x = rect.left() + slot * i as f32 + 2.0;
            let bar_h = (rect.height() - 4.0) * f.max(0.06);
            let r = egui::Rect::from_min_size(
                egui::pos2(x, rect.bottom() - bar_h),
                vec2(slot - 4.0, bar_h),
            );
            let c = if m <= 0.0 {
                p.panel_hi
            } else if f >= 1.0 {
                p.good
            } else {
                p.accent.gamma_multiply(0.7)
            };
            pt.rect_filled(r, egui::CornerRadius::same(3), c);
        }
        ui.label(muted(&p, "last 14 days").size(11.5));
    })
    .1
}

/// Goal edits are handed back through `changed` as (wpm, accuracy, minutes).
fn goals_card(
    app: &App,
    ui: &mut Ui,
    w: f32,
    min_h: f32,
    changed: &std::cell::Cell<Option<(u32, u32, u32)>>,
) -> f32 {
    let p = app.palette;
    let recent = app.history.recent(10);
    let n = recent.len() as f64;
    let (wpm, acc) = if n > 0.0 {
        (
            recent.iter().map(|l| l.wpm).sum::<f64>() / n,
            100.0 * recent.iter().map(|l| l.accuracy).sum::<f64>() / n,
        )
    } else {
        (0.0, 0.0)
    };
    let mut dirty = false;
    let h = card_h(ui, &p, min_h, |ui| {
        ui.set_width(w - 40.0);
        ui.label(eyebrow(&p, "Goals"));
        let mut s = app.profile.clone();
        let line =
            |ui: &mut Ui, label: &str, now: String, frac: f32, control: &mut dyn FnMut(&mut Ui)| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        control(ui)
                    });
                });
                meter(
                    ui,
                    &p,
                    frac,
                    if frac >= 1.0 { p.good } else { p.accent2 },
                    w - 40.0,
                    6.0,
                );
                ui.label(muted(&p, now).size(12.5));
                ui.add_space(6.0);
            };
        let goal_wpm = s.goal_wpm.max(1) as f64;
        line(
            ui,
            "Speed",
            if n > 0.0 {
                format!("now {wpm:.0} wpm (last {n:.0} lessons)")
            } else {
                "no lessons yet".into()
            },
            (wpm / goal_wpm) as f32,
            &mut |ui| {
                dirty |= ui
                    .add(
                        egui::DragValue::new(&mut s.goal_wpm)
                            .range(10..=250)
                            .suffix(" wpm"),
                    )
                    .changed();
            },
        );
        let goal_acc = s.goal_accuracy.max(1) as f64;
        line(
            ui,
            "Accuracy",
            if n > 0.0 {
                format!("now {acc:.1}%")
            } else {
                "no lessons yet".into()
            },
            ((acc - 80.0).max(0.0) / (goal_acc - 80.0).max(1.0)) as f32,
            &mut |ui| {
                dirty |= ui
                    .add(
                        egui::DragValue::new(&mut s.goal_accuracy)
                            .range(80..=100)
                            .suffix(" %"),
                    )
                    .changed();
            },
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new("Daily practice").size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dirty |= ui
                    .add(
                        egui::DragValue::new(&mut s.daily_minutes)
                            .range(1..=180)
                            .suffix(" min"),
                    )
                    .changed();
            });
        });
        if dirty {
            changed.set(Some((s.goal_wpm, s.goal_accuracy, s.daily_minutes)));
        }
    })
    .1;
    h
}

fn profiles_card(app: &mut App, ui: &mut Ui, w: f32) {
    let p = app.palette;
    let mut switch_to = None;
    let mut delete = None;
    let mut create = None;
    let mut import = None;
    let mut export = false;
    card_h(ui, &p, 0.0, |ui| {
        ui.set_width(w - 40.0);
        ui.label(eyebrow(&p, "Profiles on this computer"));
        ui.label(
            muted(
                &p,
                "Each profile has its own progress, lessons, theme and settings. \
                 Switching saves and closes the current one.",
            )
            .size(13.0),
        );
        ui.add_space(6.0);
        let active = app.registry.active.clone();
        let entries = app.registry.profiles.clone();
        for e in &entries {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                avatar(ui, &p, &e.name, 32.0);
                ui.label(RichText::new(&e.name).size(16.0).color(p.text));
                if e.id == active {
                    super::widgets::chip(ui, &p, "open");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if e.id == active {
                        return;
                    }
                    if app.profile_ui.confirm_delete.as_deref() == Some(e.id.as_str()) {
                        if ui
                            .button(RichText::new("Delete for good").color(p.error))
                            .clicked()
                        {
                            delete = Some(e.id.clone());
                        }
                        if ui.button("Keep").clicked() {
                            app.profile_ui.confirm_delete = None;
                        }
                        ui.label(muted(&p, "All of its typing history goes.").size(12.5));
                    } else {
                        if ui.button("Delete…").clicked() {
                            app.profile_ui.confirm_delete = Some(e.id.clone());
                        }
                        if ui.button("Switch").clicked() {
                            switch_to = Some(e.id.clone());
                        }
                    }
                });
            });
            ui.add_space(2.0);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let r = ui.add(
                egui::TextEdit::singleline(&mut app.profile_ui.new_name)
                    .hint_text("Name for a new profile")
                    .desired_width(260.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = !app.profile_ui.new_name.trim().is_empty();
            if (ui
                .add_enabled(ok, egui::Button::new("Create and switch"))
                .clicked()
                || enter)
                && ok
            {
                create = Some(app.profile_ui.new_name.trim().to_string());
            }
        });
        ui.add_space(12.0);
        ui.label(eyebrow(&p, "Move a profile to another computer"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if primary_button(ui, &p, "Export this profile", "").clicked() {
                export = true;
            }
            ui.label(
                muted(
                    &p,
                    format!(
                        "One .qwertea file in {}, with all progress and settings.",
                        profile::export_dir().display()
                    ),
                )
                .size(12.5),
            );
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.add(
                egui::TextEdit::singleline(&mut app.profile_ui.import_path)
                    .hint_text("~/Downloads/qwertea-me-2026-11-04.qwertea")
                    .desired_width(380.0),
            );
            let ok = !app.profile_ui.import_path.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Import")).clicked() {
                import = Some(app.profile_ui.import_path.clone());
            }
        });
        ui.label(
            muted(
                &p,
                "Paste the path of an exported file (you can drop it onto the window).",
            )
            .size(12.5),
        );
    });
    // Files dropped onto the window fill the import field.
    if let Some(path) = ui.ctx().input(|i| {
        i.raw
            .dropped_files
            .first()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
    }) {
        app.profile_ui.import_path = path.display().to_string();
    }
    if export {
        app.export_profile();
    }
    if let Some(path) = import {
        app.import_profile(&path);
    }
    if let Some(id) = delete {
        app.profile_ui.confirm_delete = None;
        app.delete_profile(&id);
    }
    if let Some(name) = create {
        app.profile_ui.new_name.clear();
        app.create_profile(&name);
    }
    if let Some(id) = switch_to {
        app.switch_profile(&id);
    }
}
