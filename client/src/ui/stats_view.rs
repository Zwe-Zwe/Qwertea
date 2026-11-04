//! Progress: your level, how speed and accuracy are trending, where on the
//! keyboard you are weaker or stronger than your own baseline, and the
//! weakest keys, letter pairs and words from the engine's model.

use super::widgets::{
    area_chart, bar_list, card, card_h, eyebrow, keyboard, metric, muted, page, split, strong,
};
use crate::app::App;
use crate::db::ItemStat;
use egui::{vec2, ScrollArea, Ui};

const GAP: f32 = 16.0;

fn rows(items: &[ItemStat]) -> Vec<(String, f32, String)> {
    let max = items
        .iter()
        .map(|i| i.weakness)
        .fold(0.0, f64::max)
        .max(0.1);
    items
        .iter()
        .filter(|i| i.weakness > 0.0)
        .take(8)
        .map(|i| {
            let mut right = format!("{:.0}%", 100.0 * i.p_error);
            if let Some(ms) = i.latency_ms {
                right.push_str(&format!(" · {ms:.0}ms"));
            }
            let mut label = i.item.replace('\n', "↵");
            if i.is_holdout {
                label.push('°');
            }
            (label, (i.weakness / max) as f32, right)
        })
        .collect()
}

fn smooth(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    (0..points.len())
        .map(|i| {
            let w = &points[i.saturating_sub(9)..=i];
            (
                points[i].0,
                w.iter().map(|x| x.1).sum::<f64>() / w.len() as f64,
            )
        })
        .collect()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ScrollArea::vertical()
        .id_salt("stats_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page(ui, super::PAGE_WIDTH, |ui| {
                let full = ui.available_width();
                ui.label(eyebrow(&p, "Progress"));
                ui.label(strong("How your typing is changing", 30.0).color(p.text));
                ui.add_space(14.0);
                metrics(app, ui, full);
                ui.add_space(GAP);
                let h = &app.history;
                let wpm = smooth(&h.lessons.iter().map(|l| (l.0 as f64, l.1)).collect::<Vec<_>>());
                let acc = smooth(
                    &h.lessons
                        .iter()
                        .map(|l| (l.0 as f64, 100.0 * l.2))
                        .collect::<Vec<_>>(),
                );
                let trend = |ui: &mut Ui, w: f32, chart_h: f32, min_h: f32, speed: bool| {
                    card_h(ui, &p, min_h, |ui| {
                        ui.set_width(w - 40.0);
                        if speed {
                            ui.label(eyebrow(&p, "Speed trend · 10-lesson average"));
                            area_chart(ui, &p, vec2(w - 40.0, chart_h), &wpm, p.accent, "wpm  ·  lessons", None);
                        } else {
                            ui.label(eyebrow(&p, "Accuracy trend · 10-lesson average"));
                            area_chart(ui, &p, vec2(w - 40.0, chart_h), &acc, p.accent2, "% first-try accuracy", None);
                        }
                    })
                    .1
                };
                if full >= 1100.0 {
                    split(
                        ui,
                        "trends",
                        full,
                        0.5,
                        GAP,
                        |ui, w, h| trend(ui, w, 240.0, h, true),
                        |ui, w, h| trend(ui, w, 240.0, h, false),
                    );
                } else {
                    trend(ui, full, 190.0, 0.0, true);
                    ui.add_space(GAP);
                    trend(ui, full, 130.0, 0.0, false);
                }
                ui.add_space(GAP);
                if full >= 1350.0 {
                    split(
                        ui,
                        "keyboard",
                        full,
                        0.6,
                        GAP,
                        |ui, w, h| keyboard_card(app, ui, w, h),
                        |ui, w, h| weak_card(app, ui, w, true, h),
                    );
                } else {
                    keyboard_card(app, ui, full, 0.0);
                    ui.add_space(GAP);
                    weak_card(app, ui, full, false, 0.0);
                }
                ui.add_space(GAP);
                ui.horizontal(|ui| {
                    if ui.button("Open the full report").clicked() {
                        app.open_report();
                    }
                    ui.add_space(8.0);
                    ui.label(
                        muted(&p, "learning curve, error types, drilled vs control items, and the insight model")
                            .size(13.0),
                    );
                });
                ui.add_space(24.0);
            });
        });
}

