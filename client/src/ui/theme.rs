//! Qwertea's look: tea-named palettes, Atkinson Hyperlegible type, soft
//! rounded surfaces.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub name: &'static str,
    pub label: &'static str,
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub text: Color32,
    pub sub: Color32,
    /// Brand / primary action colour.
    pub accent: Color32,
    /// Secondary highlight: the lesson's training targets.
    pub accent2: Color32,
    pub error: Color32,
    pub error_dim: Color32,
    pub good: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const THEMES: [Palette; 4] = [
    Palette {
        name: "sencha",
        label: "Sencha",
        dark: true,
        bg: rgb(0x14, 0x1c, 0x18),
        panel: rgb(0x1c, 0x26, 0x21),
        panel_hi: rgb(0x29, 0x36, 0x2f),
        text: rgb(0xe9, 0xe4, 0xd4),
        sub: rgb(0x80, 0x91, 0x87),
        accent: rgb(0x8e, 0xd1, 0xa5),
        accent2: rgb(0xe3, 0xb8, 0x7c),
        error: rgb(0xef, 0x7a, 0x6b),
        error_dim: rgb(0x5a, 0x2e, 0x2a),
        good: rgb(0x8e, 0xd1, 0xa5),
    },
    Palette {
        name: "chai",
        label: "Chai",
        dark: false,
        bg: rgb(0xf5, 0xee, 0xe3),
        panel: rgb(0xec, 0xe2, 0xd2),
        panel_hi: rgb(0xdf, 0xd2, 0xbe),
        text: rgb(0x2e, 0x26, 0x20),
        sub: rgb(0x78, 0x6a, 0x5c),
        accent: rgb(0xa9, 0x52, 0x2a),
        accent2: rgb(0x4f, 0x80, 0x55),
        error: rgb(0xc0, 0x39, 0x2b),
        error_dim: rgb(0xf0, 0xc9, 0xbd),
        good: rgb(0x4f, 0x80, 0x55),
    },
    Palette {
        name: "earlgrey",
        label: "Earl Grey",
        dark: true,
        bg: rgb(0x17, 0x1a, 0x22),
        panel: rgb(0x1f, 0x23, 0x30),
        panel_hi: rgb(0x2b, 0x30, 0x40),
        text: rgb(0xe3, 0xe5, 0xee),
        sub: rgb(0x7c, 0x82, 0x97),
        accent: rgb(0xaa, 0xa1, 0xe8),
        accent2: rgb(0xef, 0xc2, 0x7f),
        error: rgb(0xf0, 0x7b, 0x8a),
        error_dim: rgb(0x4d, 0x2a, 0x33),
        good: rgb(0x8f, 0xd3, 0xb0),
    },
    Palette {
        name: "oolong",
        label: "Oolong",
        dark: true,
        bg: rgb(0x1d, 0x16, 0x12),
        panel: rgb(0x27, 0x1e, 0x18),
        panel_hi: rgb(0x35, 0x29, 0x20),
        text: rgb(0xf1, 0xe3, 0xcf),
        sub: rgb(0x9c, 0x86, 0x73),
        accent: rgb(0xe8, 0x95, 0x5a),
        accent2: rgb(0x9c, 0xcf, 0x8f),
        error: rgb(0xff, 0x7b, 0x6b),
        error_dim: rgb(0x5a, 0x2a, 0x22),
        good: rgb(0x9c, 0xcf, 0x8f),
    },
];

/// Palette by name; names from Qwertea 1.0.0 configs map to the new ones.
pub fn palette(name: &str) -> Palette {
    let name = match name {
        "dark" | "matcha" => "sencha",
        "light" => "chai",
        "nord" => "earlgrey",
        n => n,
    };
    THEMES
        .iter()
        .copied()
        .find(|p| p.name == name)
        .unwrap_or(THEMES[0])
}

/// Font family for bold UI text.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "atkinson",
        include_bytes!("../../assets/fonts/AtkinsonHyperlegible-Regular.ttf"),
    );
    add(
        &mut fonts,
        "atkinson-bold",
        include_bytes!("../../assets/fonts/AtkinsonHyperlegible-Bold.ttf"),
    );
    add(
        &mut fonts,
        "atkinson-mono",
        include_bytes!("../../assets/fonts/AtkinsonHyperlegibleMono.ttf"),
    );
    // Atkinson first; egui's bundled fonts stay as fallbacks (arrows, symbols).
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "atkinson".to_owned());
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "atkinson-mono".to_owned());
    let fallback: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    let mut b = vec!["atkinson-bold".to_owned()];
    b.extend(fallback);
    fonts.families.insert(bold(), b);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.panel_hi;
    v.faint_bg_color = p.panel;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.panel_hi);
    v.window_corner_radius = CornerRadius::same(14);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, p.bg),
        (&mut v.widgets.inactive, p.panel_hi),
        (&mut v.widgets.hovered, p.panel_hi),
        (&mut v.widgets.active, p.panel_hi),
        (&mut v.widgets.open, p.panel_hi),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(10);
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.2, p.text);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.sub);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.text);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.accent);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(28.0, bold())),
            (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(15.5, FontFamily::Proportional),
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
        s.spacing.button_padding = egui::vec2(16.0, 8.0);
        s.spacing.item_spacing = egui::vec2(10.0, 9.0);
        s.spacing.interact_size.y = 32.0;
    });
}
