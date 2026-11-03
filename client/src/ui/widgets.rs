//! Small drawing helpers: stat cards, chips, line charts and the keyboard heatmap.

use super::theme::Palette;
use crate::layout::Layout;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui,
};
use std::collections::HashMap;

pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.panel)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add)
                .inner
        })
        .inner
}

/// Big number with a small label under it.
pub fn stat(ui: &mut Ui, p: &Palette, value: &str, label: &str, big: f32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).size(big).color(p.accent));
        ui.label(RichText::new(label).size(13.0).color(p.sub));
    });
}

pub fn chip(ui: &mut Ui, p: &Palette, text: &str) {
    egui::Frame::new()
        .fill(p.panel_hi)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(14.0).color(p.text));
        });
}

pub fn primary_button(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).size(17.0).color(p.bg).strong())
        .fill(p.accent)
        .corner_radius(CornerRadius::same(10))
        .min_size(vec2(160.0, 42.0));
    ui.add(btn)
}

pub fn muted(p: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(p.sub)
}

pub struct Series<'a> {
    pub points: &'a [(f64, f64)],
    pub color: Color32,
    pub width: f32,
    pub dots: bool,
}

/// A simple line chart with horizontal grid lines and y labels.
pub fn line_chart(
    ui: &mut Ui,
    p: &Palette,
    size: egui::Vec2,
    series: &[Series<'_>],
    markers: &[(f64, Color32)],
    y_label: &str,
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = Rect::from_min_max(rect.min + vec2(40.0, 8.0), rect.max - vec2(8.0, 22.0));
    let all: Vec<(f64, f64)> = series
        .iter()
        .flat_map(|s| s.points.iter().copied())
        .collect();
    if all.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "no data yet",
            FontId::proportional(14.0),
            p.sub,
        );
        return;
    }
    let (mut x0, mut x1) = (f64::MAX, f64::MIN);
    let (mut y0, mut y1) = (0.0f64, f64::MIN);
    for &(x, y) in &all {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if x1 <= x0 {
        x1 = x0 + 1.0;
    }
    let step = nice_step((y1 - y0).max(1.0) / 4.0);
    y1 = (y1 / step).ceil() * step;
    let tx = |x: f64| plot.left() + ((x - x0) / (x1 - x0)) as f32 * plot.width();
    let ty = |y: f64| plot.bottom() - ((y - y0) / (y1 - y0).max(1e-9)) as f32 * plot.height();
    let mut y = y0;
    while y <= y1 + 1e-9 {
        let py = ty(y);
        painter.line_segment(
            [pos2(plot.left(), py), pos2(plot.right(), py)],
            Stroke::new(1.0, p.panel_hi),
        );
        painter.text(
            pos2(plot.left() - 6.0, py),
            Align2::RIGHT_CENTER,
            format!("{y:.0}"),
            FontId::proportional(11.0),
            p.sub,
        );
        y += step;
    }
    painter.text(
        pos2(plot.left(), rect.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        y_label,
        FontId::proportional(11.0),
        p.sub,
    );
    for &(x, color) in markers {
        let px = tx(x);
        painter.line_segment(
            [pos2(px, plot.top()), pos2(px, plot.bottom())],
            Stroke::new(1.0, color.gamma_multiply(0.5)),
        );
    }
    for s in series {
        let pts: Vec<egui::Pos2> = s.points.iter().map(|&(x, y)| pos2(tx(x), ty(y))).collect();
        if pts.len() >= 2 {
            painter.line(pts.clone(), Stroke::new(s.width, s.color));
        }
        if s.dots || pts.len() == 1 {
            for q in pts {
                painter.circle_filled(q, 2.5, s.color);
            }
        }
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

/// Keyboard heatmap: red = weaker than your baseline, green = stronger.
pub fn keyboard(ui: &mut Ui, p: &Palette, layout: &Layout, weakness: &HashMap<String, f64>) {
    let unit = (ui.available_width() / 15.2).clamp(24.0, 52.0);
    let size = vec2(unit * 15.2, unit * 5.3);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let lim = weakness
        .values()
        .fold(1.0f64, |m, v| m.max(v.abs()))
        .min(4.0);
    for k in layout.visible_keys() {
        let w = if k.base == " " { 6.0 } else { 0.92 };
        let cx = rect.left() + k.x * unit;
        let cy = rect.top() + (k.row as f32 + 0.5) * unit * 1.02;
        let r = Rect::from_center_size(pos2(cx, cy), vec2(w * unit, 0.92 * unit));
        let v = weakness
            .get(&k.base)
            .copied()
            .or_else(|| weakness.get(&k.base.to_uppercase()).copied());
        let fill = match v {
            None => p.panel,
            Some(v) => {
                let t = (v / lim).clamp(-1.0, 1.0) as f32;
                if t >= 0.0 {
                    lerp(p.panel_hi, p.error, t)
                } else {
                    lerp(p.panel_hi, p.good, -t)
                }
            }
        };
        painter.rect_filled(r, CornerRadius::same(6), fill);
        painter.rect_stroke(
            r,
            CornerRadius::same(6),
            Stroke::new(1.0, p.panel_hi),
            StrokeKind::Inside,
        );
        let label = if k.base == " " {
            "space"
        } else {
            k.base.as_str()
        };
        painter.text(
            r.center() - vec2(0.0, if v.is_some() { 5.0 } else { 0.0 }),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(unit * 0.36),
            p.text,
        );
        if let Some(v) = v {
            painter.text(
                r.center() + vec2(0.0, unit * 0.24),
                Align2::CENTER_CENTER,
                format!("{v:+.1}"),
                FontId::proportional(unit * 0.2),
                p.text.gamma_multiply(0.7),
            );
        }
    }
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}