fn metrics(app: &App, ui: &mut Ui, full: f32) {
    let p = app.palette;
    let h = &app.history;
    let recent = h.recent(10);
    let avg = |f: fn(&(usize, f64, f64, f64, String)) -> f64| {
        (!recent.is_empty()).then(|| recent.iter().map(f).sum::<f64>() / recent.len() as f64)
    };
    let dash = || "–".to_string();
    let lvl = app.level.as_ref().filter(|l| l.lessons > 0);
    let items: [(&str, String, &str); 6] = [
        (
            "model level",
            lvl.map_or_else(dash, |l| format!("{:.0}", l.wpm)),
            "wpm",
        ),
        (
            "last 10",
            avg(|l| l.1).map_or_else(dash, |v| format!("{v:.0}")),
            "wpm",
        ),
        (
            "accuracy",
            avg(|l| 100.0 * l.2).map_or_else(dash, |v| format!("{v:.1}")),
            "%",
        ),
        (
            "best",
            h.best_wpm().map_or_else(dash, |v| format!("{v:.0}")),
            "wpm",
        ),
        (
            "practised",
            format!("{:.0}", h.total_seconds() / 60.0),
            "min",
        ),
        ("lessons", h.lessons.len().to_string(), ""),
    ];
    card(ui, &p, |ui| {
        ui.set_width(full - 40.0);
        if full >= 760.0 {
            // Spread evenly across the whole width.
            ui.columns(items.len(), |cols| {
                for (c, (label, value, unit)) in cols.iter_mut().zip(items.iter()) {
                    metric(c, &p, label, value, unit, 30.0);
                }
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(36.0, 12.0);
                for (label, value, unit) in &items {
                    metric(ui, &p, label, value, unit, 30.0);
                }
            });
        }
    });
}

fn keyboard_card(app: &App, ui: &mut Ui, w: f32, min_h: f32) -> f32 {
    let p = app.palette;
    card_h(ui, &p, min_h, |ui| {
        ui.set_width(w - 40.0);
        ui.label(eyebrow(&p, "Your keyboard"));
        ui.label(
            muted(
                &p,
                "Each key against your own baseline: a red bar is weaker than your usual, \
                 green is stronger; brighter means further from it.",
            )
            .size(13.5),
        );
        ui.add_space(10.0);
        keyboard(ui, &p, &app.layout, &app.key_weakness);
    })
    .1
}

fn weak_card(app: &App, ui: &mut Ui, w: f32, stacked: bool, min_h: f32) -> f32 {
    let p = app.palette;
    card_h(ui, &p, min_h, |ui| {
        ui.set_width(w - 40.0);
        ui.label(eyebrow(&p, "Weak spots the engine is working on"));
        ui.add_space(4.0);
        let lists = [
            ("Keys", rows(&app.weak_keys), p.error),
            ("Letter pairs", rows(&app.weak_bigrams), p.accent2),
            ("Words", rows(&app.weak_words), p.accent),
        ];
        if stacked {
            for (title, items, color) in &lists {
                ui.label(strong(*title, 16.0));
                bar_list(ui, &p, &items[..items.len().min(5)], *color);
                ui.add_space(6.0);
            }
        } else {
            ui.columns(3, |c| {
                for (col, (title, items, color)) in c.iter_mut().zip(lists.iter()) {
                    col.label(strong(*title, 16.0));
                    bar_list(col, &p, items, *color);
                }
            });
        }
        ui.add_space(4.0);
        ui.label(
            muted(
                &p,
                "Bar = weakness score (errors and slowness vs your baseline) · error rate · median time. \
                 ° marks a control item that is deliberately not drilled.",
            )
            .size(12.5),
        );
    })
    .1
}
