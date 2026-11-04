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

/// A page column: at most `max_w` wide and centred, so wide or full-screen
/// windows get balanced margins instead of an empty band on one side.
pub fn page<R>(ui: &mut Ui, max_w: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_width();
    let w = avail.min(max_w);
    let pad = ((avail - w) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui)
        })
        .inner
    })
    .inner
}

/// Two columns side by side: `ratio` of the width (minus `gap`) on the left.
pub fn split(
    ui: &mut Ui,
    id_salt: &str,
    total: f32,
    ratio: f32,
    gap: f32,
    left: impl FnOnce(&mut Ui, f32, f32) -> f32,
    right: impl FnOnce(&mut Ui, f32, f32) -> f32,
) {
    // Both columns get the height of the taller one from the previous frame,
    // so side-by-side cards line up. Each closure returns its natural height,
    // so the row can also shrink when its content does.
    let id = ui.make_persistent_id(("split", id_salt));
    let row_h: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let lw = ((total - gap) * ratio).floor();
    let rw = total - gap - lw;
    let natural = ui
        .horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let a = ui
                .vertical(|ui| {
                    ui.set_width(lw);
                    left(ui, lw, row_h)
                })
                .inner;
            let b = ui
                .vertical(|ui| {
                    ui.set_width(rw);
                    right(ui, rw, row_h)
                })
                .inner;
            a.max(b)
        })
        .inner;
    if (natural - row_h).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, natural));
        ui.ctx().request_repaint();
    }
}

pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_h(ui, p, 0.0, add).0
}

/// A card at least `min_h` tall (outer height). Returns the closure's value
/// and the card's natural outer height (before stretching).
pub fn card_h<R>(ui: &mut Ui, p: &Palette, min_h: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, f32) {
    let pad = 20.0;
    let out = egui::Frame::new()
        .fill(p.panel)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(pad as i8))
        .show(ui, |ui| {
            let out = ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
            let natural = out.response.rect.height() + 2.0 * pad;
            let r = out.inner;
            if min_h > natural + 0.5 {
                // Pad with exact space (set_min_height over-stretches inside frames).
                ui.add_space(min_h - natural - ui.spacing().item_spacing.y);
            }
            (r, natural)
        });
    out.inner
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

/// A pill drawn as a single widget, so it wraps inside `horizontal_wrapped`
/// rows (framed containers do not).
fn pill(
    ui: &mut Ui,
    text: &str,
    size: f32,
    color: Color32,
    fill: Color32,
    stroke: Stroke,
) -> Response {
    let font = FontId::monospace(size);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, color));
    let pad = vec2(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::hover());
    let pt = ui.painter();
    pt.rect_filled(rect, CornerRadius::same(255), fill);
    if stroke.width > 0.0 {
        pt.rect_stroke(rect, CornerRadius::same(255), stroke, StrokeKind::Inside);
    }
    pt.galley(rect.min + pad, galley, color);
    resp
}

/// A training-target pill (the lesson's focus items use the secondary colour).
pub fn target_chip(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    let shown = text.replace(' ', "·").replace('\n', "↵");
    pill(
        ui,
        &shown,
        14.5,
        p.accent2,
        p.accent2.gamma_multiply(0.16),
        Stroke::new(1.0, p.accent2.gamma_multiply(0.55)),
    )
}

/// A neutral pill.
pub fn chip(ui: &mut Ui, p: &Palette, text: &str) {
    pill(ui, text, 14.0, p.text, p.panel_hi, Stroke::NONE);
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
    let label_font = FontId::monospace(15.0);
    let right_font = FontId::proportional(12.5);
    let measure = |t: &str, f: &FontId| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(t.to_owned(), f.clone(), p.text)
                .size()
                .x
        })
    };
    let label_w = rows
        .iter()
        .map(|r| measure(&r.0.replace(' ', "·"), &label_font))
        .fold(0.0, f32::max)
        + 12.0;
    let right_w = rows
        .iter()
        .map(|r| measure(&r.2, &right_font))
        .fold(0.0, f32::max)
        + 12.0;
    // Leave a gutter on the right so neighbouring columns never touch.
    let gutter = 14.0;
    for (label, v, right) in rows {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
        let pt = ui.painter_at(rect);
        pt.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            label.replace(' ', "·"),
            label_font.clone(),
            p.text,
        );
        let track = Rect::from_min_max(
            pos2(rect.left() + label_w, rect.center().y - 4.0),
            pos2(
                (rect.right() - gutter - right_w).max(rect.left() + label_w + 20.0),
                rect.center().y + 4.0,
            ),
        );
        pt.rect_filled(track, CornerRadius::same(4), p.panel_hi);
        let mut fill = track;
        fill.set_width(track.width() * v.clamp(0.02, 1.0));
        pt.rect_filled(fill, CornerRadius::same(4), color);
        pt.text(
            pos2(rect.right() - gutter, rect.center().y),
            Align2::RIGHT_CENTER,
            right,
            right_font.clone(),
            p.sub,
        );
    }
}

