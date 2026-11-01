//! Controls: everything that takes input, other than the audio-specific ones in `audio.rs`.

use super::feedback::{keys, tip};
use super::icons::{draw_icon, Icon};
use super::tokens::*;
use eframe::egui::{self, Align2, Color32, Response, Sense, Stroke, Ui, Widget};
use std::fmt::Display;

#[derive(Clone, Copy, PartialEq)]
enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

/// A text button. `primary` is for the one main action in view, and everything else is `secondary`
/// or, in dense or repeated places, `ghost`.
pub struct Button<'a> {
    text: &'a str,
    kind: ButtonKind,
    icon: Option<Icon>,
    small: bool,
    enabled: bool,
    active: bool,
    fill_width: bool,
    tooltip: Option<(&'a str, Option<&'a str>)>,
}

impl<'a> Button<'a> {
    fn new(text: &'a str, kind: ButtonKind) -> Self {
        Self { text, kind, icon: None, small: false, enabled: true, active: false, fill_width: false, tooltip: None }
    }
    pub fn primary(text: &'a str) -> Self {
        Self::new(text, ButtonKind::Primary)
    }
    pub fn secondary(text: &'a str) -> Self {
        Self::new(text, ButtonKind::Secondary)
    }
    pub fn ghost(text: &'a str) -> Self {
        Self::new(text, ButtonKind::Ghost)
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Shows the button as switched on, for buttons that toggle a state.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }
    /// A tooltip, with its shortcut if it has one.
    pub fn tip(mut self, text: &'a str, shortcut: Option<&'a str>) -> Self {
        self.tooltip = Some((text, shortcut));
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let height = if self.small { CONTROL_SM } else { CONTROL_H };
        let font = if self.small { font_caption() } else { font_label() };
        let galley = ui.painter().layout_no_wrap(self.text.to_string(), font, Color32::PLACEHOLDER);
        let padding = if self.small { SP_2 } else { 10.0 };
        let icon_w = if self.icon.is_some() { 12.0 + SP_1 + 1.0 } else { 0.0 };
        let content = galley.size().x + icon_w;
        let width = if self.fill_width { ui.available_width() } else { content + padding * 2.0 };
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), sense);
        let hover = ui.ctx().animate_bool_with_time(response.id, self.enabled && response.hovered(), MOTION_HOVER);
        let (fill, fg) = match (self.kind, self.active) {
            (ButtonKind::Primary, _) => (mix(ACCENT, ACCENT_HOVER, hover), TEXT_ON_ACCENT),
            (_, true) => (CONTROL_HOVER, TEXT),
            (ButtonKind::Secondary, _) => (mix(CONTROL, CONTROL_HOVER, hover), TEXT),
            (ButtonKind::Ghost, _) => (alpha(CONTROL_HOVER, (hover * 255.0) as u8), mix(TEXT_2, TEXT, hover)),
        };
        let (fill, fg) = if self.enabled { (fill, fg) } else { (mix(PANEL, fill, 0.4), TEXT_3) };
        ui.painter().rect_filled(rect, R_S, fill);
        let mut x = rect.center().x - content * 0.5;
        if let Some(icon) = self.icon {
            let icon_rect = egui::Rect::from_center_size(egui::pos2(x + 6.0, rect.center().y), egui::vec2(11.0, 11.0));
            draw_icon(ui.painter(), icon_rect, icon, fg);
            x += icon_w;
        }
        ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y * 0.5), galley, fg);
        match self.tooltip {
            Some((text, shortcut)) => tip(response, text, shortcut),
            None => response,
        }
    }
}

