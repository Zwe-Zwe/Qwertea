//! Screens. A left sidebar holds the brand, navigation and engine status; it
//! disappears while typing so only the lesson remains.

pub mod home;
pub mod profile_view;
pub mod settings;
pub mod stats_view;
pub mod summary;
pub mod theme;
pub mod typing_view;
pub mod widgets;

use crate::app::{App, Screen};
use egui::{pos2, vec2, Align2, CornerRadius, FontId, RichText, Sense, Ui};
use widgets::{avatar, icon, logo, strong, Icon};

pub fn show(app: &mut App, ui: &mut Ui) {
    app.apply_theme(ui.ctx());
    let p = app.palette;
    if app.screen != Screen::Typing {
        egui::Panel::left("nav")
            .exact_size(212.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(egui::Margin::symmetric(16, 22)),
            )
            .show(ui, |ui| sidebar(app, ui));
    }
    if let Some(msg) = app.banner.clone() {
        egui::Panel::bottom("banner")
            .frame(
                egui::Frame::new()
                    .fill(p.panel_hi)
                    .inner_margin(egui::Margin::symmetric(18, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(msg).color(p.text).size(13.0));
                    if ui.small_button("dismiss").clicked() {
                        app.banner = None;
                    }
                });
            });
    }
    let margin = if app.screen == Screen::Typing { 24 } else { 36 };
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(margin, 26)),
        )
        .show(ui, |ui| match app.screen {
            Screen::Home => home::show(app, ui),
            Screen::Intro => home::intro(app, ui),
            Screen::SittingDone => home::sitting_done(app, ui),
            Screen::Typing => typing_view::show(app, ui),
            Screen::Summary => summary::show(app, ui),
            Screen::Stats => stats_view::show(app, ui),
            Screen::Settings => settings::show(app, ui),
            Screen::Profile => profile_view::show(app, ui),
        });
    if let Some((msg, _)) = app.toast.clone() {
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -24.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(p.panel_hi)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| ui.label(RichText::new(msg).color(p.text)));
            });
    }
}

fn nav_item(ui: &mut Ui, app: &App, label: &str, which: Icon, active: bool) -> bool {
    let p = app.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let pt = ui.painter();
    if active {
        pt.rect_filled(rect, CornerRadius::same(10), p.panel_hi);
        pt.rect_filled(
            egui::Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(3.0, 20.0)),
            CornerRadius::same(2),
            p.accent,
        );
    } else if resp.hovered() {
        pt.rect_filled(rect, CornerRadius::same(10), p.panel_hi.gamma_multiply(0.6));
    }
    let color = if active { p.text } else { p.sub };
    icon(
        pt,
        pos2(rect.left() + 22.0, rect.center().y),
        18.0,
        which,
        if active { p.accent } else { p.sub },
    );
    pt.text(
        pos2(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(15.5),
        color,
    );
    resp.clicked()
}

fn sidebar(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    ui.horizontal(|ui| {
        logo(ui, &p, 34.0);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(strong("qwertea", 20.0).color(p.text));
            ui.label(
                RichText::new("typing, steeped in data")
                    .size(11.5)
                    .color(p.sub),
            );
        });
    });
    ui.add_space(22.0);
    // Who is typing: click to open the profile screen.
    let chip = ui.horizontal(|ui| {
        avatar(ui, &p, &app.profile.name, 30.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(&app.profile.name).size(14.5).color(p.text));
            let today = crate::profile::today();
            let mins = app.history.seconds_on(today) / 60.0;
            ui.label(
                RichText::new(format!("{mins:.0}/{} min today", app.profile.daily_minutes))
                    .size(11.5)
                    .color(p.sub),
            );
        });
    });
    let chip = ui.interact(
        chip.response.rect,
        egui::Id::unique("profile_chip"),
        Sense::click(),
    );
    if chip.clicked() {
        app.refresh_history();
        app.screen = Screen::Profile;
    }
    chip.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Profile, goals and switching");
    ui.add_space(18.0);
    let practice = matches!(
        app.screen,
        Screen::Home | Screen::Intro | Screen::Summary | Screen::SittingDone
    );
    if nav_item(ui, app, "Practice", Icon::Cup, practice) {
        app.refresh_queue();
        app.screen = Screen::Home;
    }
    if nav_item(
        ui,
        app,
        "Progress",
        Icon::Chart,
        app.screen == Screen::Stats,
    ) {
        app.refresh_history();
        app.screen = Screen::Stats;
    }
    if nav_item(
        ui,
        app,
        "Profile",
        Icon::Person,
        app.screen == Screen::Profile,
    ) {
        app.refresh_history();
        app.screen = Screen::Profile;
    }
    if nav_item(
        ui,
        app,
        "Settings",
        Icon::Sliders,
        app.screen == Screen::Settings,
    ) {
        app.screen = Screen::Settings;
    }
    if nav_item(ui, app, "Full report", Icon::Page, false) {
        app.open_report();
    }
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.label(
            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                .size(11.5)
                .color(p.sub),
        );
        let badge = match app.updater.state() {
            crate::update::State::Available { .. } => Some("Update available"),
            crate::update::State::Installing { .. } => Some("Updating…"),
            crate::update::State::Installed { .. } => Some("Restart to update"),
            _ => None,
        };
        if let Some(badge) = badge {
            let r = ui.add(
                egui::Button::new(RichText::new(badge).size(12.5).color(p.bg))
                    .fill(p.accent2)
                    .corner_radius(CornerRadius::same(8)),
            );
            if r.clicked() {
                app.screen = Screen::Settings;
            }
            ui.add_space(4.0);
        }
        let ok = app.engine.status.is_ok();
        let (color, text) = if ok {
            (p.good, "engine brewing")
        } else {
            (p.error, "engine offline")
        };
        let r = ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, color);
            ui.label(RichText::new(text).size(12.5).color(p.sub));
        });
        r.response.on_hover_text(app.engine.status.label());
    });
}

/// Maximum width of a page's content column; wider windows get even margins.
pub const PAGE_WIDTH: f32 = 1560.0;

/// Format seconds as m:ss.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