/// One drawn keycap, in key units (a letter key is 1 x 1).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyCap {
    pub label: String,
    /// The character this key types, for keys that are scored.
    pub base: Option<String>,
    pub row: usize,
    pub x: f32,
    pub w: f32,
}

/// Total width of every keyboard row, in key units (standard ANSI).
pub const BOARD_UNITS: f32 = 15.0;

/// A full keyboard: the layout's typing keys in row order, framed by the
/// usual modifier keys so every row is exactly `BOARD_UNITS` wide. If a row's
/// keys do not fill it, the last typing key takes up the slack (e.g. the
/// 1.5-wide backslash on ANSI keyboards).
pub fn keyboard_caps(layout: &Layout) -> Vec<KeyCap> {
    type Mods = &'static [(&'static str, f32)];
    const MODS: [(Mods, Mods); 5] = [
        (&[], &[("backspace", 2.0)]),
        (&[("tab", 1.5)], &[]),
        (&[("caps", 1.75)], &[("enter", 2.25)]),
        (&[("shift", 2.25)], &[("shift", 2.75)]),
        (
            &[("ctrl", 1.25), ("super", 1.25), ("alt", 1.25)],
            &[
                ("alt", 1.25),
                ("super", 1.25),
                ("menu", 1.25),
                ("ctrl", 1.25),
            ],
        ),
    ];
    let mut caps = Vec::new();
    for (row, (pre, post)) in MODS.iter().enumerate() {
        let mut keys: Vec<&crate::layout::Key> = layout
            .visible_keys()
            .filter(|k| k.row == row as i32)
            .collect();
        keys.sort_by(|a, b| a.x.total_cmp(&b.x));
        let fixed: f32 = pre.iter().chain(post.iter()).map(|m| m.1).sum();
        let n = keys.len() as f32;
        let mut widths = vec![1.0f32; keys.len()];
        let slack = BOARD_UNITS - fixed - n;
        if let Some(last) = widths.last_mut() {
            if slack > 0.0 {
                *last += slack;
            }
        }
        // If a layout has more keys than fit, shrink them evenly.
        let total: f32 = widths.iter().sum();
        let scale = if total > BOARD_UNITS - fixed && total > 0.0 {
            (BOARD_UNITS - fixed) / total
        } else {
            1.0
        };
        let mut x = 0.0;
        for (label, w) in pre.iter() {
            caps.push(KeyCap {
                label: (*label).into(),
                base: None,
                row,
                x,
                w: *w,
            });
            x += w;
        }
        for (k, w) in keys.iter().zip(widths) {
            let w = w * scale;
            let label = if k.base == " " {
                "space".to_string()
            } else {
                k.base.clone()
            };
            caps.push(KeyCap {
                label,
                base: Some(k.base.clone()),
                row,
                x,
                w,
            });
            x += w;
        }
        for (label, w) in post.iter() {
            caps.push(KeyCap {
                label: (*label).into(),
                base: None,
                row,
                x,
                w: *w,
            });
            x += w;
        }
    }
    caps
}