/// A square button holding one icon. It has no background until hovered, so rows of them stay quiet.
/// Every icon button needs a tooltip, because the icon alone doesn't say what it does.
pub struct IconButton<'a> {
    icon: Icon,
    text: &'a str,
    shortcut: Option<&'a str>,
    active: bool,
    tint: Color32,
    size: f32,
    enabled: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, text: &'a str) -> Self {
        Self { icon, text, shortcut: None, active: false, tint: TEXT, size: CONTROL_H, enabled: true }
    }
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
    /// Shows it as switched on, in `TEXT` unless `tint` says otherwise.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn tint(mut self, tint: Color32) -> Self {
        self.tint = tint;
        self
    }
    pub fn small(mut self) -> Self {
        self.size = CONTROL_SM;
        self
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(self.size, self.size), sense);
        let hover = ui.ctx().animate_bool_with_time(response.id, self.enabled && response.hovered(), MOTION_HOVER);
        let fill = if self.active {
            CONTROL
        } else {
            alpha(CONTROL_HOVER, (hover * 255.0) as u8)
        };
        ui.painter().rect_filled(rect, R_S, fill);
        let color = if !self.enabled {
            alpha(TEXT_3, 110)
        } else if self.active {
            self.tint
        } else {
            mix(TEXT_2, TEXT, hover)
        };
        let inset = (self.size * 0.27).round();
        draw_icon(ui.painter(), rect.shrink(inset), self.icon, color);
        tip(response, self.text, self.shortcut)
    }
}

/// A small lettered toggle, such as Mute or Solo on a track.
pub fn letter_toggle(ui: &mut Ui, letter: &str, on: bool, tint: Color32, text: &str, shortcut: Option<&str>) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(CONTROL_SM - 2.0, CONTROL_SM - 2.0), Sense::click());
    let fill = if on { tint } else if response.hovered() { CONTROL_HOVER } else { CONTROL };
    ui.painter().rect_filled(rect, R_S, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, font_label(), if on { BASE } else { TEXT_2 });
    tip(response, text, shortcut)
}

/// Choosing one of a few options, shown side by side. Returns true when the choice changes.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = font_caption();
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| ui.painter().layout_no_wrap(label.to_string(), font.clone(), TEXT).size().x + SP_3)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + SP_HALF * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, CONTROL_SM + SP_HALF), Sense::hover());
    ui.painter().rect_filled(rect, R_S, CONTROL);
    let mut x = rect.left() + SP_HALF;
    let mut changed = false;
    for (i, (option, label)) in options.iter().enumerate() {
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top() + SP_HALF), egui::vec2(widths[i], rect.height() - SP_1));
        let id = ui.scope_id().with(("segment", i, rect.left() as i32, rect.top() as i32));
        let response = ui.interact(cell, id, Sense::click());
        let selected = *value == *option;
        if selected {
            ui.painter().rect_filled(cell, egui::CornerRadius::same(3), CONTROL_HOVER);
        }
        let color = if selected || response.hovered() { TEXT } else { TEXT_2 };
        ui.painter().text(cell.center(), Align2::CENTER_CENTER, *label, font.clone(), color);
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
        x += widths[i];
    }
    changed
}

/// A tab bar. The selected tab's underline shows where you are. Returns true on a change.
pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_1;
        for (option, label) in options {
            let selected = *value == *option;
            let galley = ui.painter().layout_no_wrap(label.to_string(), font_label(), Color32::PLACEHOLDER);
            let (rect, response) = ui.allocate_exact_size(egui::vec2(galley.size().x + SP_3, CONTROL_H), Sense::click());
            let color = if selected || response.hovered() { TEXT } else { TEXT_3 };
            if selected {
                let underline = egui::Rect::from_min_max(egui::pos2(rect.left() + SP_1, rect.bottom() - SP_HALF), egui::pos2(rect.right() - SP_1, rect.bottom()));
                ui.painter().rect_filled(underline, egui::CornerRadius::same(1), TEXT);
            }
            ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
            if response.clicked() && !selected {
                *value = *option;
                changed = true;
            }
        }
    });
    changed
}

