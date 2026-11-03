//! Colour themes and egui style setup. Calm, high-contrast, dark by default.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub name: &'static str,
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub text: Color32,
    pub sub: Color32,
    pub accent: Color32,
    pub error: Color32,
    pub error_dim: Color32,
    pub good: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const THEMES: [Palette; 4] = [
    Palette {
        name: "dark",
        dark: true,
        bg: rgb(0x2b, 0x2d, 0x31),
        panel: rgb(0x32, 0x34, 0x39),
        panel_hi: rgb(0x3c, 0x3f, 0x45),
        text: rgb(0xe3, 0xe1, 0xd8),
        sub: rgb(0x6f, 0x72, 0x7a),
        accent: rgb(0xe2, 0xb7, 0x14),
        error: rgb(0xe0, 0x57, 0x5b),
        error_dim: rgb(0x8f, 0x3a, 0x3d),
        good: rgb(0x6a, 0xb0, 0x7f),
    },
    Palette {
        name: "light",
        dark: false,
        bg: rgb(0xf4, 0xf1, 0xea),
        panel: rgb(0xe9, 0xe5, 0xdb),
        panel_hi: rgb(0xdd, 0xd8, 0xcc),
        text: rgb(0x2b, 0x2a, 0x27),
        sub: rgb(0x9a, 0x96, 0x8c),
        accent: rgb(0xc0, 0x6a, 0x10),
        error: rgb(0xc8, 0x3a, 0x3a),
        error_dim: rgb(0xe6, 0xa4, 0xa0),
        good: rgb(0x3d, 0x8a, 0x55),
    },
    Palette {
        name: "nord",
        dark: true,
        bg: rgb(0x24, 0x29, 0x33),
        panel: rgb(0x2e, 0x34, 0x40),
        panel_hi: rgb(0x3b, 0x42, 0x52),
        text: rgb(0xd8, 0xde, 0xe9),
        sub: rgb(0x61, 0x6e, 0x88),
        accent: rgb(0x88, 0xc0, 0xd0),
        error: rgb(0xbf, 0x61, 0x6a),
        error_dim: rgb(0x7a, 0x45, 0x4c),
        good: rgb(0xa3, 0xbe, 0x8c),
    },
    Palette {
        name: "matcha",
        dark: true,
        bg: rgb(0x52, 0x3e, 0x2b),
        panel: rgb(0x5c, 0x47, 0x33),
        panel_hi: rgb(0x6a, 0x54, 0x3d),
        text: rgb(0xec, 0xdd, 0xc6),
        sub: rgb(0x9a, 0x8a, 0x6e),
        accent: rgb(0x7b, 0xb3, 0x6b),
        error: rgb(0xe8, 0x76, 0x5a),
        error_dim: rgb(0x96, 0x55, 0x40),
        good: rgb(0x9d, 0xc8, 0x84),
    },
];

pub fn palette(name: &str) -> Palette {
    THEMES
        .iter()
        .copied()
        .find(|p| p.name == name)
        .unwrap_or(THEMES[0])
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.panel;
    v.faint_bg_color = p.panel;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.panel_hi);
    v.window_corner_radius = CornerRadius::same(10);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, p.bg),
        (&mut v.widgets.inactive, p.panel_hi),
        (&mut v.widgets.hovered, p.panel_hi),
        (&mut v.widgets.active, p.panel_hi),
        (&mut v.widgets.open, p.panel_hi),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(8);
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0, p.text);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.sub);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.accent);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(26.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(15.0, FontFamily::Monospace),
            ),
        ]
        .into();
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.interact_size.y = 30.0;
    });
}
