//! Settings. Everything saves itself a moment after you change it:
//! * profile settings (look, lessons, typing) go to the open profile's
//!   database, so each person keeps their own (decision D28);
//! * this computer's settings go to `config.toml` (comments are kept).

use super::theme::{Palette, THEMES};
use super::widgets::{card, eyebrow, muted, primary_button, segmented, strong};
use crate::app::App;
use crate::config::{PresentModePref, TimestampSourceKind};
use crate::profile::{CaretStyle, LessonLength, LiveStats, SoundStyle, TypingFont};
use crate::session::ErrorMode;
use crate::update::{InstallKind, State as UpdateState};
use egui::{
    pos2, vec2, Align2, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};

/// A setting row: name and explanation on the left, the control on the right.
fn row(ui: &mut Ui, p: &Palette, name: &str, help: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() * 0.55).max(220.0));
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(name).size(15.5));
            if !help.is_empty() {
                ui.label(muted(p, help).size(12.5));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    ui.add_space(6.0);
}

/// A clickable theme preview: background, panel, text and the two accents.
fn theme_card(ui: &mut Ui, t: &Palette, selected: bool, active: &Palette) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 92.0), Sense::click());
    let pt = ui.painter();
    pt.rect_filled(rect, CornerRadius::same(12), t.bg);
    let inner = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(130.0, 44.0));
    pt.rect_filled(inner, CornerRadius::same(8), t.panel);
    pt.text(
        inner.min + vec2(10.0, 9.0),
        Align2::LEFT_TOP,
        "steep",
        FontId::monospace(14.0),
        t.text,
    );
    pt.text(
        inner.min + vec2(58.0, 9.0),
        Align2::LEFT_TOP,
        "ou",
        FontId::monospace(14.0),
        t.accent2,
    );
    pt.text(
        inner.min + vec2(80.0, 9.0),
        Align2::LEFT_TOP,
        "ve",
        FontId::monospace(14.0),
        t.sub,
    );
    pt.rect_filled(
        Rect::from_min_size(inner.min + vec2(10.0, 30.0), vec2(60.0, 3.0)),
        CornerRadius::same(2),
        t.accent,
    );
    pt.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        Align2::LEFT_BOTTOM,
        t.label,
        FontId::proportional(14.0),
        t.text,
    );
    let stroke = if selected {
        Stroke::new(2.5, active.accent)
    } else if resp.hovered() {
        Stroke::new(1.5, active.sub)
    } else {
        Stroke::new(1.0, active.panel_hi)
    };
    pt.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Outside);
    if selected {
        pt.circle_filled(
            pos2(rect.right() - 14.0, rect.bottom() - 16.0),
            5.0,
            active.accent,
        );
    }
    resp.clicked()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            super::widgets::page(ui, 920.0, |ui| {
                let width = ui.available_width();
                ui.label(eyebrow(&p, "Settings"));
                ui.label(strong("Make it yours", 30.0).color(p.text));
                ui.label(muted(
                    &p,
                    format!(
                        "Changes save automatically. Look, lessons and typing belong to {}'s profile.",
                        app.profile.name
                    ),
                ));
                ui.add_space(14.0);
                let mut dirty = false;

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Look"));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                        for t in THEMES {
                            let selected =
                                super::theme::palette(&app.profile.theme).name == t.name;
                            if theme_card(ui, &t, selected, &p) {
                                app.profile.theme = t.name.to_string();
                                dirty = true;
                            }
                        }
                    });
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Typing font & caret"));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                        let avail = ui.available_width() - 1.0;
                        let cols = ((avail + 12.0) / 242.0).floor().max(1.0);
                        let card_w = ((avail + 12.0) / cols - 12.0).floor();
                        for &f in TypingFont::ALL {
                            if font_card(ui, &p, f, app.profile.typing_font == f, card_w) {
                                app.profile.typing_font = f;
                                dirty = true;
                            }
                        }
                    });
                    ui.add_space(10.0);
                    ui.label(
                        muted(&p, "Try it: click the box and type. It uses your font, size, caret, mistakes mode and key sound.")
                            .size(12.5),
                    );
                    demo(app, ui, width - 40.0);
                    ui.add_space(10.0);
                    let s = &mut app.profile;
                    row(ui, &p, "Text size", "Size of the lesson text.", |ui| {
                        dirty |= ui
                            .add(
                                egui::Slider::new(&mut s.font_size, 16.0..=48.0)
                                    .step_by(1.0)
                                    .fixed_decimals(0),
                            )
                            .changed();
                    });
                    row(ui, &p, "Caret", "How the next character is marked.", |ui| {
                        let opts: Vec<_> = CaretStyle::ALL.iter().map(|c| (*c, c.label())).collect();
                        dirty |= segmented(ui, &p, &mut s.caret, &opts);
                    });
                    row(
                        ui,
                        &p,
                        "Smooth caret",
                        "The caret glides between characters instead of jumping.",
                        |ui| dirty |= ui.checkbox(&mut s.smooth_caret, "").changed(),
                    );
                    row(
                        ui,
                        &p,
                        "Live stats",
                        "What shows under the text while you type.",
                        |ui| {
                            let opts: Vec<_> =
                                LiveStats::ALL.iter().map(|c| (*c, c.label())).collect();
                            dirty |= segmented(ui, &p, &mut s.live_stats, &opts);
                        },
                    );
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Lessons"));
                    let s = &mut app.profile;
                    row(
                        ui,
                        &p,
                        "Lesson length",
                        "Short ≈ 30 s, medium ≈ 1 min, long ≈ 2 min at 40 wpm.",
                        |ui| {
                            if s.lesson_length == LessonLength::Custom {
                                dirty |= ui
                                    .add(
                                        egui::DragValue::new(&mut s.lesson_chars)
                                            .range(40..=2000)
                                            .speed(5)
                                            .suffix(" chars"),
                                    )
                                    .changed();
                            }
                            let opts: Vec<_> =
                                LessonLength::ALL.iter().map(|c| (*c, c.label())).collect();
                            dirty |= segmented(ui, &p, &mut s.lesson_length, &opts);
                        },
                    );
                    row(ui, &p, "Capital letters", "Off: lessons are all lower case.", |ui| {
                        dirty |= ui.checkbox(&mut s.capitals, "").changed()
                    });
                    row(ui, &p, "Punctuation", "Off: no commas, full stops or quotes.", |ui| {
                        dirty |= ui.checkbox(&mut s.punctuation, "").changed()
                    });
                    row(ui, &p, "Numbers", "Mix numbers into the text.", |ui| {
                        dirty |= ui.checkbox(&mut s.numbers, "").changed()
                    });
                    ui.label(
                        muted(
                            &p,
                            "New lessons are brewed with these settings within a few seconds. \
                             Calibration lessons stay as they are, so they measure every key.",
                        )
                        .size(12.5),
                    );
                });
                ui.add_space(14.0);

                let mut sound_changed = false;
                let mut preview = false;
                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Typing"));
                    let s = &mut app.profile;
                    row(ui, &p, "Mistakes", mistake_help(s.error_mode), |ui| {
                        let opts: Vec<_> = ErrorMode::ALL.iter().map(|c| (*c, c.label())).collect();
                        dirty |= segmented(ui, &p, &mut s.error_mode, &opts);
                    });
                    row(
                        ui,
                        &p,
                        "Key sounds",
                        "A sound for each key, a different one for a mistake.",
                        |_| {},
                    );
                    ui.horizontal_wrapped(|ui| {
                        let opts: Vec<_> =
                            SoundStyle::ALL.iter().map(|c| (*c, c.label())).collect();
                        if segmented(ui, &p, &mut s.sound, &opts) {
                            dirty = true;
                            sound_changed = true;
                            preview = s.sound != SoundStyle::Off;
                        }
                    });
                    if s.sound != SoundStyle::Off {
                        ui.add_space(6.0);
                        row(ui, &p, "Volume", "", |ui| {
                            let r = ui.add(
                                egui::Slider::new(&mut s.sound_volume, 0..=100).suffix(" %"),
                            );
                            if r.changed() {
                                dirty = true;
                                sound_changed = true;
                            }
                            preview |= r.drag_stopped() || (r.changed() && !r.dragged());
                        });
                    }
                    if let Some(e) = app
                        .sound
                        .error
                        .as_ref()
                        .filter(|_| app.profile.sound != SoundStyle::Off)
                    {
                        ui.label(RichText::new(format!("Sound unavailable: {e}")).color(p.error).size(12.5));
                    }
                });
                if sound_changed {
                    app.sync_sound();
                }
                if preview {
                    app.sound.click(false);
                }
                if dirty {
                    app.mark_profile_dirty();
                }
                ui.add_space(14.0);

                updates_card(app, ui, width);
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "This computer · timing applies after a restart"));
                    let mut dirty = false;
                    let c = &mut app.cfg.client;
                    row(
                        ui,
                        &p,
                        "Present mode",
                        "mailbox is smooth with low latency; fifo saves power.",
                        |ui| {
                            egui::ComboBox::from_id_salt("present")
                                .selected_text(c.present_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for m in PresentModePref::ALL {
                                        dirty |= ui
                                            .selectable_value(&mut c.present_mode, m, m.as_str())
                                            .changed();
                                    }
                                });
                        },
                    );
                    row(
                        ui,
                        &p,
                        "Timestamp source",
                        "evdev reads kernel timestamps (Linux, input group).",
                        |ui| {
                            egui::ComboBox::from_id_salt("tsrc")
                                .selected_text(c.timestamp_source.as_str())
                                .show_ui(ui, |ui| {
                                    for m in
                                        [TimestampSourceKind::Winit, TimestampSourceKind::Evdev]
                                    {
                                        dirty |= ui
                                            .selectable_value(
                                                &mut c.timestamp_source,
                                                m,
                                                m.as_str(),
                                            )
                                            .changed();
                                    }
                                });
                        },
                    );
                    row(
                        ui,
                        &p,
                        "Keyboard device",
                        "For evdev; empty picks the first keyboard.",
                        |ui| {
                            dirty |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut c.keyboard_device)
                                        .hint_text("/dev/input/event3")
                                        .desired_width(200.0),
                                )
                                .changed();
                        },
                    );
                    let layout = app.cfg.general.layout.clone();
                    row(
                        ui,
                        &p,
                        "Keyboard layout",
                        "Set general.layout in config.toml.",
                        |ui| {
                            ui.label(RichText::new(layout).monospace());
                        },
                    );
                    row(
                        ui,
                        &p,
                        "Start the engine automatically",
                        "The engine analyses lessons and brews new ones.",
                        |ui| dirty |= ui.checkbox(&mut app.cfg.engine.autostart, "").changed(),
                    );
                    if dirty {
                        app.mark_cfg_dirty();
                    }
                });
                ui.add_space(14.0);

                card(ui, &p, |ui| {
                    ui.set_width(width - 40.0);
                    ui.label(eyebrow(&p, "Engine & data"));
                    let status = app.engine.status.label();
                    row(ui, &p, "Engine status", "", |ui| {
                        ui.label(muted(&p, status));
                    });
                    let db = app.db_path().display().to_string();
                    let log = app.engine.log_path().display().to_string();
                    ui.label(muted(&p, format!("Database  {db}")).size(12.5));
                    ui.label(muted(&p, format!("Engine log  {log}")).size(12.5));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open full report").clicked() {
                            app.open_report();
                        }
                        let calibrating = app.calib.as_ref().is_some_and(|c| !c.is_complete());
                        if calibrating {
                            if app.confirm_skip_calibration {
                                if ui.button("Confirm: skip calibration").clicked() {
                                    app.skip_calibration();
                                    app.confirm_skip_calibration = false;
                                }
                            } else if ui.button("Skip remaining calibration").clicked() {
                                app.confirm_skip_calibration = true;
                            }
                        }
                    });
                });
                ui.add_space(16.0);
                let saved = app.profile_dirty_at.is_none() && app.cfg_dirty_at.is_none();
                ui.label(
                    muted(
                        &p,
                        format!(
                            "{} · Qwertea {} · everything stays on this computer.",
                            if saved { "All changes saved" } else { "Saving…" },
                            env!("CARGO_PKG_VERSION")
                        ),
                    )
                    .size(12.5),
                );
                ui.add_space(24.0);
            });
        });
}