/// A compact on/off switch with a label to its right.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), font_caption(), Color32::PLACEHOLDER);
    let size = egui::vec2(24.0 + SP_1 + 2.0 + galley.size().x, CONTROL_SM);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, MOTION_HOVER);
    let track = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 7.0), egui::vec2(24.0, 14.0));
    ui.painter().rect_filled(track, egui::CornerRadius::same(7), mix(CONTROL_HOVER, TEXT_2, t));
    let knob_x = track.left() + 7.0 + t * 10.0;
    ui.painter().circle_filled(egui::pos2(knob_x, track.center().y), 5.0, if *on { BASE } else { TEXT });
    let color = if response.hovered() { TEXT } else { TEXT_2 };
    ui.painter().galley(egui::pos2(track.right() + SP_1 + 2.0, rect.center().y - galley.size().y * 0.5), galley, color);
    response
}

#[derive(Clone, Copy, PartialEq)]
pub enum FieldScale {
    Linear,
    Log,
}

/// A number you can scrub: drag across it, double-click to type, and Alt-click to reset. It shows
/// its label, value and position in one control, and changes nothing on a plain click. Check
/// `response.changed()`, then begin an undo gesture before applying the value.
pub struct ValueField<'a> {
    label: &'a str,
    value: &'a mut f64,
    min: f64,
    max: f64,
    default: f64,
    scale: FieldScale,
    unit: &'a str,
    decimals: usize,
    bipolar: bool,
    width: Option<f32>,
    enabled: bool,
}

impl<'a> ValueField<'a> {
    pub fn new(label: &'a str, value: &'a mut f64, range: std::ops::RangeInclusive<f64>) -> Self {
        let (min, max) = (*range.start(), *range.end());
        Self { label, value, min, max, default: min, scale: FieldScale::Linear, unit: "", decimals: 1, bipolar: false, width: None, enabled: true }
    }
    pub fn default(mut self, default: f64) -> Self {
        self.default = default;
        self
    }
    pub fn log(mut self) -> Self {
        self.scale = FieldScale::Log;
        self
    }
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }
    /// Fills from the middle, for values with a neutral centre, such as pitch and pan.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn to_norm(&self, v: f64) -> f64 {
        match self.scale {
            FieldScale::Linear => (v - self.min) / (self.max - self.min),
            FieldScale::Log => (v.max(self.min) / self.min).ln() / (self.max / self.min).ln(),
        }
    }

    fn from_norm(&self, n: f64) -> f64 {
        match self.scale {
            FieldScale::Linear => self.min + n * (self.max - self.min),
            FieldScale::Log => self.min * (self.max / self.min).powf(n),
        }
    }
}

