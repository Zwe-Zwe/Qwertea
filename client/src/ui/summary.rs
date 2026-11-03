//! Lesson results: WPM, accuracy, the per-second chart, characters,
//! consistency, slowest bigrams and the lesson's focus items.

use super::widgets::{card, chip, line_chart, muted, primary_button, stat, Series};
use crate::app::{App, Screen};
use egui::{vec2, Key, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let Some(f) = app.last.as_ref() else {
        app.screen = Screen::Home;
        return;
    };
    let s = f.stats.clone();
    let lesson = f.lesson.clone();
    let saved = f.saved;
    let width = ui.available_width().min(980.0);
    let mut go_next = false;
    let mut go_home = false;
    ui.vertical_centered(|ui| {
        ui.set_max_width(width);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(lesson.title()).size(15.0).color(p.sub));
            if !saved {
                ui.label(muted(&p, "· saving…"));
            }
        });
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                stat(ui, &p, &format!("{:.0}", s.wpm), "wpm", 64.0);
                ui.add_space(6.0);
                stat(
                    ui,
                    &p,
                    &format!("{:.0}%", 100.0 * s.accuracy),
                    "accuracy",
                    64.0,
                );
            });
            card(ui, &p, |ui| {
                let pts_wpm: Vec<(f64, f64)> = s
                    .per_second
                    .iter()
                    .map(|x| (x.second as f64, x.wpm))
                    .collect();
                let pts_raw: Vec<(f64, f64)> = s
                    .per_second
                    .iter()
                    .map(|x| (x.second as f64, x.raw))
                    .collect();
                let markers: Vec<(f64, egui::Color32)> = s
                    .per_second
                    .iter()
                    .filter(|x| x.errors > 0)
                    .map(|x| (x.second as f64, p.error))
                    .collect();
                line_chart(
                    ui,
                    &p,
                    vec2(ui.available_width(), 220.0),
                    &[
                        Series {
                            points: &pts_raw,
                            color: p.sub,
                            width: 1.5,
                            dots: false,
                        },
                        Series {
                            points: &pts_wpm,
                            color: p.accent,
                            width: 2.5,
                            dots: false,
                        },
                    ],
                    &markers,
                    "wpm (yellow) · raw (grey) · red lines = errors · seconds",
                );
            });
        });
        ui.add_space(10.0);
        card(ui, &p, |ui| {
            ui.set_width(width - 32.0);
            ui.columns(5, |c| {
                stat(&mut c[0], &p, &format!("{:.0}", s.raw_wpm), "raw wpm", 26.0);
                stat(
                    &mut c[1],
                    &p,
                    &format!("{}/{}/{}", s.correct, s.incorrect, s.corrected),
                    "correct / incorrect / fixed",
                    26.0,
                );
                stat(
                    &mut c[2],
                    &p,
                    &format!("{:.0}%", s.consistency),
                    "consistency",
                    26.0,
                );
                stat(&mut c[3], &p, &super::clock(s.seconds), "time", 26.0);
                stat(&mut c[4], &p, &s.errors.to_string(), "errors", 26.0);
            });
        });
        ui.add_space(10.0);
        card(ui, &p, |ui| {
            ui.set_width(width - 32.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(muted(&p, "slowest bigrams"));
                for (bg, ms) in &s.slowest_bigrams {
                    chip(ui, &p, &format!("{} {:.0}ms", bg.replace(' ', "·"), ms));
                }
                if !s.missed_keys.is_empty() {
                    ui.add_space(16.0);
                    ui.label(muted(&p, "missed"));
                    for (c, n) in s.missed_keys.iter().take(6) {
                        let shown = match c {
                            ' ' => "·".to_string(),
                            '\n' => "enter".to_string(),
                            c => c.to_string(),
                        };
                        chip(ui, &p, &format!("{shown} ×{n}"));
                    }
                }
            });
            let targets = lesson.targets();
            if !targets.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(muted(&p, "this lesson focused on"));
                    for t in &targets {
                        chip(ui, &p, &t.item.replace(' ', "·"));
                    }
                });
            }
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
            let has_next = app.next.is_some();
            let label = if has_next {
                "Next lesson"
            } else {
                "Next lesson (preparing…)"
            };
            if primary_button(ui, &p, label).clicked() {
                go_next = true;
            }
            if ui.button("Menu  esc").clicked() {
                go_home = true;
            }
        });
        if app.next.is_none() {
            ui.label(muted(
                &p,
                "The engine is analysing this lesson and brewing the next one…",
            ));
        }
    });
    let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if (go_next || enter) && saved && app.next.is_some() {
        app.start_next();
    } else if go_home || esc {
        app.screen = Screen::Home;
    }
}
