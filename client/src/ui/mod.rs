//! Screens. `show` draws the current screen; the top bar is hidden while typing.

pub mod home;
pub mod settings;
pub mod stats_view;
pub mod summary;
pub mod theme;
pub mod typing_view;
pub mod widgets;

use crate::app::{App, Screen};
use egui::{Align, Layout, RichText, Ui};

pub fn show(app: &mut App, ui: &mut Ui) {
    app.apply_theme(ui.ctx());
    let p = app.palette;
    if app.screen != Screen::Typing {
        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(28, 14)),
            )
            .show_separator_line(false)
            .show(ui, |ui| top_bar(app, ui));
    }
    if let Some(msg) = app.banner.clone() {
        egui::Panel::bottom("banner")
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(msg).color(p.sub).size(13.0));
                    if ui.small_button("dismiss").clicked() {
                        app.banner = None;
                    }
                });
            });
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(28, 12)),
        )
        .show(ui, |ui| match app.screen {
            Screen::Home => home::show(app, ui),
            Screen::Intro => home::intro(app, ui),
            Screen::SittingDone => home::sitting_done(app, ui),
            Screen::Typing => typing_view::show(app, ui),
            Screen::Summary => summary::show(app, ui),
            Screen::Stats => stats_view::show(app, ui),
            Screen::Settings => settings::show(app, ui),
        });
    if let Some((msg, _)) = app.toast.clone() {
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(p.panel_hi)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| ui.label(RichText::new(msg).color(p.text)));
            });
    }
}

fn top_bar(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("qwertea")
                .size(26.0)
                .color(p.accent)
                .monospace()
                .strong(),
        );
        ui.add_space(28.0);
        for (label, screen) in [
            ("practice", Screen::Home),
            ("stats", Screen::Stats),
            ("settings", Screen::Settings),
        ] {
            let current = app.screen == screen
                || (screen == Screen::Home
                    && matches!(
                        app.screen,
                        Screen::Intro | Screen::Summary | Screen::SittingDone
                    ));
            let text = RichText::new(label)
                .size(16.0)
                .color(if current { p.text } else { p.sub });
            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                if screen == Screen::Stats {
                    app.refresh_history();
                }
                if screen == Screen::Home {
                    app.refresh_queue();
                }
                app.screen = screen;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ok = app.engine.status.is_ok();
            let (dot, text) = if ok {
                (p.good, "engine ready")
            } else {
                (p.error, "engine offline")
            };
            let r = ui.label(RichText::new(format!("• {text}")).size(13.0).color(dot));
            r.on_hover_text(app.engine.status.label());
            if ui
                .add(
                    egui::Button::new(RichText::new("report").size(13.0).color(p.sub)).frame(false),
                )
                .on_hover_text("Open the full HTML report in your browser")
                .clicked()
            {
                app.open_report();
            }
        });
    });
}

/// Format seconds as m:ss.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
