//! Design tokens: every colour, size and font in the app is defined here and nowhere else.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

// The colours are chosen in OKLCH, where equal numbers look equal, unlike in RGB or HSL. Each comment gives lightness,
// chroma and hue. See "How the palette is built" in DESIGN.md, and the tests at the end of this file.

// ----- Surfaces, from furthest back to nearest. Depth comes from these, not from lines. -----
// All share one cool hue that sits opposite the accent, at a chroma too low to read as a colour.

/// The canvas behind clips and waveforms. oklch(.168 .006 262)
pub const BASE: Color32 = Color32::from_rgb(14, 15, 18);
/// Panels and bars. oklch(.212 .008 262)
pub const PANEL: Color32 = Color32::from_rgb(23, 25, 29);
/// Cards, floating bars and overlays. oklch(.253 .009 262)
pub const RAISED: Color32 = Color32::from_rgb(32, 34, 39);
/// Resting controls. oklch(.300 .011 262)
pub const CONTROL: Color32 = Color32::from_rgb(43, 46, 51);
/// Hovered controls, and the selected segment. oklch(.357 .013 262)
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(57, 60, 67);
/// The gap between panels. oklch(.135 .005 262)
pub const DIVIDER: Color32 = Color32::from_rgb(7, 8, 10);
/// Outlines around floating surfaces. oklch(.332 .012 262)
pub const BORDER: Color32 = Color32::from_rgb(51, 54, 60);

// ----- Text. All three meet WCAG AA (4.5:1) on PANEL and RAISED. -----

pub const TEXT: Color32 = Color32::from_rgb(237, 238, 241);
pub const TEXT_2: Color32 = Color32::from_rgb(171, 176, 184);
pub const TEXT_3: Color32 = Color32::from_rgb(134, 139, 148);
pub const TEXT_ON_ACCENT: Color32 = Color32::from_rgb(24, 12, 6);

// ----- Meaning. See DESIGN.md: accent is scarce on purpose. -----

/// Playhead, selection, and the single primary action in view. Nothing else. oklch(.727 .176 41)
pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 69);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 150, 111);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(88, 50, 36);
// ----- Every other colour has the same vibrancy: one chroma, and one lightness as it appears. -----
//
// Equal OKLCH lightness isn't enough, because a saturated colour looks brighter than a grey of the same lightness, by an
// amount that depends on its hue (the Helmholtz-Kohlrausch effect). So each hue's lightness is solved for, until its
// apparent lightness under Fairchild and Pirrotta's model is APPARENT_LIGHTNESS. Blues come out darker and yellows lighter,
// and side by side none of them looks brighter or more colourful than another. The accent alone breaks the rule, as the
// most colourful thing on screen. See "How the palette is built" in DESIGN.md.

/// The chroma every track and status colour shares: the most that every hue used can show at this lightness. Red sets
/// the limit.
const CHROMA: f32 = 0.120;
/// Their lightness as it appears, in CIELAB L* units, Helmholtz-Kohlrausch effect included.
const APPARENT_LIGHTNESS: f32 = 78.0;

/// oklch(.761 .120 150)
pub const GOOD: Color32 = Color32::from_rgb(118, 199, 136);
/// oklch(.787 .120 78)
pub const WARN: Color32 = Color32::from_rgb(228, 176, 90);
/// oklch(.791 .120 25)
pub const BAD: Color32 = Color32::from_rgb(255, 156, 148);
/// oklch(.787 .120 92)
pub const SOLO: Color32 = Color32::from_rgb(213, 183, 87);
/// oklch(.760 .120 255)
pub const MUTE: Color32 = Color32::from_rgb(124, 180, 252);

