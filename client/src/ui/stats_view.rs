//! Progress: WPM and accuracy history, keyboard weakness heatmap and the
//! weakest keys, bigrams and words from the engine's model.

use super::widgets::{card, keyboard, line_chart, muted, stat, Series};
use crate::app::App;
use crate::db::ItemStat;
use egui::{vec2, Grid, RichText, ScrollArea, Ui};

fn table(ui: &mut Ui, app: &App, id: &str, rows: &[ItemStat]) {
    let p = app.palette;
    if rows.is_empty() {
        ui.label(muted(&p, "not enough data yet"));
        return;
    }
    Grid::new(id)
        .striped(false)
        .spacing(vec2(18.0, 4.0))
        .show(ui, |ui| {
            for h in ["item", "error rate", "latency", "weakness"] {
                ui.label(muted(&p, h).size(12.0));
            }
            ui.end_row();
            for r in rows {
                let item = r.item.replace(' ', "·").replace('\n', "↵");
                let mut t = RichText::new(item).monospace().size(16.0);
                if r.is_holdout {
                    t = t.color(p.sub);
                }
                ui.label(t).on_hover_text(if r.is_holdout {
                    "holdout: deliberately not drilled, to measure whether drilling helps"
                } else {
                    ""
                });
                ui.label(format!("{:.1}%", 100.0 * r.p_error));
                ui.label(r.latency_ms.map_or("–".into(), |m| format!("{m:.0} ms")));
                let color = if r.weakness > 0.0 { p.error } else { p.good };
                ui.label(RichText::new(format!("{:+.2}", r.weakness)).color(color));
                ui.end_row();
            }
        });
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ScrollArea::vertical().id_salt("stats_scroll").auto_shrink([false, false]).show(ui, |ui| {
        let width = ui.available_width().min(1000.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            card(ui, &p, |ui| {
                ui.set_width(width - 32.0);
                let h = &app.history;
                let recent = h.recent(10);
                let avg = |f: fn(&(usize, f64, f64, f64, String)) -> f64| {
                    (!recent.is_empty())
                        .then(|| recent.iter().map(f).sum::<f64>() / recent.len() as f64)
                };
                ui.columns(5, |c| {
                    let lvl = app.level.as_ref().filter(|l| l.lessons > 0);
                    stat(
                        &mut c[0],
                        &p,
                        &lvl.map_or("–".into(), |l| format!("{:.0}", l.wpm)),
                        "level wpm (model)",
                        30.0,
                    );
                    stat(
                        &mut c[1],
                        &p,
                        &avg(|l| l.1).map_or("–".into(), |v| format!("{v:.0}")),
                        "avg wpm (last 10)",
                        30.0,
                    );
                    stat(
                        &mut c[2],
                        &p,
                        &avg(|l| 100.0 * l.2).map_or("–".into(), |v| format!("{v:.1}%")),
                        "avg accuracy",
                        30.0,
                    );
                    stat(
                        &mut c[3],
                        &p,
                        &h.best_wpm().map_or("–".into(), |v| format!("{v:.0}")),
                        "best wpm",
                        30.0,
                    );
                    stat(
                        &mut c[4],
                        &p,
                        &format!("{:.0} min", h.total_seconds() / 60.0),
                        "time typing",
                        30.0,
                    );
                });
            });
            ui.add_space(12.0);
            card(ui, &p, |ui| {
                ui.set_width(width - 32.0);
                ui.label(RichText::new("Speed").size(17.0));
                let wpm: Vec<(f64, f64)> = app
                    .history
                    .lessons
                    .iter()
                    .map(|l| (l.0 as f64, l.1))
                    .collect();
                // Moving average over 10 lessons.
                let avg: Vec<(f64, f64)> = (0..wpm.len())
                    .map(|i| {
                        let lo = i.saturating_sub(9);
                        let w = &wpm[lo..=i];
                        (wpm[i].0, w.iter().map(|x| x.1).sum::<f64>() / w.len() as f64)
                    })
                    .collect();
                line_chart(
                    ui,
                    &p,
                    vec2(ui.available_width(), 200.0),
                    &[
                        Series { points: &wpm, color: p.sub, width: 1.0, dots: true },
                        Series { points: &avg, color: p.accent, width: 2.5, dots: false },
                    ],
                    &[],
                    "wpm per lesson (dots) · 10-lesson average (line) · lessons",
                );
                ui.label(RichText::new("Accuracy").size(17.0));
                let acc: Vec<(f64, f64)> = app
                    .history
                    .lessons
                    .iter()
                    .map(|l| (l.0 as f64, 100.0 * l.2))
                    .collect();
                line_chart(
                    ui,
                    &p,
                    vec2(ui.available_width(), 140.0),
                    &[Series { points: &acc, color: p.good, width: 1.5, dots: true }],
                    &[],
                    "first-attempt accuracy % per lesson",
                );
            });
            ui.add_space(12.0);
            card(ui, &p, |ui| {
                ui.set_width(width - 32.0);
                ui.label(RichText::new("Keyboard").size(17.0));
                ui.label(muted(
                    &p,
                    "Weakness per key against your own baseline: red is weaker, green is stronger.",
                ));
                ui.add_space(6.0);
                keyboard(ui, &p, &app.layout, &app.key_weakness);
            });
            ui.add_space(12.0);
            card(ui, &p, |ui| {
                ui.set_width(width - 32.0);
                ui.columns(3, |c| {
                    c[0].label(RichText::new("Weakest keys").size(17.0));
                    table(&mut c[0], app, "keys", &app.weak_keys);
                    c[1].label(RichText::new("Weakest letter pairs").size(17.0));
                    table(&mut c[1], app, "bigrams", &app.weak_bigrams);
                    c[2].label(RichText::new("Weakest words").size(17.0));
                    table(&mut c[2], app, "words", &app.weak_words);
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Open full report").clicked() {
                    app.open_report();
                }
                ui.label(muted(
                    &p,
                    "learning curve, error types, drilling vs holdouts, and the LightGBM/SHAP insight model",
                ));
            });
            ui.add_space(20.0);
        });
    });
}
