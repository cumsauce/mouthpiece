//! Controls specific to audio: knobs, faders and meters.

use super::icons::arc_points;
use super::tokens::*;
use eframe::egui::{self, Color32, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::f32::consts::PI;

pub struct KnobOutput {
    pub response: Response,
    pub changed: bool,
    pub started: bool,
    pub finished: bool,
}

/// A rotary knob over a value in 0..=1. Drag vertically, hold Shift to fine-tune, double-click to
/// reset. `center` draws the arc from the middle, for bipolar values. `tint` is the owning track's colour.
pub fn knob(ui: &mut Ui, value: &mut f32, default: f32, label: &str, display: &str, center: bool, tint: Color32) -> KnobOutput {
    const W: f32 = 56.0;
    const H: f32 = 66.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(W, H), Sense::click_and_drag());
    let mut changed = false;
    if response.dragged_by(egui::PointerButton::Primary) {
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
    let active = response.hovered() || response.dragged_by(egui::PointerButton::Primary);
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let painter = ui.painter();
    let c = rect.center_top() + egui::vec2(0.0, 23.0);
    let r = 16.0;
    let (from, sweep) = (0.75 * PI, 1.5 * PI);
    painter.add(Shape::line(arc_points(c, r, from, from + sweep, 40), Stroke::new(2.5, CONTROL)));
    let to = from + sweep * *value;
    let start = if center { from + sweep * 0.5 } else { from };
    if (to - start).abs() > 0.01 {
        painter.add(Shape::line(arc_points(c, r, start, to, 32), Stroke::new(2.5, tint)));
    }
    painter.circle_filled(c, r - 5.0, if active { CONTROL_HOVER } else { CONTROL });
    let dir = egui::vec2(to.cos(), to.sin());
    painter.line_segment([c + dir * (r - 11.0), c + dir * (r - 5.5)], Stroke::new(2.0, TEXT));
    painter.text(
        rect.center_top() + egui::vec2(0.0, 52.0),
        egui::Align2::CENTER_CENTER,
        if active { display } else { label },
        font_caption(),
        if active { TEXT } else { TEXT_3 },
    );
    KnobOutput {
        started: response.drag_started_by(egui::PointerButton::Primary) || response.double_clicked(),
        finished: response.drag_stopped_by(egui::PointerButton::Primary) || response.double_clicked(),
        changed,
        response,
    }
}

/// Maps dB to meter height: -60 dB at the bottom, +6 dB at the top.
pub fn meter_pos(db: f32) -> f32 {
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

/// A stereo level meter, vertical when taller than wide. Green is healthy, amber is hot, red clips.
pub fn meter(painter: &egui::Painter, rect: Rect, peaks: [f32; 2]) {
    painter.rect_filled(rect, egui::CornerRadius::same(1), BASE);
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
            painter.rect_filled(bar, egui::CornerRadius::ZERO, color);
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

/// A slim gain fader, vertical when taller than wide. Double-click resets it to 0 dB.
pub fn fader(ui: &mut Ui, db: &mut f32, size: Vec2) -> FaderOutput {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let vertical = size.y > size.x;
    let mut changed = false;
    if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(p) = response.interact_pointer_pos() {
            let norm = if vertical { 1.0 - (p.y - rect.top()) / rect.height() } else { (p.x - rect.left()) / rect.width() };
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
    let active = response.hovered() || response.dragged_by(egui::PointerButton::Primary);
    let cap_color = if active { TEXT } else { TEXT_2 };
    if vertical {
        painter.rect_filled(Rect::from_center_size(rect.center(), egui::vec2(2.0, rect.height())), egui::CornerRadius::same(1), BASE);
        let zero = rect.bottom() - rect.height() * 0.75;
        painter.line_segment([egui::pos2(rect.left() + 2.0, zero), egui::pos2(rect.right() - 2.0, zero)], Stroke::new(1.0, CONTROL_HOVER));
        let y = rect.bottom() - rect.height() * norm;
        let cap = Rect::from_center_size(egui::pos2(rect.center().x, y), egui::vec2(rect.width(), 10.0));
        painter.rect_filled(cap, egui::CornerRadius::same(2), cap_color);
    } else {
        let track = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 2.0));
        painter.rect_filled(track, egui::CornerRadius::same(1), BASE);
        let filled = Rect::from_min_size(track.min, egui::vec2(rect.width() * norm, 2.0));
        painter.rect_filled(filled, egui::CornerRadius::same(1), CONTROL_HOVER);
        let x = rect.left() + rect.width() * norm;
        painter.circle_filled(egui::pos2(x, rect.center().y), if active { 5.0 } else { 4.0 }, cap_color);
    }
    FaderOutput {
        started: response.drag_started_by(egui::PointerButton::Primary) || response.double_clicked(),
        finished: response.drag_stopped_by(egui::PointerButton::Primary) || response.double_clicked(),
        changed,
        response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_mapping_roundtrips() {
        for db in [-48.0, -24.0, -6.0, -0.5, 0.0, 3.0, 12.0] {
            let back = fader_db(fader_norm(db));
            assert!((back - db).abs() < 0.01, "{db} -> {back}");
        }
        assert_eq!(fader_norm(0.0), 0.75);
    }
}
