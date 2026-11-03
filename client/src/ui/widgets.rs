//! Qwertea's widget kit: cards, eyebrow labels, metrics, target chips,
//! meters, a step tracker, charts (area, sparkline, rhythm strip), bar lists,
//! the keycap heatmap, the logo mark and the navigation icons.

use super::theme::{bold, Palette};
use crate::layout::Layout;
use crate::stats::WordSample;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui,
};
use std::collections::HashMap;

pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.panel)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(20))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add)
                .inner
        })
        .inner
}

/// Small, spaced, upper-case section label.
pub fn eyebrow(p: &Palette, text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(11.5)
        .color(p.sub)
        .extra_letter_spacing(1.4)
}

pub fn muted(p: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(p.sub)
}

pub fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into()).size(size).family(bold())
}

/// Eyebrow label, then a big value with a small unit next to it.
pub fn metric(ui: &mut Ui, p: &Palette, label: &str, value: &str, unit: &str, size: f32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(eyebrow(p, label));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(strong(value, size).color(p.text));
            if !unit.is_empty() {
                ui.label(RichText::new(unit).size(size * 0.45).color(p.sub));
            }
        });
    });
}

/// A training-target pill (the lesson's focus items use the secondary colour).
pub fn target_chip(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    let shown = text.replace(' ', "·").replace('\n', "↵");
    egui::Frame::new()
        .fill(p.accent2.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, p.accent2.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(shown).monospace().size(14.5).color(p.accent2))
                    .extend(),
            );
        })
        .response
}

/// A neutral pill.
pub fn chip(ui: &mut Ui, p: &Palette, text: &str) {
    egui::Frame::new()
        .fill(p.panel_hi)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).monospace().size(14.0).color(p.text)).extend(),
            );
        });
}

pub fn primary_button(ui: &mut Ui, p: &Palette, text: &str, hint: &str) -> Response {
    let label = if hint.is_empty() {
        RichText::new(text).size(16.5).family(bold()).color(p.bg)
    } else {
        RichText::new(format!("{text}   {hint}"))
            .size(16.5)
            .family(bold())
            .color(p.bg)
    };
    ui.add(
        egui::Button::new(label)
            .fill(p.accent)
            .corner_radius(CornerRadius::same(12))
            .min_size(vec2(180.0, 44.0)),
    )
}

pub fn quiet_button(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).color(p.sub)).frame(false))
}

/// Thin horizontal meter, `frac` in 0..=1.
pub fn meter(ui: &mut Ui, p: &Palette, frac: f32, color: Color32, width: f32, height: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let pt = ui.painter();
    let radius = CornerRadius::same((height / 2.0) as u8);
    pt.rect_filled(r, radius, p.panel_hi);
    let w = r.width() * frac.clamp(0.0, 1.0);
    if w > 0.5 {
        pt.rect_filled(
            Rect::from_min_size(r.min, vec2(w.max(height), height)),
            radius,
            color,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Done,
    Current,
    Todo,
}

/// Connected dots with labels: the phases of a calibration sitting.
pub fn step_tracker(ui: &mut Ui, p: &Palette, steps: &[(String, Step)]) {
    if steps.is_empty() {
        return;
    }
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 46.0), Sense::hover());
    let pt = ui.painter_at(rect);
    let n = steps.len();
    let gap = rect.width() / n as f32;
    let y = rect.top() + 10.0;
    for i in 0..n.saturating_sub(1) {
        let x0 = rect.left() + gap * (i as f32 + 0.5);
        let x1 = x0 + gap;
        let c = if steps[i + 1].1 != Step::Todo {
            p.accent
        } else {
            p.panel_hi
        };
        pt.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(2.0, c));
    }
    for (i, (label, st)) in steps.iter().enumerate() {
        let x = rect.left() + gap * (i as f32 + 0.5);
        match st {
            Step::Done => {
                pt.circle_filled(pos2(x, y), 6.0, p.accent);
            }
            Step::Current => {
                pt.circle_filled(pos2(x, y), 8.0, p.bg);
                pt.circle_stroke(pos2(x, y), 7.0, Stroke::new(2.5, p.accent));
            }
            Step::Todo => {
                pt.circle_filled(pos2(x, y), 5.0, p.panel_hi);
            }
        }
        pt.text(
            pos2(x, y + 20.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.0),
            if *st == Step::Current { p.text } else { p.sub },
        );
    }
}

fn nice_step(raw: f64) -> f64 {
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    let f = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    f * mag
}