/// Hues on a decagon, 36° apart from 70°. The tracks that are usually open together, a song's stems and the Sentence
/// track, take alternate corners, a pentad, so that they're 72° apart. A second or third track of a kind takes a corner
/// between. Of the two corners left, one is where the accent is, so no track is mistaken for the selection, and the
/// other is an olive that looks muddy at this lightness. The pentad's rotation is the one that keeps it furthest apart
/// under simulated deuteranopia and protanopia.
const TRACK_COLORS: [Color32; 8] = [
    Color32::from_rgb(248, 152, 185), // Vocal: rose, oklch(.788 .120 358)
    Color32::from_rgb(235, 172, 94),  // Drums: amber, oklch(.788 .120 70)
    Color32::from_rgb(171, 166, 251), // Bass: violet, oklch(.762 .120 286)
    Color32::from_rgb(133, 198, 126), // Music: green, oklch(.764 .120 142)
    Color32::from_rgb(67, 200, 176),  // A second music track: teal, oklch(.756 .120 178)
    Color32::from_rgb(57, 196, 223),  // A second vocal, usually Sentence: sky, oklch(.759 .120 214)
    Color32::from_rgb(115, 183, 251), // A second drum track: blue, oklch(.761 .120 250)
    Color32::from_rgb(217, 155, 227), // A third music track: orchid, oklch(.774 .120 322)
];

/// The tracks a song usually has open together: vocal, drums, bass, music, and Sentence.
#[cfg(test)]
const TRACKS_OPEN_TOGETHER: [usize; 5] = [0, 1, 2, 3, 5];

pub fn track_color(index: u8) -> Color32 {
    TRACK_COLORS[index as usize % TRACK_COLORS.len()]
}

/// The colours phoneme blocks use, by broad sound class. Built like the tracks, but dark and quiet: chroma 0.05 and an
/// apparent lightness of 37.5, so white text reads on each and none stands out. Sounds that ring are cool, 36° apart,
/// and sounds that are noise or a stop are warm, 28° apart.
pub const PHONEME_VOWEL: Color32 = Color32::from_rgb(62, 73, 101); // oklch(.408 .05 268)
pub const PHONEME_GLIDE: Color32 = Color32::from_rgb(45, 80, 98); // oklch(.413 .05 232)
pub const PHONEME_NASAL: Color32 = Color32::from_rgb(38, 84, 84); // oklch(.413 .05 196)
pub const PHONEME_VOICED_FRICATIVE: Color32 = Color32::from_rgb(98, 77, 50); // oklch(.435 .05 72)
pub const PHONEME_FRICATIVE: Color32 = Color32::from_rgb(104, 72, 58); // oklch(.433 .05 44)
pub const PHONEME_VOICED_STOP: Color32 = Color32::from_rgb(88, 67, 93); // oklch(.415 .05 320)
pub const PHONEME_STOP: Color32 = Color32::from_rgb(99, 67, 83); // oklch(.424 .05 348)
pub const PHONEME_AFFRICATE: Color32 = Color32::from_rgb(105, 69, 71); // oklch(.431 .05 16)

/// The background the spectrogram fades into, which is its map at zero.
pub const SPECTROGRAM_FLOOR: Color32 = Color32::from_rgb(14, 13, 28);

