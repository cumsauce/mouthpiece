//! Design tokens: every colour, size and font in the app is defined here and nowhere else.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

// ----- Surfaces, from furthest back to nearest. Depth comes from these, not from lines. -----

/// The canvas behind clips and waveforms.
pub const BASE: Color32 = Color32::from_rgb(14, 15, 17);
/// Panels and bars.
pub const PANEL: Color32 = Color32::from_rgb(24, 25, 28);
/// Cards, floating bars and overlays.
pub const RAISED: Color32 = Color32::from_rgb(33, 34, 38);
/// Resting controls.
pub const CONTROL: Color32 = Color32::from_rgb(44, 46, 51);
/// Hovered controls, and the selected segment.
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(58, 60, 67);
/// The gap between panels.
pub const DIVIDER: Color32 = Color32::from_rgb(8, 8, 10);
/// Outlines around floating surfaces.
pub const BORDER: Color32 = Color32::from_rgb(52, 54, 60);

// ----- Text. All three meet WCAG AA (4.5:1) on PANEL. -----

pub const TEXT: Color32 = Color32::from_rgb(237, 238, 240);
pub const TEXT_2: Color32 = Color32::from_rgb(168, 171, 178);
pub const TEXT_3: Color32 = Color32::from_rgb(130, 133, 141);
pub const TEXT_ON_ACCENT: Color32 = Color32::from_rgb(24, 12, 6);

// ----- Meaning. See DESIGN.md: accent is scarce on purpose. -----

/// Playhead, selection, and the single primary action in view. Nothing else.
pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 69);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 143, 97);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(86, 50, 35);
pub const GOOD: Color32 = Color32::from_rgb(92, 205, 140);
pub const WARN: Color32 = Color32::from_rgb(240, 186, 82);
pub const BAD: Color32 = Color32::from_rgb(238, 96, 96);
pub const SOLO: Color32 = Color32::from_rgb(240, 200, 70);
pub const MUTE: Color32 = Color32::from_rgb(96, 154, 240);

const TRACK_COLORS: [Color32; 8] = [
    Color32::from_rgb(255, 128, 98),
    Color32::from_rgb(66, 196, 180),
    Color32::from_rgb(156, 132, 255),
    Color32::from_rgb(90, 162, 255),
    Color32::from_rgb(238, 186, 90),
    Color32::from_rgb(232, 114, 172),
    Color32::from_rgb(134, 200, 106),
    Color32::from_rgb(112, 190, 228),
];

pub fn track_color(index: u8) -> Color32 {
    TRACK_COLORS[index as usize % TRACK_COLORS.len()]
}

/// The colours phoneme blocks use, by broad sound class.
pub const PHONEME_VOWEL: Color32 = Color32::from_rgb(66, 74, 110);
pub const PHONEME_GLIDE: Color32 = Color32::from_rgb(58, 88, 100);
pub const PHONEME_NASAL: Color32 = Color32::from_rgb(70, 80, 80);
pub const PHONEME_VOICED_FRICATIVE: Color32 = Color32::from_rgb(96, 76, 64);
pub const PHONEME_FRICATIVE: Color32 = Color32::from_rgb(112, 74, 60);
pub const PHONEME_VOICED_STOP: Color32 = Color32::from_rgb(88, 62, 88);
pub const PHONEME_STOP: Color32 = Color32::from_rgb(102, 60, 84);
pub const PHONEME_AFFRICATE: Color32 = Color32::from_rgb(108, 68, 72);

/// The background the spectrogram fades into.
pub const SPECTROGRAM_FLOOR: Color32 = Color32::from_rgb(20, 18, 34);

