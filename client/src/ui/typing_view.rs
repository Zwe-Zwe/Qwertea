//! The typing screen: the lesson on a "sheet" with a progress edge, the
//! current character marked by an underline caret, and the lesson's training
//! targets tinted in the text still to come, so you can see what each lesson
//! is for. A small labelled stat strip sits under the sheet.
//!
//! Keys never reach egui here; they go straight to `LessonRun` in the key
//! handler. This function only draws (and handles the pause menu's buttons).

use super::theme::Palette;
use super::widgets::{eyebrow, muted, strong, target_chip};
use crate::app::{App, PausedAction};
use crate::profile::{CaretStyle, LiveStats};
use crate::session::{CharState, LessonRun, RunState};
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Stroke, Ui};

const VISIBLE_LINES: usize = 3;

fn display(c: char) -> &'static str {
    match c {
        '\n' => "↵\n",
        '\t' => "→   ",
        _ => "",
    }
}

/// Text positions covered by any occurrence of a target item (case-insensitive).
fn target_mask(target: &[char], items: &[String]) -> Vec<bool> {
    let low: Vec<char> = target.iter().map(|c| c.to_ascii_lowercase()).collect();
    let mut mask = vec![false; target.len()];
    for item in items {
        let pat: Vec<char> = item.to_lowercase().chars().collect();
        if pat.is_empty() || pat.len() > low.len() || pat.iter().all(|c| c.is_whitespace()) {
            continue;
        }
        for i in 0..=low.len() - pat.len() {
            if low[i..i + pat.len()] == pat[..] {
                for m in &mut mask[i..i + pat.len()] {
                    *m = true;
                }
            }
        }
    }
    mask
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let font_size = app.profile.font_size.clamp(14.0, 64.0);
    let live_stats = app.profile.live_stats;
    let caret_style = app.profile.caret;
    let smooth_caret = app.profile.smooth_caret;
    let family = super::theme::typing_family(app.profile.typing_font);
    let Some(a) = app.active.as_ref() else { return };
    let run = &a.run;
    let now_ns = a.origin.elapsed().as_nanos() as u64;
    let state = run.state();
    let title = a.lesson.title();
    let instruction = a.lesson.meta.instruction.clone();
    let targets: Vec<String> = a.lesson.targets().into_iter().map(|t| t.item).collect();
    let started = run.has_typed();
    let target = run.target();
    let mask = target_mask(target, &targets);

    let full = ui.available_rect_before_wrap();
    let sheet_w = (full.width() - 32.0).min(font_size * 0.6 * 70.0).max(320.0);
    let pad = vec2(36.0, 30.0);
    let text_w = sheet_w - 2.0 * pad.x;

    let line_h = font_size * 1.55;
    let text_h = line_h * VISIBLE_LINES as f32;
    let look = TextLook {
        font_size,
        family,
        caret: caret_style,
        smooth: smooth_caret,
    };

    // Sheet rectangle, a little above centre.
    let sheet_h = text_h + 2.0 * pad.y + 6.0;
    let sheet = Rect::from_center_size(
        pos2(full.center().x, full.center().y - 10.0),
        vec2(sheet_w, sheet_h),
    );
    let text_rect = Rect::from_min_size(sheet.min + pad + vec2(0.0, 6.0), vec2(text_w, text_h));

    let cursor = run.cursor();

    let painter = ui.painter().clone();

    // ---------------------------------------------------------------- header above the sheet
    let head_y = sheet.top() - 64.0;
    painter.text(
        pos2(sheet.left() + 4.0, head_y),
        Align2::LEFT_TOP,
        title.to_uppercase(),
        FontId::proportional(12.0),
        p.sub,
    );
    if !started {
        if let Some(instr) = &instruction {
            painter.text(
                pos2(sheet.left() + 4.0, head_y + 20.0),
                Align2::LEFT_TOP,
                instr,
                FontId::proportional(15.0),
                p.text,
            );
        }
    }
    painter.text(
        pos2(sheet.right() - 4.0, head_y),
        Align2::RIGHT_TOP,
        "ESC  PAUSE",
        FontId::proportional(12.0),
        p.sub,
    );
    if !targets.is_empty() {
        let mut chip_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    pos2(
                        sheet.left() + 4.0,
                        head_y + if started { 22.0 } else { 44.0 },
                    ),
                    vec2(sheet_w, 26.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        chip_ui.label(RichText::new("training").size(13.0).color(p.sub));
        for t in targets.iter().take(5) {
            target_chip(&mut chip_ui, &p, t);
        }
    }

    // ---------------------------------------------------------------- the sheet
    painter.rect_filled(sheet, CornerRadius::same(20), p.panel);
    let progress = if target.is_empty() {
        0.0
    } else {
        cursor as f32 / target.len() as f32
    };
    let edge = Rect::from_min_size(sheet.min + vec2(pad.x, 16.0), vec2(text_w, 3.0));
    painter.rect_filled(edge, CornerRadius::same(2), p.panel_hi);
    let mut done = edge;
    done.set_width(edge.width() * progress);
    painter.rect_filled(done, CornerRadius::same(2), p.accent);

    draw_text(ui, &p, run, &mask, text_rect, &look, "lesson");
    let clip = text_rect.expand2(vec2(6.0, 4.0));
    let tp = ui.painter_at(clip);
    if state == RunState::Paused {
        tp.rect_filled(clip, 0.0, p.panel);
    }

    // ---------------------------------------------------------------- stat strip
    let secs = if started {
        run.elapsed_ns(now_ns) as f64 / 1e9
    } else {
        0.0
    };
    let summary = run.summary();
    let mut items: Vec<(&str, String)> = Vec::new();
    if live_stats == LiveStats::Full {
        items.push((
            "speed",
            if started && secs > 1.0 {
                format!("{:.0} wpm", run.live_wpm(now_ns))
            } else {
                "–".into()
            },
        ));
    }
    if live_stats == LiveStats::Full {
        items.push((
            "accuracy",
            if started {
                format!("{:.0}%", 100.0 * summary.accuracy)
            } else {
                "–".into()
            },
        ));
    }
    if live_stats != LiveStats::Hidden {
        items.push(("time", super::clock(secs)));
        let (words_done, words_total) = run.word_progress();
        items.push(("words", format!("{words_done}/{words_total}")));
    }
    let strip_y = sheet.bottom() + 26.0;
    let slot = sheet_w / items.len().max(1) as f32;
    for (i, (label, value)) in items.iter().enumerate() {
        let x = sheet.left() + slot * (i as f32 + 0.5);
        painter.text(
            pos2(x, strip_y),
            Align2::CENTER_TOP,
            label.to_uppercase(),
            FontId::proportional(11.0),
            p.sub,
        );
        painter.text(
            pos2(x, strip_y + 16.0),
            Align2::CENTER_TOP,
            value,
            FontId::new(20.0, super::theme::bold()),
            p.text,
        );
    }

    // ---------------------------------------------------------------- pause menu
    if state == RunState::Paused {
        let mut action = None;
        let mut resume = false;
        let menu = Rect::from_center_size(sheet.center(), vec2(sheet_w - 80.0, 120.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(menu));
        child.vertical_centered(|ui| {
            ui.label(strong("Paused", 24.0).color(p.text));
            ui.label(muted(&p, "the clock is stopped").size(13.0));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let total = 4.0 * 150.0 + 3.0 * 10.0;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                let b = |ui: &mut Ui, t: &str| {
                    ui.add_sized([150.0, 34.0], egui::Button::new(t)).clicked()
                };
                if b(ui, "Resume  ·  Esc") {
                    resume = true;
                }
                if b(ui, "Restart  ·  R") {
                    action = Some(PausedAction::Restart);
                }
                if b(ui, "Skip  ·  S") {
                    action = Some(PausedAction::Skip);
                }
                if b(ui, "Leave  ·  Q") {
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
    let _ = eyebrow;
}

/// How lesson text is drawn: the profile's font, size and caret.
pub struct TextLook {
    pub font_size: f32,
    pub family: egui::FontFamily,
    pub caret: CaretStyle,
    pub smooth: bool,
}

/// Draw `run`'s text inside `text_rect`: typed characters coloured by
/// result, training targets tinted, and the caret in the chosen style. The
/// view scrolls so the caret's line is the second visible one. Shared by the
/// typing screen and the live demo in Settings, so both look the same.
pub fn draw_text(
    ui: &mut Ui,
    p: &Palette,
    run: &LessonRun,
    mask: &[bool],
    text_rect: Rect,
    look: &TextLook,
    salt: &str,
) {
    let p = *p;
    let font_size = look.font_size;
    let text_w = text_rect.width();
    let target = run.target();
    let state = run.state();
    let font = FontId::new(font_size, look.family.clone());
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: text_w,
            ..Default::default()
        },
        ..Default::default()
    };
    let fmt_for = |st: CharState, is_target: bool, is_space: bool| -> TextFormat {
        let mut f = TextFormat::simple(font.clone(), p.sub);
        f.line_height = Some(font_size * 1.55);
        match st {
            CharState::Pending if is_target => f.color = p.accent2,
            CharState::Pending => {}
            CharState::Correct => f.color = p.text,
            CharState::Wrong => {
                f.color = p.error;
                f.background = p.error_dim;
                if is_space {
                    f.underline = Stroke::new(2.0, p.error);
                }
            }
        }
        f
    };
    let mut disp_index = Vec::with_capacity(target.len() + 1);
    let mut d = 0usize;
    let mut buf = String::new();
    let mut cur: Option<(CharState, bool, bool)> = None;
    for (i, &c) in target.iter().enumerate() {
        let st = run.char_state(i);
        let key = (st, mask[i], c == ' ');
        let key = if st == CharState::Wrong {
            key
        } else {
            (key.0, key.1, false)
        };
        if cur != Some(key) {
            if let Some((s, t, sp)) = cur {
                job.append(&buf, 0.0, fmt_for(s, t, sp));
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
    if let Some((s, t, sp)) = cur {
        job.append(&buf, 0.0, fmt_for(s, t, sp));
    }
    disp_index.push(d);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let line_h = font_size * 1.55;
    let cursor = run.cursor();
    let caret = galley.pos_from_cursor(CCursor::new(disp_index[cursor.min(target.len())]));
    let caret_next =
        galley.pos_from_cursor(CCursor::new(disp_index[(cursor + 1).min(target.len())]));
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
    let origin = text_rect.min - vec2(0.0, scroll);
    let clip = text_rect.expand2(vec2(6.0, 4.0));
    let tp = ui.painter_at(clip);
    // Caret on the character to type next, in the profile's style.
    if state != RunState::Finished {
        let c = caret.translate(origin.to_vec2());
        let n = caret_next.translate(origin.to_vec2());
        let w = if (n.min.y - c.min.y).abs() < 1.0 && n.min.x > c.min.x {
            n.min.x - c.min.x
        } else {
            font_size * 0.6
        };
        let mut x = c.min.x;
        let mut cw = w;
        if look.smooth {
            // Glide along a line; jump straight to a new line (one
            // animation per row, so wrapping never sweeps across the text).
            let ctx = ui.ctx();
            let id = egui::Id::unique(("caret", salt, caret_row));
            x = ctx.animate_value_with_time(id.with("x"), x, 0.07);
            cw = ctx.animate_value_with_time(id.with("w"), w, 0.07);
        }
        let cell = Rect::from_min_size(
            pos2(x - 1.0, c.min.y + line_h * 0.12),
            vec2(cw + 2.0, line_h * 0.8),
        );
        let color = if run.blocked_error { p.error } else { p.accent };
        match look.caret {
            CaretStyle::Underline => {
                tp.rect_filled(cell, CornerRadius::same(4), color.gamma_multiply(0.14));
                tp.rect_filled(
                    Rect::from_min_size(
                        pos2(cell.left(), cell.bottom() - 2.5),
                        vec2(cell.width(), 3.0),
                    ),
                    CornerRadius::same(2),
                    color,
                );
            }
            CaretStyle::Block => {
                tp.rect_filled(cell, CornerRadius::same(4), color.gamma_multiply(0.42));
            }
            CaretStyle::Bar => {
                tp.rect_filled(
                    Rect::from_min_size(
                        pos2(cell.left() - 0.5, cell.top()),
                        vec2(2.5, cell.height()),
                    ),
                    CornerRadius::same(1),
                    color,
                );
            }
        }
    }
    tp.galley(origin, galley.clone(), Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::target_mask;

    #[test]
    fn targets_are_found_case_insensitively() {
        let t: Vec<char> = "You could".chars().collect();
        let m = target_mask(&t, &["ou".into(), " c".into()]);
        assert_eq!(
            m,
            vec![false, true, true, true, true, true, true, false, false]
        );
        assert!(target_mask(&t, &[" ".into()]).iter().all(|x| !x));
    }
}