/// The spectrogram's colour map. `t` is 0..=1. Lightness rises evenly with level, as in magma and inferno, so that
/// loudness reads truly and survives colour blindness. The hue passes through the accent's on its way to pale gold.
pub fn spectrogram_color(t: f32) -> Color32 {
    static MAP: std::sync::LazyLock<Vec<Color32>> = std::sync::LazyLock::new(|| {
        // Position, chroma and hue.
        const STOPS: [(f32, f32, f32); 5] = [(0.0, 0.03, 285.0), (0.35, 0.12, 320.0), (0.6, 0.17, 375.0), (0.8, 0.16, 415.0), (1.0, 0.07, 455.0)];
        (0..256)
            .map(|i| {
                let t = i as f32 / 255.0;
                let pair = STOPS.windows(2).find(|p| t <= p[1].0).unwrap_or(&STOPS[3..]);
                let u = (t - pair[0].0) / (pair[1].0 - pair[0].0);
                oklch(0.17 + 0.79 * t, pair[0].1 + (pair[1].1 - pair[0].1) * u, pair[0].2 + (pair[1].2 - pair[0].2) * u)
            })
            .collect()
    });
    MAP[(t.clamp(0.0, 1.0) * 255.0) as usize]
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

fn to_oklab(c: Color32) -> [f32; 3] {
    let (r, g, b) = (to_linear(c.r()), to_linear(c.g()), to_linear(c.b()));
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_05 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_04 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

/// Linear RGB, which may be out of range.
fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let x = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let y = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let z = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [4.076_741_7 * x - 3.307_711_6 * y + 0.230_969_94 * z, -1.268_438 * x + 2.609_757_4 * y - 0.341_319_38 * z, -0.004_196_09 * x - 0.703_418_6 * y + 1.707_614_7 * z]
}

fn from_linear([r, g, b]: [f32; 3]) -> Color32 {
    Color32::from_rgb(to_srgb(r), to_srgb(g), to_srgb(b))
}

/// A colour from lightness 0..=1, chroma and hue in degrees. A colour the screen can't show loses chroma until it
/// can, which keeps its lightness and hue, where simply clipping it would change both.
pub fn oklch(l: f32, mut c: f32, h: f32) -> Color32 {
    let (sin, cos) = h.to_radians().sin_cos();
    loop {
        let rgb = from_oklab([l, c * cos, c * sin]);
        if c <= 0.0 || rgb.iter().all(|v| (-0.000_1..=1.000_1).contains(v)) {
            return from_linear(rgb);
        }
        c -= 0.002;
    }
}

/// Blends two colours in OKLab, so that midway between them looks midway, and tints don't turn muddy as they do in sRGB.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let (a, b) = (to_oklab(a), to_oklab(b));
    from_linear(from_oklab(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)))
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

#[cfg(test)]
mod palette_tests {
    use super::*;