impl Widget for ValueField<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let sense = if self.enabled { Sense::click_and_drag() } else { Sense::hover() };
        let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), sense);
        let edit_id = response.id.with("edit");
        let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
        let mut new_value = None;

        if let Some(mut text) = editing {
            ui.painter().rect_filled(rect, R_S, BASE);
            ui.painter().rect_stroke(rect, R_S, Stroke::new(1.0, TEXT_3), egui::StrokeKind::Inside);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(SP_2, 4.0))));
            let edit = child.add(
                egui::TextEdit::singleline(&mut text)
                    .id(edit_id.with("text"))
                    .frame(egui::Frame::NONE)
                    .font(font_num(11.5))
                    .desired_width(f32::INFINITY),
            );
            edit.request_focus();
            let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if escape {
                ui.data_mut(|d| d.remove::<String>(edit_id));
            } else if enter || edit.clicked_elsewhere() {
                let cleaned: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+')).collect();
                new_value = cleaned.parse::<f64>().ok();
                ui.data_mut(|d| d.remove::<String>(edit_id));
            } else {
                ui.data_mut(|d| d.insert_temp(edit_id, text));
            }
        } else {
            if response.dragged_by(egui::PointerButton::Primary) {
                let fine = ui.input(|i| i.modifiers.shift);
                let dx = response.drag_delta().x as f64 / width as f64 * if fine { 0.1 } else { 1.0 };
                if dx != 0.0 {
                    let norm = (self.to_norm(*self.value) + dx).clamp(0.0, 1.0);
                    new_value = Some(self.from_norm(norm));
                }
            }
            if response.clicked() && ui.input(|i| i.modifiers.alt) {
                new_value = Some(self.default);
            }
            if response.double_clicked() {
                let start = format!("{:.*}", self.decimals, *self.value);
                ui.data_mut(|d| d.insert_temp(edit_id, start));
            }
            let hover = ui.ctx().animate_bool_with_time(response.id, self.enabled && (response.hovered() || response.dragged_by(egui::PointerButton::Primary)), MOTION_HOVER);
            ui.painter().rect_filled(rect, R_S, mix(CONTROL, mix(CONTROL, CONTROL_HOVER, 0.5), hover));
            let norm = self.to_norm(*self.value).clamp(0.0, 1.0) as f32;
            let (a, b) = if self.bipolar { (norm.min(0.5), norm.max(0.5)) } else { (0.0, norm) };
            let fill = egui::Rect::from_min_max(
                egui::pos2(rect.left() + rect.width() * a, rect.top()),
                egui::pos2(rect.left() + rect.width() * b, rect.bottom()),
            );
            ui.painter().with_clip_rect(rect).rect_filled(fill, R_S, mix(CONTROL, CONTROL_HOVER, 0.9));
            let color = if self.enabled { TEXT_2 } else { TEXT_3 };
            ui.painter().text(rect.left_center() + egui::vec2(SP_2, 0.0), Align2::LEFT_CENTER, self.label, font_caption(), color);
            let text = format!("{:.*}{}", self.decimals, *self.value, self.unit);
            ui.painter().text(rect.right_center() - egui::vec2(SP_2, 0.0), Align2::RIGHT_CENTER, text, font_num(11.0), if self.enabled { TEXT } else { TEXT_3 });
            if self.enabled && (response.hovered() || response.dragged_by(egui::PointerButton::Primary)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if let Some(v) = new_value {
            let v = v.clamp(self.min, self.max);
            if v != *self.value {
                *self.value = v;
                response.mark_changed();
            }
        }
        if self.enabled {
            response.on_hover_text_at_pointer(keys("Drag to change. Double-click to type. ⌥-click to reset."))
        } else {
            response
        }
    }
}

/// A text box that fits the design system. For the big line input, see `panels/say.rs`.
pub fn text_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let frame = egui::Frame::NONE.fill(BASE).corner_radius(R_S).inner_margin(egui::Margin::symmetric(SP_2 as i8, 4));
    frame
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(text).frame(egui::Frame::NONE).desired_width(width - SP_4).hint_text(hint))
        })
        .inner
}

/// A dropdown. Fill `add_items` with `menu_item`s.
pub fn dropdown(ui: &mut Ui, id: &str, selected: impl Display, width: f32, add_items: impl FnOnce(&mut Ui)) {
    egui::ComboBox::from_id_salt(id).selected_text(selected.to_string()).width(width).show_ui(ui, |ui| {
        add_items(ui);
    });
}

/// A button that opens a menu.
pub fn menu_button(ui: &mut Ui, text: &str, add_items: impl FnOnce(&mut Ui)) {
    ui.menu_button(text, |ui| {
        ui.set_min_width(180.0);
        add_items(ui);
    });
}

/// One row in a menu or dropdown. Keep the shortcut in sync with `ACTIONS` in `actions.rs`.
pub fn menu_item(ui: &mut Ui, text: &str, shortcut: Option<&str>) -> Response {
    let button = egui::Button::new(text).frame_when_inactive(false);
    let button = match shortcut {
        Some(s) => button.shortcut_text(keys(s)),
        None => button,
    };
    ui.add(button)
}

/// A dropdown row that shows whether it's chosen.
pub fn menu_choice<T: PartialEq>(ui: &mut Ui, value: &mut T, option: T, text: &str) -> Response {
    let selected = *value == option;
    let response = ui.add(egui::Button::selectable(selected, text));
    if response.clicked() {
        *value = option;
    }
    response
}

pub fn menu_separator(ui: &mut Ui) {
    ui.separator();
}