fn mistake_help(m: ErrorMode) -> &'static str {
    match m {
        ErrorMode::Normal => "A wrong key is marked and the caret moves on.",
        ErrorMode::StopOnError => "The caret waits until you press the right key.",
        ErrorMode::StrictWord => "Fix every mistake (Backspace) before the next word.",
    }
}

/// Update status and actions; also used on the home screen when an update is ready.
pub fn updates_card(app: &mut App, ui: &mut Ui, width: f32) {
    let p = app.palette;
    card(ui, &p, |ui| {
        ui.set_width(width - 40.0);
        ui.label(eyebrow(&p, "Updates"));
        update_status(app, ui);
        ui.add_space(4.0);
        let mut check = app.cfg.updates.check;
        row(
            ui,
            &p,
            "Check for updates at start-up",
            "Asks GitHub for the latest version number. Nothing else is sent.",
            |ui| {
                if ui.checkbox(&mut check, "").changed() {
                    app.cfg.updates.check = check;
                    app.mark_cfg_dirty();
                }
            },
        );
    });
}

/// The current update state with the buttons that fit it.
pub fn update_status(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let state = app.updater.state();
    let kind = app.updater.kind.clone();
    let current = env!("CARGO_PKG_VERSION");
    match state {
        UpdateState::Idle | UpdateState::UpToDate | UpdateState::CheckFailed(_) => {
            ui.horizontal(|ui| {
                let msg = match &state {
                    UpdateState::UpToDate => format!("Qwertea {current} is the latest version."),
                    UpdateState::CheckFailed(e) => format!("Could not check: {e}."),
                    _ => format!("You have Qwertea {current} ({}).", kind.label()),
                };
                ui.label(RichText::new(msg).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Check now").clicked() {
                        app.updater.check();
                    }
                });
            });
        }
        UpdateState::Checking => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(muted(&p, "Checking for updates…"));
            });
        }
        UpdateState::Available { version } => {
            ui.label(
                RichText::new(format!(
                    "Qwertea {version} is available (you have {current})."
                ))
                .color(p.text)
                .size(16.0),
            );
            let url = app.updater.release_url(&version);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                match &kind {
                InstallKind::Source { .. } => {
                    if primary_button(ui, &p, "Update now", "").clicked() {
                        app.flush_settings();
                        app.updater.install();
                    }
                    ui.label(muted(&p, "Downloads, builds and installs it for you.").size(12.5));
                }
                InstallKind::ArchPackage => {
                    let cmd = app.updater.manual_command().unwrap_or_default();
                    ui.label(muted(&p, "Update it like your other packages:"));
                    ui.label(RichText::new(cmd).monospace().color(p.text));
                    if ui.button("Copy command").clicked() {
                        ui.ctx().copy_text(cmd.to_string());
                        app.toast("Copied. Paste it into a terminal, or update from your software manager.");
                    }
                }
                InstallKind::Other => {
                    ui.label(muted(&p, "Download the new version from the release page."));
                }
                }
            });
            if ui.link("What's new (release notes)").clicked() {
                if let Err(e) = crate::app::open_in_browser(std::path::Path::new(&url)) {
                    app.toast(format!("Could not open the browser: {e}"));
                }
            }
        }
        UpdateState::Installing { version, step } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("Updating to {version}: {step}…")).color(p.text));
            });
            ui.label(
                muted(
                    &p,
                    "You can keep typing; the app restarts when you are ready.",
                )
                .size(12.5),
            );
        }
        UpdateState::Installed { version } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.label(
                    RichText::new(format!("Qwertea {version} is installed."))
                        .color(p.good)
                        .size(16.0),
                );
                if primary_button(ui, &p, "Restart now", "").clicked() {
                    app.restart();
                }
            });
        }
        UpdateState::InstallFailed(e) => {
            ui.label(RichText::new(format!("The update did not finish: {e}")).color(p.error));
            ui.label(
                muted(
                    &p,
                    "Nothing was changed that stops this version working. From a terminal in the \
                     Qwertea folder, `git pull && make build` finishes it by hand.",
                )
                .size(12.5),
            );
            if ui.button("Try again").clicked() {
                app.updater.check();
            }
        }
    }
}

