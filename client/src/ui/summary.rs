//! Lesson notes: a plain-language verdict against your own recent lessons,
//! the rhythm of the lesson word by word, how you did on the patterns it
//! trained, and the details.

use super::widgets::{
    card, chip, eyebrow, meter, muted, primary_button, quiet_button, rhythm_strip, strong,
    target_chip,
};
use crate::app::{App, Screen};
use crate::stats::LessonStats;
use egui::{Grid, Key, RichText, Ui};

/// (occurrences, occurrences with a first-attempt slip) of `item` in the lesson.
fn target_result(text: &[char], s: &LessonStats, item: &str) -> (usize, usize) {
    let pat: Vec<char> = item.to_lowercase().chars().collect();
    if pat.is_empty() || pat.len() > text.len() {
        return (0, 0);
    }
    let low: Vec<char> = text.iter().map(|c| c.to_ascii_lowercase()).collect();
    let (mut n, mut slips) = (0, 0);
    for i in 0..=low.len() - pat.len() {
        if low[i..i + pat.len()] == pat[..] && i + pat.len() <= s.first_wrong.len() {
            n += 1;
            if s.first_wrong[i..i + pat.len()].iter().any(|w| *w) {
                slips += 1;
            }
        }
    }
    (n, slips)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let Some(f) = app.last.as_ref() else {
        app.screen = Screen::Home;
        return;
    };
    let s = f.stats.clone();
    let lesson = f.lesson.clone();
    let saved = f.saved;
    let recent = f.recent;
    let best_before = f.best_before;
    let text: Vec<char> = crate::session::normalize_text(&lesson.text);
    let mut go_next = false;
    let mut go_home = false;
    egui::ScrollArea::vertical()
        .id_salt("summary_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let full = ui.available_width().min(1040.0);
            ui.horizontal(|ui| {
                ui.label(eyebrow(&p, &format!("Lesson notes · {}", lesson.title())));
                if !saved {
                    ui.label(muted(&p, "saving…").size(12.0));
                }
            });
            ui.label(
                strong(format!("{:.0} wpm, {:.0}% accurate", s.wpm, 100.0 * s.accuracy), 36.0)
                    .color(p.text),
            );
            let (line, color) = verdict(&s, recent, best_before, &p);
            ui.label(RichText::new(line).size(16.5).color(color));
            ui.add_space(14.0);

            card(ui, &p, |ui| {
                ui.set_width(full - 40.0);
                ui.horizontal(|ui| {
                    ui.label(eyebrow(&p, "Rhythm · speed of each word"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("slip").size(12.0).color(p.error));
                        ui.label(RichText::new("clean").size(12.0).color(p.accent));
                        ui.label(muted(&p, "dashes: lesson average  ·  bars:").size(12.0));
                    });
                });
                rhythm_strip(ui, &p, &s.words, 120.0);
                ui.label(muted(&p, "Hover a bar to see the word.").size(12.0));
            });
            ui.add_space(14.0);

            let full_w = full;
            ui.allocate_ui(egui::vec2(full_w, 0.0), |ui| ui.columns(2, |cols| {
                {
                    let ui = &mut cols[0];
                    card(ui, &p, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(eyebrow(&p, "What this lesson trained"));
                        let targets = lesson.targets();
                        if targets.is_empty() {
                            ui.label(muted(
                                &p,
                                if lesson.is_calibration() {
                                    "A calibration lesson: it measures your baseline, so it has no targets yet."
                                } else {
                                    "General practice: no specific targets."
                                },
                            ));
                        }
                        for t in targets.iter().take(6) {
                            let (n, slips) = target_result(&text, &s, &t.item);
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(64.0, 30.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| target_chip(ui, &p, &t.item),
                                );
                                if n > 0 {
                                    let clean = (n - slips) as f32 / n as f32;
                                    meter(ui, &p, clean, if slips == 0 { p.good } else { p.accent2 }, 90.0, 6.0);
                                    ui.label(muted(&p, format!("{}/{} clean", n - slips, n)).size(13.0));
                                } else {
                                    ui.label(muted(&p, "not in this text").size(13.0));
                                }
                            });
                        }
                        if !s.missed_keys.is_empty() {
                            ui.add_space(8.0);
                            ui.label(eyebrow(&p, "Keys you missed"));
                            ui.horizontal_wrapped(|ui| {
                                for (c, n) in s.missed_keys.iter().take(8) {
                                    let shown = match c {
                                        ' ' => "space".to_string(),
                                        '\n' => "enter".to_string(),
                                        c => c.to_string(),
                                    };
                                    chip(ui, &p, &format!("{shown} ×{n}"));
                                }
                            });
                        }
                    });
                }
                {
                    let ui = &mut cols[1];
                    card(ui, &p, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(eyebrow(&p, "Details"));
                        Grid::new("details").num_columns(2).spacing([28.0, 6.0]).show(ui, |ui| {
                            let mut row = |k: &str, v: String| {
                                ui.label(muted(&p, k));
                                ui.label(RichText::new(v).size(16.0));
                                ui.end_row();
                            };
                            row("raw speed", format!("{:.0} wpm", s.raw_wpm));
                            row("consistency", format!("{:.0}%", s.consistency));
                            row("time", super::clock(s.seconds));
                            row("keystrokes", s.keystrokes.to_string());
                            row("first-try slips", s.errors.to_string());
                            row("fixed / left wrong", format!("{} / {}", s.corrected, s.incorrect));
                        });
                        if !s.slowest_bigrams.is_empty() {
                            ui.add_space(8.0);
                            ui.label(eyebrow(&p, "Slowest letter pairs"));
                            ui.horizontal_wrapped(|ui| {
                                for (bg, ms) in &s.slowest_bigrams {
                                    chip(ui, &p, &format!("{} {:.0} ms", bg.replace(' ', "·"), ms));
                                }
                            });
                        }
                    });
                }
            }));
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                let label = if app.next.is_some() { "Next lesson" } else { "Brewing next…" };
                if primary_button(ui, &p, label, "Enter").clicked() {
                    go_next = true;
                }
                if quiet_button(ui, &p, "back to practice  ·  Esc").clicked() {
                    go_home = true;
                }
            });
        });
    let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if (go_next || enter) && saved && app.next.is_some() {
        app.start_next();
    } else if go_home || esc {
        app.screen = Screen::Home;
    }
}