    fn wcag_contrast(a: Color32, b: Color32) -> f32 {
        let luminance = |c: Color32| 0.2126 * to_linear(c.r()) + 0.7152 * to_linear(c.g()) + 0.0722 * to_linear(c.b());
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn distance(a: Color32, b: Color32) -> f32 {
        let (a, b) = (to_oklab(a), to_oklab(b));
        (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
    }

    /// What someone with deuteranopia or protanopia sees (Machado, Oliveira and Fernandes, 2009).
    fn as_seen_with(matrix: [[f32; 3]; 3], c: Color32) -> Color32 {
        let rgb = [to_linear(c.r()), to_linear(c.g()), to_linear(c.b())];
        from_linear(std::array::from_fn(|i| (0..3).map(|j| matrix[i][j] * rgb[j]).sum()))
    }

    #[test]
    fn text_is_readable_on_every_surface_it_sits_on() {
        for surface in [PANEL, RAISED] {
            for text in [TEXT, TEXT_2, TEXT_3] {
                assert!(wcag_contrast(text, surface) >= 4.5, "{text:?} on {surface:?}");
            }
        }
        assert!(wcag_contrast(TEXT_ON_ACCENT, ACCENT) >= 4.5);
    }

    #[test]
    fn surfaces_get_evenly_lighter_towards_the_viewer() {
        let lightness: Vec<f32> = [DIVIDER, BASE, PANEL, RAISED, CONTROL, BORDER, CONTROL_HOVER].iter().map(|c| to_oklab(*c)[0]).collect();
        for pair in lightness.windows(2) {
            assert!((0.02..0.06).contains(&(pair[1] - pair[0])), "{lightness:?}");
        }
    }

    /// CIELAB lightness, chroma and hue in degrees, for a D65 white.
    fn cielab_lch(c: Color32) -> [f32; 3] {
        let (r, g, b) = (to_linear(c.r()), to_linear(c.g()), to_linear(c.b()));
        let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
        let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
        let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
        let f = |t: f32| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
        let (a, b) = (500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)));
        [116.0 * f(y) - 16.0, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0)]
    }

    /// Lightness as it appears, counting the Helmholtz-Kohlrausch effect (Fairchild and Pirrotta, 1991).
    fn apparent_lightness(c: Color32) -> f32 {
        let [l, chroma, hue] = cielab_lch(c);
        l + (2.5 - 0.025 * l) * (0.116 * ((hue - 90.0) / 2.0).to_radians().sin().abs() + 0.085) * chroma
    }

    fn oklch_chroma(c: Color32) -> f32 {
        let [_, a, b] = to_oklab(c);
        a.hypot(b)
    }

    #[test]
    fn tracks_and_status_colours_are_equally_vibrant() {
        for c in TRACK_COLORS.iter().chain(&[GOOD, WARN, BAD, SOLO, MUTE]) {
            assert!((oklch_chroma(*c) - CHROMA).abs() < 0.004, "{c:?} has chroma {}", oklch_chroma(*c));
            assert!((apparent_lightness(*c) - APPARENT_LIGHTNESS).abs() < 0.6, "{c:?} looks {} light", apparent_lightness(*c));
        }
        // The accent is the one colour allowed to stand out.
        assert!(oklch_chroma(ACCENT) > CHROMA * 1.3);
        let phonemes = [PHONEME_VOWEL, PHONEME_GLIDE, PHONEME_NASAL, PHONEME_VOICED_FRICATIVE, PHONEME_FRICATIVE, PHONEME_VOICED_STOP, PHONEME_STOP, PHONEME_AFFRICATE];
        for c in phonemes {
            assert!((oklch_chroma(c) - 0.05).abs() < 0.004 && (apparent_lightness(c) - 37.5).abs() < 0.6, "{c:?}");
        }
    }

    #[test]
    fn tracks_stay_apart_and_away_from_the_accent_even_with_colour_blindness() {
        const DEUTERANOPIA: [[f32; 3]; 3] = [[0.367_322, 0.860_646, -0.227_968], [0.280_085, 0.672_501, 0.047_413], [-0.011_82, 0.042_94, 0.968_881]];
        const PROTANOPIA: [[f32; 3]; 3] = [[0.152_286, 1.052_583, -0.204_868], [0.114_503, 0.786_281, 0.099_216], [-0.003_882, -0.048_116, 1.051_998]];
        for (i, &a) in TRACK_COLORS.iter().enumerate() {
            assert!(distance(a, ACCENT) > 0.1, "{a:?}");
            assert!(wcag_contrast(a, PANEL) > 4.5, "{a:?}");
            for &b in &TRACK_COLORS[i + 1..] {
                assert!(distance(a, b) > 0.07, "{a:?} {b:?}");
            }
        }
        // At one lightness, hue is all that tells colours apart, so only so many can be far apart. The tracks open
        // together get the room, and names tell apart the rest.
        for (n, &i) in TRACKS_OPEN_TOGETHER.iter().enumerate() {
            for &j in &TRACKS_OPEN_TOGETHER[n + 1..] {
                let (a, b) = (TRACK_COLORS[i], TRACK_COLORS[j]);
                assert!(distance(a, b) > 0.135, "{a:?} {b:?}");
                for matrix in [DEUTERANOPIA, PROTANOPIA] {
                    assert!(distance(as_seen_with(matrix, a), as_seen_with(matrix, b)) > 0.04, "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn the_spectrogram_only_ever_gets_lighter_with_level() {
        let lightness: Vec<f32> = (0..=255).map(|i| to_oklab(spectrogram_color(i as f32 / 255.0))[0]).collect();
        for pair in lightness.windows(2) {
            assert!(pair[1] >= pair[0] - 0.002, "{lightness:?}");
        }
        assert_eq!(spectrogram_color(0.0), SPECTROGRAM_FLOOR);
        assert!(lightness[255] > 0.9);
    }

    #[test]
    fn oklch_and_mix_agree_with_known_colours() {
        let accent = to_oklab(ACCENT);
        assert!((accent[0] - 0.727).abs() < 0.005, "{accent:?}");
        assert!(distance(oklch(0.727, 0.176, 41.2), ACCENT) < 0.01);
        assert_eq!(mix(ACCENT, TEXT, 0.0), ACCENT);
        assert_eq!(mix(ACCENT, TEXT, 1.0), TEXT);
    }
}
