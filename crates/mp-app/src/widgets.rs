//! Custom widgets: vector icons, knobs, meters and faders.

use crate::theme::*;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Play,
    Pause,
    ToStart,
    Loop,
    Metronome,
    Magnet,
    Plus,
    Close,
    Search,
    Power,
    Reverse,
}

pub fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.5, color);
    let line = |a: Vec2, b: Vec2| painter.line_segment([c + a * r, c + b * r], stroke);
    let v = egui::vec2;
    match icon {
        Icon::Play => {
            let pts = vec![c + v(-0.55, -0.75) * r, c + v(0.8, 0.0) * r, c + v(-0.55, 0.75) * r];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Pause => {
            for x in [-0.55, 0.2] {
                painter.rect_filled(
                    Rect::from_min_size(c + v(x, -0.7) * r, v(0.38, 1.4) * r),
                    CornerRadius::same(1),
                    color,
                );
            }
        }
        Icon::ToStart => {
            painter.rect_filled(Rect::from_min_size(c + v(-0.75, -0.7) * r, v(0.25, 1.4) * r), CornerRadius::same(1), color);
            let pts = vec![c + v(0.75, -0.7) * r, c + v(0.75, 0.7) * r, c + v(-0.35, 0.0) * r];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Loop => {
            let pts = arc_points(c, r * 0.72, -0.25 * PI, 1.4 * PI, 24);
            painter.add(Shape::line(pts.clone(), stroke));
            let end = pts[pts.len() - 1];
            painter.line_segment([end, end + v(0.45, -0.1) * r], stroke);
            painter.line_segment([end, end + v(0.1, -0.45) * r], stroke);
        }
        Icon::Metronome => {
            let pts = vec![c + v(-0.6, 0.8) * r, c + v(-0.25, -0.8) * r, c + v(0.25, -0.8) * r, c + v(0.6, 0.8) * r, c + v(-0.6, 0.8) * r];
            painter.add(Shape::line(pts, stroke));
            line(v(0.0, 0.35), v(0.55, -0.55));
        }
        Icon::Magnet => {
            let pts = arc_points(c + v(0.0, -0.05) * r, r * 0.6, 0.0, -PI, 16);
            let mut all = vec![c + v(0.6, 0.75) * r];
            all.extend(pts);
            all.push(c + v(-0.6, 0.75) * r);
            painter.add(Shape::line(all, Stroke::new(2.2, color)));
        }
        Icon::Plus => {
            line(v(-0.7, 0.0), v(0.7, 0.0));
            line(v(0.0, -0.7), v(0.0, 0.7));
        }
        Icon::Close => {
            line(v(-0.55, -0.55), v(0.55, 0.55));
            line(v(-0.55, 0.55), v(0.55, -0.55));
        }
        Icon::Search => {
            painter.circle_stroke(c + v(-0.15, -0.15) * r, r * 0.52, stroke);
            line(v(0.25, 0.25), v(0.8, 0.8));
        }
        Icon::Power => {
            painter.add(Shape::line(arc_points(c, r * 0.65, -0.3 * PI, 1.3 * PI, 20), stroke));
            line(v(0.0, -0.9), v(0.0, -0.2));
        }
        Icon::Reverse => {
            line(v(0.8, 0.0), v(-0.8, 0.0));
            line(v(-0.8, 0.0), v(-0.3, -0.5));
            line(v(-0.8, 0.0), v(-0.3, 0.5));
        }
    }
}

fn arc_points(c: Pos2, r: f32, from: f32, to: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// A square icon toggle. `active` lights it in `tint`.
pub fn icon_toggle(ui: &mut Ui, icon: Icon, active: bool, tint: Color32, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 26.0), Sense::click());
    let hovered = response.hovered();
    let fill = if active {
        alpha(tint, 40)
    } else if hovered {
        BG_4
    } else {
        BG_3
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    let color = if active { tint } else if hovered { TEXT } else { TEXT_DIM };
    draw_icon(ui.painter(), rect.shrink(7.0), icon, color);
    response.on_hover_text(tooltip)
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    icon_toggle(ui, icon, false, ACCENT, tooltip)
}

/// A small icon with no background, for dense places such as device headers.
pub fn tiny_icon(ui: &mut Ui, icon: Icon, color: Color32, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let c = if response.hovered() { TEXT } else { color };
    draw_icon(ui.painter(), rect.shrink(3.0), icon, c);
    response.on_hover_text(tooltip)
}

/// A letter toggle such as M or S on a track.
pub fn letter_toggle(ui: &mut Ui, letter: &str, active: bool, tint: Color32, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
    let fill = if active { tint } else if response.hovered() { BG_4 } else { BG_3 };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        ui_font(10.5),
        if active { BG_0 } else { TEXT_DIM },
    );
    response.on_hover_text(tooltip)
}