fn verdict(
    s: &LessonStats,
    recent: Option<(f64, f64)>,
    best_before: Option<f64>,
    p: &super::theme::Palette,
) -> (String, egui::Color32) {
    if let Some(best) = best_before {
        if s.wpm > best && s.accuracy >= 0.9 {
            return (
                format!(
                    "A new personal best, {:.0} wpm above your previous record.",
                    s.wpm - best
                ),
                p.accent,
            );
        }
    }
    match recent {
        Some((wpm, acc)) => {
            let dw = s.wpm - wpm;
            let da = 100.0 * (s.accuracy - acc);
            let speed = if dw.abs() < 1.0 {
                "about your usual speed".to_string()
            } else if dw > 0.0 {
                format!("{dw:.0} wpm faster than usual")
            } else {
                format!("{:.0} wpm slower than usual", -dw)
            };
            let accuracy = if da.abs() < 0.5 {
                "steady accuracy".to_string()
            } else if da > 0.0 {
                format!("{da:.1} points more accurate")
            } else {
                format!("{:.1} points less accurate", -da)
            };
            let color = if dw >= 0.0 && da >= -0.5 {
                p.good
            } else {
                p.text
            };
            (
                format!("Compared with your last lessons: {speed}, {accuracy}."),
                color,
            )
        }
        None => (
            "Your first measured lesson. This sets the baseline future lessons are compared with."
                .into(),
            p.sub,
        ),
    }
}
