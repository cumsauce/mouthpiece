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

/// The empty state for the whole workspace, used once: before anything has been added.
pub fn hero_state(ui: &mut Ui, rect: Rect, title: &str, text: &str, action: &str) -> Response {
    let c = rect.center() - egui::vec2(0.0, 28.0);
    let painter = ui.painter_at(rect);
    painter.text(c, Align2::CENTER_CENTER, title, font_display(), TEXT);
    painter.text(c + egui::vec2(0.0, 28.0), Align2::CENTER_CENTER, text, font_body(), TEXT_3);
    let galley = painter.layout_no_wrap(action.to_string(), font_label(), TEXT);
    let at = Rect::from_center_size(c + egui::vec2(0.0, 66.0), egui::vec2(galley.size().x + 20.0, CONTROL_H));
    ui.put(at, Button::primary(action))
}

/// A keycap followed by what it does.
pub fn hint_row(ui: &mut Ui, keys_text: &str, what: &str) {
    ui.horizontal(|ui| {
        keycap(ui, keys_text);
        ui.label(caption(what));
    });
}