/// Keycap heatmap on a full keyboard. Scored keys carry a thin base strip:
/// warm = weaker than your own baseline, cool = stronger, more saturated =
/// further from the baseline; the number is the weakness score. Modifier
/// keys are drawn dimmed for orientation only.
pub fn keyboard(ui: &mut Ui, p: &Palette, layout: &Layout, weakness: &HashMap<String, f64>) {
    let avail = ui.available_width();
    let unit = (avail / BOARD_UNITS).clamp(22.0, 56.0);
    let gap = (unit * 0.08).max(2.0);
    let board = vec2(unit * BOARD_UNITS, unit * 5.0);
    let (outer, _) = ui.allocate_exact_size(vec2(avail, board.y), Sense::hover());
    let rect = Rect::from_min_size(pos2(outer.center().x - board.x / 2.0, outer.top()), board);
    let pt = ui.painter_at(outer);
    let lim = weakness
        .values()
        .fold(1.0f64, |m, v| m.max(v.abs()))
        .min(4.0);
    let radius = CornerRadius::same((unit * 0.16) as u8);
    for cap in keyboard_caps(layout) {
        let r = Rect::from_min_size(
            rect.min + vec2(cap.x * unit + gap / 2.0, cap.row as f32 * unit + gap / 2.0),
            vec2(cap.w * unit - gap, unit - gap),
        );
        let Some(base) = &cap.base else {
            pt.rect_filled(r, radius, p.panel_hi.gamma_multiply(0.45));
            pt.text(
                pos2(r.left() + unit * 0.14, r.bottom() - unit * 0.14),
                Align2::LEFT_BOTTOM,
                &cap.label,
                FontId::proportional(unit * 0.2),
                p.sub.gamma_multiply(0.7),
            );
            continue;
        };
        let v = weakness
            .get(base)
            .copied()
            .or_else(|| weakness.get(&base.to_uppercase()).copied());
        pt.rect_filled(r, radius, p.panel_hi);
        if let Some(v) = v {
            let t = (v / lim).clamp(-1.0, 1.0) as f32;
            let color = if t >= 0.0 { p.error } else { p.good };
            let strip = Rect::from_min_max(
                pos2(r.left() + unit * 0.12, r.bottom() - unit * 0.12 - 3.0),
                pos2(r.right() - unit * 0.12, r.bottom() - unit * 0.12),
            );
            pt.rect_filled(
                strip,
                CornerRadius::same(2),
                color.gamma_multiply(0.3 + 0.7 * t.abs()),
            );
        }
        let shows_value = v.is_some_and(|v| v.abs() >= 0.5);
        pt.text(
            r.center()
                - vec2(
                    0.0,
                    if shows_value {
                        unit * 0.12
                    } else {
                        unit * 0.04
                    },
                ),
            Align2::CENTER_CENTER,
            &cap.label,
            FontId::monospace(unit * if cap.label.len() > 1 { 0.26 } else { 0.32 }),
            p.text,
        );
        if let Some(v) = v.filter(|_| shows_value) {
            pt.text(
                r.center() + vec2(0.0, unit * 0.14),
                Align2::CENTER_CENTER,
                format!("{v:+.1}"),
                FontId::proportional(unit * 0.17),
                if v > 0.0 { p.error } else { p.good },
            );
        }
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    #[test]
    fn every_row_is_full_width_and_gapless() {
        let caps = keyboard_caps(&Layout::builtin());
        for row in 0..5 {
            let mut keys: Vec<&KeyCap> = caps.iter().filter(|c| c.row == row).collect();
            keys.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut x = 0.0;
            for k in &keys {
                assert!((k.x - x).abs() < 1e-4, "row {row}: gap before {}", k.label);
                x += k.w;
            }
            assert!((x - BOARD_UNITS).abs() < 1e-4, "row {row} is {x} wide");
        }
    }

    #[test]
    fn ansi_widths() {
        let caps = keyboard_caps(&Layout::builtin());
        let w = |label: &str| caps.iter().find(|c| c.label == label).map(|c| c.w).unwrap();
        assert_eq!(w("\\"), 1.5);
        assert_eq!(w("space"), 6.25);
        assert_eq!(w("enter"), 2.25);
        assert_eq!(w("q"), 1.0);
        assert_eq!(caps.iter().filter(|c| c.base.is_some()).count(), 48);
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
    Person,
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
        Icon::Person => {
            painter.circle_stroke(c + vec2(0.0, -3.5) * s, 3.6 * s, st);
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 12.0);
                    c + vec2(-7.0 * a.cos(), 8.0 - 5.5 * a.sin()) * s
                })
                .collect();
            painter.add(egui::Shape::line(pts, st));
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

/// A row of mutually exclusive options. Returns true when the choice changed.
/// Works in left-to-right and right-to-left layouts (options keep their order).
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut draw = |ui: &mut Ui, v: T, label: &str| {
            let sel = *value == v;
            let text = RichText::new(label)
                .size(14.0)
                .color(if sel { p.bg } else { p.text });
            let b = egui::Button::new(text)
                .fill(if sel { p.accent } else { p.panel_hi })
                .corner_radius(CornerRadius::same(8))
                .min_size(vec2(0.0, 30.0));
            if ui.add(b).clicked() && !sel {
                *value = v;
                changed = true;
            }
        };
        if rtl {
            for (v, l) in options.iter().rev() {
                draw(ui, *v, l);
            }
        } else {
            for (v, l) in options {
                draw(ui, *v, l);
            }
        }
    });
    changed
}

/// A round avatar with the profile's initial.
pub fn avatar(ui: &mut Ui, p: &Palette, name: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let initial: String = name
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into());
    let pt = ui.painter();
    pt.circle_filled(rect.center(), size / 2.0, p.accent.gamma_multiply(0.22));
    pt.circle_stroke(rect.center(), size / 2.0 - 0.75, Stroke::new(1.5, p.accent));
    pt.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::new(size * 0.46, bold()),
        p.accent,
    );
    resp
}
