//! Progress: your level, how speed and accuracy are trending, where on the
//! keyboard you are weaker or stronger than your own baseline, and the
//! weakest keys, letter pairs and words from the engine's model.

use super::widgets::{area_chart, bar_list, card, eyebrow, keyboard, metric, muted, strong};
use crate::app::App;
use crate::db::ItemStat;
use egui::{vec2, ScrollArea, Ui};

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

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ScrollArea::vertical()
        .id_salt("stats_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let full = ui.available_width().min(1040.0);
            ui.label(eyebrow(&p, "Progress"));
            ui.label(strong("How your typing is changing", 30.0).color(p.text));
            ui.add_space(14.0);
            let h = &app.history;
            let recent = h.recent(10);
            let avg = |f: fn(&(usize, f64, f64, f64, String)) -> f64| {
                (!recent.is_empty()).then(|| recent.iter().map(f).sum::<f64>() / recent.len() as f64)
            };
            card(ui, &p, |ui| {
                ui.set_width(full - 40.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 40.0;
                    let lvl = app.level.as_ref().filter(|l| l.lessons > 0);
                    metric(ui, &p, "model level", &lvl.map_or("–".into(), |l| format!("{:.0}", l.wpm)), "wpm", 30.0);
                    metric(ui, &p, "last 10", &avg(|l| l.1).map_or("–".into(), |v| format!("{v:.0}")), "wpm", 30.0);
                    metric(
                        ui,
                        &p,
                        "accuracy",
                        &avg(|l| 100.0 * l.2).map_or("–".into(), |v| format!("{v:.1}")),
                        "%",
                        30.0,
                    );
                    metric(ui, &p, "best", &h.best_wpm().map_or("–".into(), |v| format!("{v:.0}")), "wpm", 30.0);
                    metric(ui, &p, "practised", &format!("{:.0}", h.total_seconds() / 60.0), "min", 30.0);
                    metric(ui, &p, "lessons", &h.lessons.len().to_string(), "", 30.0);
                });
            });
            ui.add_space(14.0);
            card(ui, &p, |ui| {
                ui.set_width(full - 40.0);
                ui.label(eyebrow(&p, "Speed trend · 10-lesson average"));
                let wpm: Vec<(f64, f64)> = h.lessons.iter().map(|l| (l.0 as f64, l.1)).collect();
                let smooth: Vec<(f64, f64)> = (0..wpm.len())
                    .map(|i| {
                        let w = &wpm[i.saturating_sub(9)..=i];
                        (wpm[i].0, w.iter().map(|x| x.1).sum::<f64>() / w.len() as f64)
                    })
                    .collect();
                area_chart(ui, &p, vec2(full - 40.0, 190.0), &smooth, p.accent, "wpm  ·  lessons", None);
                ui.add_space(6.0);
                ui.label(eyebrow(&p, "Accuracy trend"));
                let acc: Vec<(f64, f64)> = h.lessons.iter().map(|l| (l.0 as f64, 100.0 * l.2)).collect();
                let acc_s: Vec<(f64, f64)> = (0..acc.len())
                    .map(|i| {
                        let w = &acc[i.saturating_sub(9)..=i];
                        (acc[i].0, w.iter().map(|x| x.1).sum::<f64>() / w.len() as f64)
                    })
                    .collect();
                area_chart(ui, &p, vec2(full - 40.0, 120.0), &acc_s, p.accent2, "% first-try accuracy", None);
            });
            ui.add_space(14.0);
            card(ui, &p, |ui| {
                ui.set_width(full - 40.0);
                ui.label(eyebrow(&p, "Your keyboard"));
                ui.label(muted(
                    &p,
                    "Each key against your own baseline: a red bar is weaker than your usual, green is stronger; brighter means further from it.",
                )
                .size(13.5));
                ui.add_space(6.0);
                keyboard(ui, &p, &app.layout, &app.key_weakness);
            });
            ui.add_space(14.0);
            card(ui, &p, |ui| {
                ui.set_width(full - 40.0);
                ui.label(eyebrow(&p, "Weak spots the engine is working on"));
                ui.add_space(4.0);
                ui.columns(3, |c| {
                    c[0].label(strong("Keys", 16.0));
                    bar_list(&mut c[0], &p, &rows(&app.weak_keys), p.error);
                    c[1].label(strong("Letter pairs", 16.0));
                    bar_list(&mut c[1], &p, &rows(&app.weak_bigrams), p.accent2);
                    c[2].label(strong("Words", 16.0));
                    bar_list(&mut c[2], &p, &rows(&app.weak_words), p.accent);
                });
                ui.add_space(4.0);
                ui.label(muted(
                    &p,
                    "Bar = weakness score (errors and slowness vs your baseline) · error rate · median time. \
                     ° marks a control item that is deliberately not drilled.",
                )
                .size(12.5));
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Open the full report").clicked() {
                    app.open_report();
                }
                ui.label(muted(&p, "learning curve, error types, drilled vs control items, and the insight model").size(13.0));
            });
            ui.add_space(24.0);
        });
}