/// Filled area chart with a few horizontal guides.
pub fn area_chart(
    ui: &mut Ui,
    p: &Palette,
    size: egui::Vec2,
    points: &[(f64, f64)],
    color: Color32,
    caption: &str,
    y_floor: Option<f64>,
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let pt = ui.painter_at(rect);
    let plot = Rect::from_min_max(rect.min + vec2(36.0, 6.0), rect.max - vec2(6.0, 20.0));
    if points.len() < 2 {
        pt.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "a few more lessons and your trend shows up here",
            FontId::proportional(14.0),
            p.sub,
        );
        return;
    }
    let (x0, x1) = (
        points[0].0,
        points[points.len() - 1].0.max(points[0].0 + 1.0),
    );
    let ymax = points.iter().map(|q| q.1).fold(f64::MIN, f64::max);
    let ymin_data = points.iter().map(|q| q.1).fold(f64::MAX, f64::min);
    let mut y0 = y_floor.unwrap_or_else(|| (ymin_data * 0.8).floor());
    let step = nice_step(((ymax - y0) / 3.0).max(1.0));
    y0 = (y0 / step).floor() * step;
    let y1 = ((ymax / step).ceil() * step).max(y0 + step);
    let tx = |x: f64| plot.left() + ((x - x0) / (x1 - x0)) as f32 * plot.width();
    let ty = |y: f64| plot.bottom() - ((y - y0) / (y1 - y0)) as f32 * plot.height();
    let mut y = y0;
    while y <= y1 + 1e-9 {
        let py = ty(y);
        pt.line_segment(
            [pos2(plot.left(), py), pos2(plot.right(), py)],
            Stroke::new(1.0, p.panel_hi),
        );
        pt.text(
            pos2(plot.left() - 8.0, py),
            Align2::RIGHT_CENTER,
            format!("{y:.0}"),
            FontId::proportional(11.0),
            p.sub,
        );
        y += step;
    }
    let line: Vec<Pos2> = points.iter().map(|&(x, y)| pos2(tx(x), ty(y))).collect();
    // Fill: one convex quad per segment down to the baseline.
    let fill = color.gamma_multiply(0.16);
    for w in line.windows(2) {
        pt.add(Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                pos2(w[1].x, plot.bottom()),
                pos2(w[0].x, plot.bottom()),
            ],
            fill,
            Stroke::NONE,
        ));
    }
    pt.line(line.clone(), Stroke::new(2.2, color));
    if let Some(last) = line.last() {
        pt.circle_filled(*last, 4.0, color);
    }
    pt.text(
        pos2(plot.left(), rect.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        caption,
        FontId::proportional(11.5),
        p.sub,
    );
}

pub fn sparkline(ui: &mut Ui, p: &Palette, values: &[f64], size: egui::Vec2, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if values.len() < 2 {
        return;
    }
    let pt = ui.painter_at(rect);
    let (lo, hi) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (hi - lo).max(1.0);
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            pos2(
                rect.left() + rect.width() * i as f32 / (values.len() - 1) as f32,
                rect.bottom() - 3.0 - (rect.height() - 6.0) * ((v - lo) / span) as f32,
            )
        })
        .collect();
    pt.line(pts.clone(), Stroke::new(1.8, color));
    if let Some(last) = pts.last() {
        pt.circle_filled(*last, 3.0, color);
    }
    let _ = p;
}

