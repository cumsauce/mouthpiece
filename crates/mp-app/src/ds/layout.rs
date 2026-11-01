//! Layout pieces: panels, sections, rows, cards and bars.

use super::controls::Button;
use super::feedback::{body, caption, keycap};
use super::tokens::*;
use eframe::egui::{self, Align2, Rect, Response, Ui};

/// The frame of a side panel.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::NONE.fill(PANEL).inner_margin(egui::Margin::same(SP_3 as i8))
}

/// The frame of a bar along an edge of the window.
pub fn bar_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(SP_2 as i8, 0))
}

/// A floating surface, such as the selection bar and toasts.
pub fn floating_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(RAISED)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(R_M)
        .inner_margin(egui::Margin::symmetric(SP_1 as i8 + 2, SP_1 as i8))
        .shadow(shadow())
}

/// A contained group, such as a device or a source.
pub fn card_frame() -> egui::Frame {
    egui::Frame::NONE.fill(RAISED).corner_radius(R_M).inner_margin(egui::Margin::same(SP_2 as i8))
}

/// The line between a card's title row and its contents.
pub fn card_rule(ui: &mut Ui) {
    let y = ui.cursor().top() + SP_1;
    let x = ui.max_rect().x_range();
    ui.painter().hline(x, y, egui::Stroke::new(1.0, CONTROL));
    ui.add_space(SP_2 + 1.0);
}

/// A heading that starts a group of related controls.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(SP_3);
    ui.label(caption(title));
    ui.add_space(SP_HALF);
}

/// A row with a label on the left and a control on the right.
pub fn row(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(ROW_LABEL_W, CONTROL_SM), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(ROW_LABEL_W);
            ui.label(caption(label));
        });
        content(ui);
    });
}

/// A row of plain read-only information.
pub fn info_row(ui: &mut Ui, label: &str, value: &str) {
    row(ui, label, |ui| {
        ui.label(body(value));
    });
}

/// A thin line between groups inside a bar or a panel.
/// Lays out a row against the right edge, in reading order. It uses last frame's width, since a
/// row's width is only known after drawing it.
pub fn right_aligned(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    let key = ui.make_persistent_id(id);
    let last: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
    ui.add_space((ui.available_width() - last - SP_2).max(SP_3));
    let row = ui.horizontal(add);
    ui.data_mut(|d| d.insert_temp(key, row.response.rect.width()));
}

pub fn divider_v(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SP_2 + 1.0, CONTROL_SM), egui::Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, BORDER));
}

pub fn divider_h(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), SP_2 + 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, BORDER));
}

/// What a panel shows when there's nothing in it yet: what this is for, and what to do next.
/// Returns the action's response if there is one.
pub fn empty_state(ui: &mut Ui, rect: Rect, title: &str, text: &str, action: Option<&str>) -> Option<Response> {
    let center = rect.center() - egui::vec2(0.0, if action.is_some() { 24.0 } else { 8.0 });
    let painter = ui.painter_at(rect);
    painter.text(center, Align2::CENTER_CENTER, title, font_heading(), TEXT_2);
    painter.text(center + egui::vec2(0.0, 20.0), Align2::CENTER_CENTER, text, font_caption(), TEXT_3);
    action.map(|label| {
        let galley = painter.layout_no_wrap(label.to_string(), font_label(), TEXT);
        let size = egui::vec2(galley.size().x + 20.0, CONTROL_H);
        let at = Rect::from_center_size(center + egui::vec2(0.0, 52.0), size);
        ui.put(at, Button::primary(label))
    })
}

/// A project to offer again on the start screen.
#[derive(Clone, Debug)]
pub struct RecentItem {
    pub name: String,
    /// The folder it's in.
    pub place: String,
    pub when: String,
}