/// A typing font option, previewed in its own typeface.
fn font_card(ui: &mut Ui, p: &Palette, font: TypingFont, selected: bool, w: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 72.0), Sense::click());
    let pt = ui.painter_at(rect);
    pt.rect_filled(rect, CornerRadius::same(12), p.bg);
    pt.text(
        rect.min + vec2(14.0, 10.0),
        Align2::LEFT_TOP,
        font.label(),
        FontId::proportional(12.5),
        if selected { p.accent } else { p.sub },
    );
    pt.text(
        rect.min + vec2(14.0, 30.0),
        Align2::LEFT_TOP,
        "Typing 0O1lI {}",
        FontId::new(19.0, super::theme::typing_family(font)),
        p.text,
    );
    let stroke = if selected {
        Stroke::new(2.0, p.accent)
    } else if resp.hovered() {
        Stroke::new(1.5, p.sub)
    } else {
        Stroke::new(1.0, p.panel_hi)
    };
    pt.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Inside);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// The live typing demo: a two-line sheet that types like a real lesson.
fn demo(app: &mut App, ui: &mut Ui, width: f32) {
    use crate::app::Demo;
    use crate::session::{KeyKind, RunState};
    let p = app.palette;
    let mode = app.profile.error_mode;
    if app.demo.as_ref().is_none_or(|d| d.mode != mode) {
        app.demo = Some(Demo::new(mode));
    }
    let font_size = app.profile.font_size.clamp(14.0, 64.0);
    let line_h = font_size * 1.55;
    let pad = vec2(22.0, 16.0);
    let (rect, resp) =
        ui.allocate_exact_size(vec2(width, 2.0 * line_h + 2.0 * pad.y), Sense::click());
    if resp.clicked() {
        resp.request_focus();
    }
    let focused = resp.has_focus();
    // Keys go to the demo only while it has focus.
    if focused {
        let events = ui.input(|i| i.events.clone());
        let sound_on = app.profile.sound != SoundStyle::Off;
        let d = app.demo.as_mut().expect("demo exists");
        for e in events {
            match e {
                egui::Event::Text(t) => {
                    for c in t.chars() {
                        d.key(KeyKind::Text(c), &app.sound, sound_on);
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => d.key(KeyKind::Backspace, &app.sound, sound_on),
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    *d = Demo::new(mode);
                    resp.surrender_focus();
                }
                _ => {}
            }
        }
    }
    let d = app.demo.as_ref().expect("demo exists");
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(14), p.bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(14),
        if focused {
            Stroke::new(1.5, p.accent)
        } else {
            Stroke::new(1.0, p.panel_hi)
        },
        StrokeKind::Inside,
    );
    let look = super::typing_view::TextLook {
        font_size,
        family: super::theme::typing_family(app.profile.typing_font),
        caret: app.profile.caret,
        smooth: app.profile.smooth_caret,
    };
    let text_rect = rect.shrink2(pad);
    let mask = vec![false; d.run.target().len()];
    super::typing_view::draw_text(ui, &p, &d.run, &mask, text_rect, &look, "demo");
    let finished = d.run.state() == RunState::Finished;
    let hint = if !focused && !d.run.has_typed() {
        Some("Click here and start typing".to_string())
    } else if finished {
        let s = crate::stats::compute(crate::app::Demo::TEXT, d.run.events());
        Some(format!(
            "{:.0} wpm · {:.0}% accuracy · type to go again",
            s.wpm,
            100.0 * s.accuracy
        ))
    } else {
        None
    };
    if let Some(h) = hint {
        let painter = ui.painter();
        let r = Rect::from_center_size(rect.center(), vec2(rect.width(), rect.height()));
        painter.rect_filled(r, CornerRadius::same(14), p.bg.gamma_multiply(0.82));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            h,
            FontId::proportional(16.0),
            p.text,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::Text);
}