/// One bar per word: height = speed, colour = clean (accent) or with a slip (error).
/// A dashed line marks the lesson's average speed.
pub fn rhythm_strip(ui: &mut Ui, p: &Palette, words: &[WordSample], height: f32) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let pt = ui.painter_at(rect);
    let timed: Vec<&WordSample> = words.iter().filter(|w| w.wpm > 0.0).collect();
    if timed.is_empty() {
        return;
    }
    let max = timed.iter().map(|w| w.wpm).fold(0.0, f64::max).max(1.0) * 1.08;
    let avg = timed.iter().map(|w| w.wpm).sum::<f64>() / timed.len() as f64;
    let n = timed.len() as f32;
    let slot = rect.width() / n;
    let bar_w = (slot * 0.62).clamp(2.0, 16.0);
    let base = rect.bottom() - 4.0;
    let hover = resp.hover_pos();
    let mut hovered: Option<(&WordSample, Pos2)> = None;
    for (i, w) in timed.iter().enumerate() {
        let cx = rect.left() + slot * (i as f32 + 0.5);
        let h = ((w.wpm / max) as f32 * (rect.height() - 8.0)).max(3.0);
        let r = Rect::from_min_max(
            pos2(cx - bar_w / 2.0, base - h),
            pos2(cx + bar_w / 2.0, base),
        );
        let color = if w.errors > 0 { p.error } else { p.accent };
        let hot = hover.is_some_and(|h| (h.x - cx).abs() <= slot / 2.0);
        pt.rect_filled(
            r,
            CornerRadius::same(3),
            if hot {
                color
            } else {
                color.gamma_multiply(0.8)
            },
        );
        if hot {
            hovered = Some((w, pos2(cx, base - h)));
        }
    }
    let ay = base - (avg / max) as f32 * (rect.height() - 8.0);
    let mut x = rect.left();
    while x < rect.right() {
        pt.line_segment(
            [pos2(x, ay), pos2((x + 6.0).min(rect.right()), ay)],
            Stroke::new(1.0, p.text.gamma_multiply(0.45)),
        );
        x += 11.0;
    }
    if let Some((w, at)) = hovered {
        let label = format!(
            "{}  {:.0} wpm{}",
            w.text,
            w.wpm,
            if w.errors > 0 { "  · slip" } else { "" }
        );
        pt.text(
            pos2(
                at.x.clamp(rect.left() + 60.0, rect.right() - 60.0),
                (at.y - 12.0).max(rect.top() + 8.0),
            ),
            Align2::CENTER_BOTTOM,
            label,
            FontId::proportional(13.0),
            p.text,
        );
    }
}

/// Horizontal bars for ranked items: (label, value 0..1, right-hand text).
pub fn bar_list(ui: &mut Ui, p: &Palette, rows: &[(String, f32, String)], color: Color32) {
    if rows.is_empty() {
        ui.label(muted(p, "not enough data yet"));
        return;
    }
    let width = ui.available_width();
    let longest = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(2);
    let label_w = (longest as f32 * 9.3 + 12.0).clamp(40.0, width * 0.4);
    for (label, v, right) in rows {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
        let pt = ui.painter_at(rect);
        let right_w = 74.0;
        pt.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            label.replace(' ', "·"),
            FontId::monospace(15.0),
            p.text,
        );
        let track = Rect::from_min_max(
            pos2(rect.left() + label_w, rect.center().y - 4.0),
            pos2(rect.right() - right_w, rect.center().y + 4.0),
        );
        pt.rect_filled(track, CornerRadius::same(4), p.panel_hi);
        let mut fill = track;
        fill.set_width(track.width() * v.clamp(0.02, 1.0));
        pt.rect_filled(fill, CornerRadius::same(4), color);
        pt.text(
            pos2(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            right,
            FontId::proportional(12.5),
            p.sub,
        );
    }
}

/// Keycap heatmap: each key carries a coloured base strip, warm = weaker
/// than your baseline, cool = stronger; the number is the weakness score.
pub fn keyboard(ui: &mut Ui, p: &Palette, layout: &Layout, weakness: &HashMap<String, f64>) {
    let unit = (ui.available_width() / 15.2).clamp(24.0, 54.0);
    let size = vec2(unit * 15.2, unit * 5.4);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let pt = ui.painter_at(rect);
    let lim = weakness
        .values()
        .fold(1.0f64, |m, v| m.max(v.abs()))
        .min(4.0);
    for k in layout.visible_keys() {
        let w = if k.base == " " { 6.0 } else { 0.9 };
        let cx = rect.left() + k.x * unit;
        let cy = rect.top() + (k.row as f32 + 0.5) * unit * 1.04;
        let r = Rect::from_center_size(pos2(cx, cy), vec2(w * unit, 0.9 * unit));
        let v = weakness
            .get(&k.base)
            .copied()
            .or_else(|| weakness.get(&k.base.to_uppercase()).copied());
        pt.rect_filled(r, CornerRadius::same(8), p.panel_hi);
        if let Some(v) = v {
            let t = (v / lim).clamp(-1.0, 1.0) as f32;
            let color = if t >= 0.0 { p.error } else { p.good };
            let strip_h = 4.0 + 0.22 * unit * t.abs();
            let strip = Rect::from_min_max(pos2(r.left(), r.bottom() - strip_h), r.max);
            pt.rect_filled(
                strip,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                },
                color.gamma_multiply(0.35 + 0.65 * t.abs()),
            );
        }
        pt.rect_stroke(
            r,
            CornerRadius::same(8),
            Stroke::new(1.0, p.bg),
            StrokeKind::Inside,
        );
        let label = if k.base == " " {
            "space"
        } else {
            k.base.as_str()
        };
        let shows_value = v.is_some_and(|v| v.abs() >= 0.5);
        pt.text(
            r.center()
                - vec2(
                    0.0,
                    if shows_value {
                        unit * 0.13
                    } else {
                        unit * 0.04
                    },
                ),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(unit * 0.32),
            p.text,
        );
        if let Some(v) = v.filter(|_| shows_value) {
            pt.text(
                r.center() + vec2(0.0, unit * 0.15),
                Align2::CENTER_CENTER,
                format!("{v:+.1}"),
                FontId::proportional(unit * 0.18),
                p.sub,
            );
        }
    }
}