/// A primary action button.
pub fn accent_button(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), ui_font(12.5), BG_0);
    let size = galley.size() + egui::vec2(20.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() { scale(ACCENT, 1.12) } else { ACCENT };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, BG_0);
    response
}

/// A segmented tab. Returns true when clicked.
pub fn tab(ui: &mut Ui, text: &str, selected: bool) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_string(), ui_font(12.0), TEXT);
    let size = galley.size() + egui::vec2(16.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let color = if selected || response.hovered() { TEXT } else { TEXT_DIM };
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_max(egui::pos2(rect.left() + 4.0, rect.bottom() - 2.0), egui::pos2(rect.right() - 4.0, rect.bottom())),
            CornerRadius::same(1),
            ACCENT,
        );
    }
    let galley = ui.painter().layout_no_wrap(text.to_string(), ui_font(12.0), color);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    response.clicked()
}

pub struct KnobOutput {
    pub response: Response,
    pub changed: bool,
    pub started: bool,
    pub finished: bool,
}

/// A rotary knob over a normalized value. Drag vertically, hold Shift for fine control,
/// double-click to reset. `center` draws the arc from the middle, for bipolar values.
pub fn knob(ui: &mut Ui, value: &mut f32, default: f32, label: &str, display: &str, center: bool) -> KnobOutput {
    const W: f32 = 58.0;
    const H: f32 = 70.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(W, H), Sense::click_and_drag());
    let mut changed = false;
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let delta = response.drag_delta().y * if fine { -0.001 } else { -0.005 };
        if delta != 0.0 {
            *value = (*value + delta).clamp(0.0, 1.0);
            changed = true;
        }
    }
    if response.double_clicked() {
        *value = default;
        changed = true;
    }
    let painter = ui.painter();
    let c = rect.center_top() + egui::vec2(0.0, 25.0);
    let r = 17.0;
    let (from, sweep) = (0.75 * PI, 1.5 * PI);
    painter.add(Shape::line(arc_points(c, r, from, from + sweep, 40), Stroke::new(3.0, BG_0)));
    let to = from + sweep * *value;
    let start = if center { from + sweep * 0.5 } else { from };
    if (to - start).abs() > 0.01 {
        painter.add(Shape::line(arc_points(c, r, start, to, 32), Stroke::new(3.0, ACCENT)));
    }
    let active = response.hovered() || response.dragged();
    painter.circle_filled(c, r - 5.0, if active { BG_4 } else { BG_3 });
    let dir = egui::vec2(to.cos(), to.sin());
    painter.line_segment([c + dir * (r - 11.0), c + dir * (r - 5.5)], Stroke::new(2.0, TEXT));
    let show_value = response.hovered() || response.dragged();
    painter.text(
        rect.center_top() + egui::vec2(0.0, 55.0),
        egui::Align2::CENTER_CENTER,
        if show_value { display } else { label },
        ui_font(10.5),
        if show_value { TEXT } else { TEXT_DIM },
    );
    KnobOutput {
        started: response.drag_started() || response.double_clicked(),
        finished: response.drag_stopped() || response.double_clicked(),
        changed,
        response,
    }
}

