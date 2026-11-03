//! The typing screen: the lesson text with the caret on the next expected
//! character, three visible lines that scroll with the caret, and a minimal
//! HUD (time, live WPM, progress). Everything else is hidden.
//!
//! Keys never reach egui here; they go straight to `LessonRun` in the key
//! handler. This function only draws (and handles the pause menu's buttons).

use super::widgets::muted;
use crate::app::{App, PausedAction};
use crate::session::{CharState, RunState};
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, RichText, Stroke, Ui};

const VISIBLE_LINES: usize = 3;

/// What to draw for one target character, and how many display chars it takes.
fn display(c: char) -> &'static str {
    match c {
        '\n' => "↵\n",
        '\t' => "→   ",
        _ => "",
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let font_size = app.cfg.client.font_size.clamp(14.0, 64.0);
    let live_wpm = app.cfg.client.live_wpm;
    let Some(a) = app.active.as_ref() else { return };
    let run = &a.run;
    let now_ns = a.origin.elapsed().as_nanos() as u64;
    let state = run.state();
    let title = a.lesson.title();
    let instruction = a.lesson.meta.instruction.clone();
    let started = run.has_typed();

    let full = ui.available_rect_before_wrap();
    let width = (full.width() - 40.0)
        .min(font_size * 0.62 * 64.0)
        .max(200.0);
    let left = full.center().x - width / 2.0;

    // ---------------------------------------------------------------- text layout
    let font = FontId::monospace(font_size);
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: width,
            ..Default::default()
        },
        ..Default::default()
    };
    let target = run.target();
    let mut disp_index = Vec::with_capacity(target.len() + 1);
    let mut d = 0usize;
    let fmt_for = |st: CharState, is_space: bool| -> TextFormat {
        let mut f = TextFormat::simple(font.clone(), p.sub);
        match st {
            CharState::Pending => {}
            CharState::Correct => f.color = p.text,
            CharState::Wrong => {
                f.color = p.error;
                if is_space {
                    f.background = p.error_dim;
                }
                f.underline = Stroke::new(2.0, p.error);
            }
        }
        f
    };
    // Group consecutive characters with the same look into one section.
    let mut buf = String::new();
    let mut cur: Option<(CharState, bool)> = None;
    for (i, &c) in target.iter().enumerate() {
        let st = run.char_state(i);
        let key = (st, c == ' ' && st == CharState::Wrong);
        if cur != Some(key) {
            if let Some((s, sp)) = cur {
                job.append(&buf, 0.0, fmt_for(s, sp));
                buf.clear();
            }
            cur = Some(key);
        }
        disp_index.push(d);
        let shown = display(c);
        if shown.is_empty() {
            buf.push(c);
            d += 1;
        } else {
            buf.push_str(shown);
            d += shown.chars().count();
        }
    }
    if let Some((s, sp)) = cur {
        job.append(&buf, 0.0, fmt_for(s, sp));
    }
    disp_index.push(d);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let line_h = galley
        .rows
        .first()
        .map(|r| r.rect().height())
        .unwrap_or(font_size * 1.3)
        * 1.0;
    let text_h = line_h * VISIBLE_LINES as f32;
    let top = full.center().y - text_h / 2.0 - 10.0;
    let text_rect = Rect::from_min_size(pos2(left, top), vec2(width, text_h));

    // Caret position and the row it is on.
    let cursor = run.cursor();
    let caret = galley.pos_from_cursor(CCursor::new(disp_index[cursor.min(target.len())]));
    let caret_row = galley
        .rows
        .iter()
        .position(|r| caret.center().y < r.rect().max.y)
        .unwrap_or(0);
    let first_row = caret_row.saturating_sub(1);
    let scroll = galley
        .rows
        .get(first_row)
        .map(|r| r.rect().min.y)
        .unwrap_or(0.0);
    let origin = pos2(left, top - scroll);

    // ---------------------------------------------------------------- HUD
    let painter = ui.painter();
    let hud_y = top - font_size * 1.4;
    let secs = run.active_ns(now_ns) as f64 / 1e9;
    let mut hud = format!(
        "{}   {}/{}",
        super::clock(if started { secs } else { 0.0 }),
        cursor,
        target.len()
    );
    if live_wpm && started && secs > 1.0 {
        hud.push_str(&format!("   {:.0} wpm", run.live_wpm(now_ns)));
    }
    painter.text(
        pos2(left, hud_y),
        Align2::LEFT_CENTER,
        hud,
        FontId::monospace(font_size * 0.7),
        p.accent,
    );
    if !started && state == RunState::Running {
        painter.text(
            pos2(full.center().x, top - font_size * 3.2),
            Align2::CENTER_CENTER,
            &title,
            FontId::proportional(20.0),
            p.text,
        );
        if let Some(instr) = &instruction {
            painter.text(
                pos2(full.center().x, top - font_size * 3.2 + 26.0),
                Align2::CENTER_CENTER,
                instr,
                FontId::proportional(14.0),
                p.sub,
            );
        }
    }

    // ---------------------------------------------------------------- text + caret
    let clip = text_rect.expand2(vec2(4.0, 2.0));
    let text_painter = ui.painter_at(clip);
    let dim = if state == RunState::Paused { 0.25 } else { 1.0 };
    text_painter.galley(origin, galley.clone(), Color32::WHITE);
    if dim < 1.0 {
        text_painter.rect_filled(clip, 0.0, p.bg.gamma_multiply(0.85));
    }
    if state != RunState::Finished {
        let c = caret.translate(origin.to_vec2());
        let color = if a.run.blocked_error {
            p.error
        } else {
            p.accent
        };
        text_painter.rect_filled(
            Rect::from_min_max(
                pos2(c.min.x - 1.0, c.min.y + line_h * 0.12),
                pos2(c.min.x + 1.5, c.max.y - line_h * 0.08),
            ),
            1.0,
            color,
        );
    }

    // ---------------------------------------------------------------- footer
    painter.text(
        pos2(full.center().x, full.bottom() - 16.0),
        Align2::CENTER_CENTER,
        "esc — pause",
        FontId::proportional(13.0),
        p.sub,
    );

    // ---------------------------------------------------------------- pause menu
    if state == RunState::Paused {
        let mut action = None;
        let mut resume = false;
        let menu = Rect::from_center_size(full.center(), vec2(340.0, 230.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(menu));
        egui::Frame::new()
            .fill(p.panel)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(18))
            .show(&mut child, |ui| {
                ui.set_width(304.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Paused").size(26.0).color(p.accent));
                    ui.label(muted(&p, "paused time is not counted"));
                    ui.add_space(8.0);
                    let b = |ui: &mut Ui, t: &str| {
                        ui.add_sized([240.0, 30.0], egui::Button::new(t)).clicked()
                    };
                    if b(ui, "Resume   (esc)") {
                        resume = true;
                    }
                    if b(ui, "Restart lesson   (r)") {
                        action = Some(PausedAction::Restart);
                    }
                    if b(ui, "Skip lesson   (s)") {
                        action = Some(PausedAction::Skip);
                    }
                    if b(ui, "Quit to menu   (q)") {
                        action = Some(PausedAction::Quit);
                    }
                });
            });
        if resume {
            app.resume();
        }
        if action.is_some() {
            app.paused_action = action;
        }
    }
}