/// The empty state for the whole workspace, used once: before anything has been added. Below the two
/// actions are recent projects, as many as fit. Also returns which of those was clicked.
pub fn hero_state(ui: &mut Ui, rect: Rect, title: &str, text: &str, action: &str, other: &str, recent: &[RecentItem]) -> (Response, Response, Option<usize>) {
    const INTRO_H: f32 = 94.0;
    const RECENT_LABEL_H: f32 = 44.0;
    const RECENT_ROW_H: f32 = 30.0;
    const RECENT_W: f32 = 460.0;
    let room = ((rect.height() - INTRO_H - RECENT_LABEL_H - SP_6) / RECENT_ROW_H).max(0.0) as usize;
    let rows = &recent[..recent.len().min(room).min(6)];
    let list_h = if rows.is_empty() { 0.0 } else { RECENT_LABEL_H + rows.len() as f32 * RECENT_ROW_H };
    let c = egui::pos2(rect.center().x, rect.center().y - (INTRO_H + list_h) * 0.5 + 16.0);
    let painter = ui.painter_at(rect);
    painter.text(c, Align2::CENTER_CENTER, title, font_display(), TEXT);
    painter.text(c + egui::vec2(0.0, 28.0), Align2::CENTER_CENTER, text, font_body(), TEXT_3);
    let galley = painter.layout_no_wrap(action.to_string(), font_label(), TEXT);
    let other_galley = painter.layout_no_wrap(other.to_string(), font_label(), TEXT);
    let (width, other_width) = (galley.size().x + 20.0, other_galley.size().x + 20.0);
    let left = c.x - (width + SP_2 + other_width) * 0.5;
    let at = Rect::from_min_size(egui::pos2(left, c.y + 66.0 - CONTROL_H * 0.5), egui::vec2(width, CONTROL_H));
    let other_at = Rect::from_min_size(egui::pos2(at.right() + SP_2, at.top()), egui::vec2(other_width, CONTROL_H));
    let (first, second) = (ui.put(at, Button::primary(action)), ui.put(other_at, Button::secondary(other)));

    let mut picked = None;
    if !rows.is_empty() {
        let width = RECENT_W.min(rect.width() - SP_6);
        let (x0, x1) = (c.x - width * 0.5, c.x + width * 0.5);
        let label_y = at.bottom() + RECENT_LABEL_H - SP_2;
        painter.text(egui::pos2(x0 + SP_2, label_y), Align2::LEFT_BOTTOM, "Recent", font_caption(), TEXT_3);
        for (i, item) in rows.iter().enumerate() {
            let top = label_y + SP_1 + i as f32 * RECENT_ROW_H;
            let row = Rect::from_min_max(egui::pos2(x0, top), egui::pos2(x1, top + RECENT_ROW_H));
            let response = ui.interact(row, ui.scope_id().with(("recent", i)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                painter.rect_filled(row, R_S, CONTROL);
            }
            if response.clicked() {
                picked = Some(i);
            }
            let y = row.center().y;
            let name = painter.text(egui::pos2(row.left() + SP_2, y), Align2::LEFT_CENTER, &item.name, font_label(), TEXT);
            let when = painter.text(egui::pos2(row.right() - SP_2, y), Align2::RIGHT_CENTER, &item.when, font_caption(), TEXT_3);
            let place = Rect::from_min_max(egui::pos2(name.right() + SP_3, row.top()), egui::pos2(when.left() - SP_3, row.bottom()));
            if place.is_positive() {
                let clipped = painter.with_clip_rect(place.intersect(rect));
                clipped.text(egui::pos2(place.left(), y), Align2::LEFT_CENTER, &item.place, font_caption(), TEXT_3);
            }
        }
    }
    (first, second, picked)
}

/// A shortcut in a list of them, with the keys in a column so that the descriptions line up.
pub fn shortcut_row(ui: &mut Ui, keys_text: &str, what: &str) {
    const KEYS_W: f32 = 62.0;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(KEYS_W, CONTROL_SM), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(KEYS_W);
            keycap(ui, keys_text);
        });
        ui.label(caption(what));
    });
}

/// A keycap followed by what it does.
pub fn hint_row(ui: &mut Ui, keys_text: &str, what: &str) {
    ui.horizontal(|ui| {
        keycap(ui, keys_text);
        ui.label(caption(what));
    });
}