/// Maps dB to meter height: -60 dB at the bottom, +6 dB at the top.
pub fn meter_pos(db: f32) -> f32 {
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

/// A stereo level meter, vertical when taller than wide.
pub fn meter(painter: &egui::Painter, rect: Rect, peaks: [f32; 2]) {
    painter.rect_filled(rect, CornerRadius::same(2), BG_0);
    let vertical = rect.height() > rect.width();
    for (i, peak) in peaks.iter().enumerate() {
        let db = mp_dsp::gain_to_db(*peak);
        let level = meter_pos(db);
        let color = if db > 0.0 {
            BAD
        } else if db > -6.0 {
            WARN
        } else {
            GOOD
        };
        let bar = if vertical {
            let w = (rect.width() - 1.0) * 0.5;
            let x = rect.left() + i as f32 * (w + 1.0);
            Rect::from_min_max(egui::pos2(x, rect.bottom() - rect.height() * level), egui::pos2(x + w, rect.bottom()))
        } else {
            let h = (rect.height() - 1.0) * 0.5;
            let y = rect.top() + i as f32 * (h + 1.0);
            Rect::from_min_max(egui::pos2(rect.left(), y), egui::pos2(rect.left() + rect.width() * level, y + h))
        };
        if level > 0.0 {
            painter.rect_filled(bar, CornerRadius::ZERO, color);
        }
    }
}

/// Fader position for a gain, where 0 dB sits at 75% of the travel.
pub fn fader_norm(db: f32) -> f32 {
    if db >= 0.0 {
        0.75 + (db / 12.0).min(1.0) * 0.25
    } else {
        0.75 * ((db + 60.0) / 60.0).max(0.0).powf(1.6)
    }
}

pub fn fader_db(norm: f32) -> f32 {
    if norm >= 0.75 {
        (norm - 0.75) / 0.25 * 12.0
    } else {
        (norm / 0.75).powf(1.0 / 1.6) * 60.0 - 60.0
    }
}

pub struct FaderOutput {
    pub response: Response,
    pub started: bool,
    pub finished: bool,
    pub changed: bool,
}

/// A slim gain fader. Double-click to reset to 0 dB.
pub fn fader(ui: &mut Ui, db: &mut f32, size: Vec2) -> FaderOutput {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let vertical = size.y > size.x;
    let mut changed = false;
    if response.dragged() {
        if let Some(p) = response.interact_pointer_pos() {
            let norm = if vertical {
                1.0 - (p.y - rect.top()) / rect.height()
            } else {
                (p.x - rect.left()) / rect.width()
            };
            *db = fader_db(norm.clamp(0.0, 1.0));
            changed = true;
        }
    }
    if response.double_clicked() {
        *db = 0.0;
        changed = true;
    }
    let norm = fader_norm(*db);
    let painter = ui.painter();
    if vertical {
        let track = Rect::from_center_size(rect.center(), egui::vec2(3.0, rect.height()));
        painter.rect_filled(track, CornerRadius::same(1), BG_0);
        let zero = rect.bottom() - rect.height() * 0.75;
        painter.line_segment([egui::pos2(rect.left(), zero), egui::pos2(rect.right(), zero)], Stroke::new(1.0, BG_4));
        let y = rect.bottom() - rect.height() * norm;
        let cap = Rect::from_center_size(egui::pos2(rect.center().x, y), egui::vec2(rect.width(), 12.0));
        painter.rect_filled(cap, CornerRadius::same(3), if response.hovered() || response.dragged() { TEXT } else { TEXT_DIM });
    } else {
        let track = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 3.0));
        painter.rect_filled(track, CornerRadius::same(1), BG_0);
        let filled = Rect::from_min_size(track.min, egui::vec2(rect.width() * norm, 3.0));
        painter.rect_filled(filled, CornerRadius::same(1), BG_4);
        let x = rect.left() + rect.width() * norm;
        let cap = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(6.0, rect.height()));
        painter.rect_filled(cap, CornerRadius::same(2), if response.hovered() || response.dragged() { TEXT } else { TEXT_DIM });
    }
    FaderOutput {
        started: response.drag_started() || response.double_clicked(),
        finished: response.drag_stopped() || response.double_clicked(),
        changed,
        response,
    }
}

/// Writes key names for this platform. Labels are authored with Mac symbols, and elsewhere those
/// become words, because the Command shortcuts are on Ctrl there.
pub fn keys(text: &str) -> String {
    if cfg!(target_os = "macos") {
        text.to_string()
    } else {
        text.replace("⌘ ", "Ctrl ")
            .replace("⌘-", "Ctrl-")
            .replace('⌘', "Ctrl+")
            .replace('⇧', "Shift+")
            .replace('⌫', "Backspace")
    }
}

/// A keycap label such as "Space" or "⌘K".
pub fn keycap(ui: &mut Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), mono(10.5), TEXT_DIM);
    let (rect, _) = ui.allocate_exact_size(galley.size() + egui::vec2(10.0, 5.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), BG_3);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, TEXT_DIM);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn key_labels_use_words_off_mac() {
        assert_eq!(keys("⇧⌘Z"), "Shift+Ctrl+Z");
        assert_eq!(keys("Hold ⌘ to bypass"), "Hold Ctrl to bypass");
        assert_eq!(keys("⌘-scroll zooms"), "Ctrl-scroll zooms");
    }

    #[test]
    fn fader_mapping_roundtrips() {
        for db in [-48.0, -24.0, -6.0, -0.5, 0.0, 3.0, 12.0] {
            let back = fader_db(fader_norm(db));
            assert!((back - db).abs() < 0.01, "{db} -> {back}");
        }
        assert_eq!(fader_norm(0.0), 0.75);
    }
}
