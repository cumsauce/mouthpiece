//! Colours, fonts and the egui style.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

pub const BG_0: Color32 = Color32::from_rgb(16, 17, 19);
pub const BG_1: Color32 = Color32::from_rgb(23, 24, 27);
pub const BG_2: Color32 = Color32::from_rgb(31, 32, 36);
pub const BG_3: Color32 = Color32::from_rgb(42, 44, 49);
pub const BG_4: Color32 = Color32::from_rgb(56, 58, 64);
pub const LINE: Color32 = Color32::from_rgb(9, 9, 11);
pub const TEXT: Color32 = Color32::from_rgb(232, 233, 236);
pub const TEXT_DIM: Color32 = Color32::from_rgb(158, 161, 168);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(106, 109, 117);
pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 69);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(92, 54, 38);
pub const GOOD: Color32 = Color32::from_rgb(92, 205, 140);
pub const WARN: Color32 = Color32::from_rgb(240, 186, 82);
pub const BAD: Color32 = Color32::from_rgb(235, 88, 88);
pub const SOLO: Color32 = Color32::from_rgb(240, 200, 70);
pub const MUTE: Color32 = Color32::from_rgb(90, 150, 240);

const TRACK_COLORS: [Color32; 8] = [
    Color32::from_rgb(255, 122, 89),
    Color32::from_rgb(64, 196, 180),
    Color32::from_rgb(154, 128, 255),
    Color32::from_rgb(86, 160, 255),
    Color32::from_rgb(240, 186, 82),
    Color32::from_rgb(232, 110, 170),
    Color32::from_rgb(130, 200, 100),
    Color32::from_rgb(110, 190, 230),
];

pub fn track_color(index: u8) -> Color32 {
    TRACK_COLORS[index as usize % TRACK_COLORS.len()]
}

pub fn scale(c: Color32, factor: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * factor).min(255.0) as u8,
        (c.g() as f32 * factor).min(255.0) as u8,
        (c.b() as f32 * factor).min(255.0) as u8,
    )
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn ui_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Uses the system UI font where available, keeping the bundled fonts as fallback for symbols.
fn load_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut prepend = |name: &str, path: &str, family: FontFamily| {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(data)));
            fonts.families.entry(family).or_default().insert(0, name.to_string());
        }
    };
    prepend("ui", "/System/Library/Fonts/HelveticaNeue.ttc", FontFamily::Proportional);
    prepend("mono", "/System/Library/Fonts/Menlo.ttc", FontFamily::Monospace);
    fonts
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(load_fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(10.5)),
            (TextStyle::Body, FontId::proportional(12.5)),
            (TextStyle::Button, FontId::proportional(12.0)),
            (TextStyle::Heading, FontId::proportional(16.0)),
            (TextStyle::Monospace, FontId::monospace(11.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size = egui::vec2(32.0, 22.0);
        style.spacing.slider_width = 120.0;
        style.spacing.combo_width = 80.0;
        style.spacing.scroll = egui::style::ScrollStyle::thin();
        style.interaction.tooltip_delay = 0.35;
        style.interaction.selectable_labels = false;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG_1;
        v.window_fill = BG_2;
        v.window_stroke = Stroke::new(1.0, BG_4);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = BG_0;
        v.faint_bg_color = BG_2;
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.text_cursor.stroke = Stroke::new(1.5, ACCENT);
        let radius = CornerRadius::same(5);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = BG_2;
        w.noninteractive.weak_bg_fill = BG_2;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        w.noninteractive.corner_radius = radius;
        w.inactive.bg_fill = BG_3;
        w.inactive.weak_bg_fill = BG_3;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = radius;
        w.hovered.bg_fill = BG_4;
        w.hovered.weak_bg_fill = BG_4;
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.corner_radius = radius;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = ACCENT_SOFT;
        w.active.weak_bg_fill = ACCENT_SOFT;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0, TEXT);
        w.active.corner_radius = radius;
        w.active.expansion = 0.0;
        w.open = w.hovered;
    });
}