/// Dark-to-warm colour map that keeps quiet detail visible. `t` is 0..=1.
pub fn spectrogram_color(t: f32) -> Color32 {
    let t = t * t;
    let (r, g, b) = if t < 0.5 {
        let u = t * 2.0;
        (20.0 + 110.0 * u, 18.0 + 25.0 * u, 34.0 + 70.0 * u)
    } else {
        let u = (t - 0.5) * 2.0;
        (130.0 + 125.0 * u, 43.0 + 180.0 * u, 104.0 + 20.0 * u)
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// ----- Spacing: a 4 px scale. -----

pub const SP_HALF: f32 = 2.0;
pub const SP_1: f32 = 4.0;
pub const SP_2: f32 = 8.0;
pub const SP_3: f32 = 12.0;
pub const SP_4: f32 = 16.0;
pub const SP_6: f32 = 24.0;

// ----- Sizes. -----

/// Standard control height.
pub const CONTROL_H: f32 = 24.0;
/// Dense control height, for track headers and toolbars inside panels.
pub const CONTROL_SM: f32 = 20.0;
pub const TOPBAR_H: f32 = 44.0;
pub const STATUSBAR_H: f32 = 24.0;
/// The label column of a property row.
pub const ROW_LABEL_W: f32 = 60.0;

// ----- Radius. -----

pub const R_S: CornerRadius = CornerRadius::same(4);
pub const R_M: CornerRadius = CornerRadius::same(6);
pub const R_L: CornerRadius = CornerRadius::same(10);

// ----- Motion, in seconds. Fast enough that nothing ever waits for an animation. -----

pub const MOTION_HOVER: f32 = 0.08;
pub const MOTION_FLASH: f32 = 0.7;
pub const TOAST_SECONDS: f32 = 3.5;

// ----- Type. -----

const MEDIUM: &str = "medium";

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

/// Dense annotations, such as badges on clips.
pub fn font_micro() -> FontId {
    FontId::proportional(10.0)
}
/// Secondary information.
pub fn font_caption() -> FontId {
    FontId::proportional(11.0)
}
/// Default text.
pub fn font_body() -> FontId {
    FontId::proportional(12.5)
}
/// The Say input, the main place people type.
pub fn font_input() -> FontId {
    FontId::proportional(14.0)
}
/// Control labels and emphasis at body size.
pub fn font_label() -> FontId {
    medium(12.0)
}
/// Panel and section headings.
pub fn font_heading() -> FontId {
    medium(13.5)
}
/// The name of what's selected.
pub fn font_title() -> FontId {
    medium(16.0)
}
/// Empty states only.
pub fn font_display() -> FontId {
    medium(24.0)
}
/// Numbers that change, so that digits don't shift as they update.
pub fn font_num(size: f32) -> FontId {
    FontId::monospace(size)
}

/// System fonts, for platform-native type. The bundled fonts remain as fallbacks, so that the app
/// looks right wherever these are missing, and every lookup falls through to them.
fn load_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, paths: &[(&str, u32)], family: FontFamily| {
        for (path, index) in paths {
            if let Ok(bytes) = std::fs::read(path) {
                let mut data = FontData::from_owned(bytes);
                data.index = *index;
                fonts.font_data.insert(name.to_string(), Arc::new(data));
                fonts.families.entry(family).or_default().insert(0, name.to_string());
                return;
            }
        }
    };
    const HELVETICA: &str = "/System/Library/Fonts/HelveticaNeue.ttc";
    add(
        "ui",
        &[
            (HELVETICA, 0),
            ("C:\\Windows\\Fonts\\segoeui.ttf", 0),
            ("/usr/share/fonts/truetype/inter/Inter-Regular.ttf", 0),
            ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
        ],
        FontFamily::Proportional,
    );
    add(
        "ui-medium",
        &[
            (HELVETICA, 10),
            ("C:\\Windows\\Fonts\\seguisb.ttf", 0),
            ("/usr/share/fonts/truetype/inter/Inter-Medium.ttf", 0),
            ("/usr/share/fonts/truetype/noto/NotoSans-Medium.ttf", 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
        ],
        FontFamily::Name(MEDIUM.into()),
    );
    add(
        "mono",
        &[("/System/Library/Fonts/Menlo.ttc", 0), ("C:\\Windows\\Fonts\\consola.ttf", 0), ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 0)],
        FontFamily::Monospace,
    );
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Name(MEDIUM.into())).or_default().extend(fallback);
    fonts
}

/// Installs the fonts, and points egui's built-in visuals at the tokens.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(load_fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, font_caption()),
            (TextStyle::Body, font_body()),
            (TextStyle::Button, font_label()),
            (TextStyle::Heading, font_heading()),
            (TextStyle::Monospace, font_num(11.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(SP_1, SP_1);
        style.spacing.button_padding = egui::vec2(SP_2, 3.0);
        style.spacing.interact_size = egui::vec2(CONTROL_H, CONTROL_SM);
        style.spacing.combo_width = 80.0;
        style.spacing.menu_margin = egui::Margin::same(SP_1 as i8);
        style.spacing.scroll = egui::style::ScrollStyle::thin();
        style.interaction.tooltip_delay = 0.35;
        style.interaction.selectable_labels = false;
        style.animation_time = MOTION_HOVER;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = PANEL;
        v.window_fill = RAISED;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = R_L;
        v.menu_corner_radius = R_M;
        v.popup_shadow = shadow();
        v.window_shadow = shadow();
        v.extreme_bg_color = BASE;
        v.faint_bg_color = RAISED;
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.hyperlink_color = ACCENT;
        v.text_cursor.stroke = Stroke::new(1.5, ACCENT);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = RAISED;
        w.noninteractive.weak_bg_fill = RAISED;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_2);
        w.noninteractive.corner_radius = R_S;
        w.inactive.bg_fill = CONTROL;
        w.inactive.weak_bg_fill = CONTROL;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = R_S;
        w.hovered.bg_fill = CONTROL_HOVER;
        w.hovered.weak_bg_fill = CONTROL_HOVER;
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.corner_radius = R_S;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = CONTROL_HOVER;
        w.active.weak_bg_fill = CONTROL_HOVER;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0, TEXT);
        w.active.corner_radius = R_S;
        w.active.expansion = 0.0;
        w.open = w.hovered;
    });
}

pub fn shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: alpha(Color32::BLACK, 130) }
}