// ---------------------------------------------------------------- brand & icons

/// The Qwertea mark: a rounded tile with a "q" shaped like a cup and its handle,
/// with a wisp of steam.
pub fn logo(ui: &mut Ui, p: &Palette, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let pt = ui.painter();
    let s = size / 64.0;
    pt.rect_filled(rect, CornerRadius::same((14.0 * s) as u8), p.accent);
    let o = rect.min;
    let ink = p.bg;
    pt.circle_stroke(
        o + vec2(29.0, 34.0) * s,
        11.5 * s,
        Stroke::new(5.5 * s, ink),
    );
    pt.rect_filled(
        Rect::from_min_size(o + vec2(38.0, 23.0) * s, vec2(5.5, 31.0) * s),
        CornerRadius::same((2.7 * s) as u8),
        ink,
    );
    // Steam: two wisps.
    for x0 in [24.5f32, 32.0] {
        let steam: Vec<Pos2> = (0..=10)
            .map(|i| {
                let t = i as f32 / 10.0;
                o + vec2(
                    x0 + 1.6 * (t * std::f32::consts::TAU).sin(),
                    19.0 - 12.0 * t,
                ) * s
            })
            .collect();
        pt.line(steam, Stroke::new(2.2 * s, ink.gamma_multiply(0.8)));
    }
    resp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Cup,
    Chart,
    Sliders,
    Page,
}

pub fn icon(painter: &egui::Painter, center: Pos2, size: f32, which: Icon, color: Color32) {
    let s = size / 20.0;
    let st = Stroke::new(1.7 * s.max(0.8), color);
    let c = center;
    match which {
        Icon::Cup => {
            let body = Rect::from_min_max(c + vec2(-7.0, -3.0) * s, c + vec2(4.0, 7.0) * s);
            painter.rect_stroke(
                body,
                CornerRadius {
                    nw: 1,
                    ne: 1,
                    sw: 5,
                    se: 5,
                },
                st,
                StrokeKind::Middle,
            );
            painter.circle_stroke(c + vec2(6.0, 1.0) * s, 2.6 * s, st);
            painter.line_segment([c + vec2(-3.5, -8.0) * s, c + vec2(-3.5, -5.5) * s], st);
            painter.line_segment([c + vec2(0.5, -8.0) * s, c + vec2(0.5, -5.5) * s], st);
        }
        Icon::Chart => {
            for (i, h) in [4.0f32, 8.0, 12.0].iter().enumerate() {
                let x = c.x + (-6.0 + 6.0 * i as f32) * s;
                painter.line_segment(
                    [pos2(x, c.y + 7.0 * s), pos2(x, c.y + (7.0 - h) * s)],
                    Stroke::new(3.0 * s, color),
                );
            }
        }
        Icon::Sliders => {
            for (i, k) in [-3.0f32, 3.0, -1.0].iter().enumerate() {
                let y = c.y + (-6.0 + 6.0 * i as f32) * s;
                painter.line_segment([pos2(c.x - 8.0 * s, y), pos2(c.x + 8.0 * s, y)], st);
                painter.circle_filled(pos2(c.x + k * 2.0 * s, y), 2.6 * s, color);
            }
        }
        Icon::Page => {
            let r = Rect::from_center_size(c, vec2(12.0, 16.0) * s);
            painter.rect_stroke(r, CornerRadius::same(2), st, StrokeKind::Middle);
            for i in 0..3 {
                let y = r.top() + (5.0 + 3.5 * i as f32) * s;
                painter.line_segment(
                    [pos2(r.left() + 3.0 * s, y), pos2(r.right() - 3.0 * s, y)],
                    st,
                );
            }
        }
    }
}
