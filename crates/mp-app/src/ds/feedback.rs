//! Text styles, tooltips, keycaps, tags, progress and modals.

use super::tokens::*;
use eframe::egui::{self, Align2, Color32, Response, RichText, Sense, Ui};

// ----- Text. Use these rather than styling text by hand. -----

pub fn micro(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_micro()).color(TEXT_3)
}
/// Secondary information.
pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_caption()).color(TEXT_3)
}
pub fn body(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_body()).color(TEXT_2)
}
/// Emphasis at body size.
pub fn strong(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_label()).color(TEXT)
}
pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_heading()).color(TEXT)
}
/// The name of what's selected.
pub fn title(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_title()).color(TEXT)
}
/// Numbers that change.
pub fn num(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(font_num(11.0)).color(TEXT_2)
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
            .replace('⌥', "Alt+")
            .replace('⌫', "Backspace")
    }
}

/// Adds a tooltip with an optional shortcut as a keycap. Shortcuts are how people learn the app.
pub fn tip(response: Response, text: &str, shortcut: Option<&str>) -> Response {
    response.on_hover_ui(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_2;
            ui.label(body(text));
            if let Some(s) = shortcut {
                keycap(ui, s);
            }
        });
    })
}

/// A key name in a cap, such as "Space" or "⌘K".
pub fn keycap(ui: &mut Ui, text: &str) {
    let text = keys(text);
    let galley = ui.painter().layout_no_wrap(text, font_num(10.5), TEXT_2);
    let (rect, _) = ui.allocate_exact_size(galley.size() + egui::vec2(SP_2, 5.0), Sense::hover());
    ui.painter().rect_filled(rect, R_S, CONTROL);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, TEXT_2);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Neutral,
    Good,
    Warn,
    Bad,
}

impl Tone {
    pub fn color(self) -> Color32 {
        match self {
            Tone::Neutral => TEXT_3,
            Tone::Good => GOOD,
            Tone::Warn => WARN,
            Tone::Bad => BAD,
        }
    }
}

/// A small status dot with a label.
pub fn status_dot(ui: &mut Ui, tone: Tone, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, CONTROL_SM - 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, tone.color());
    ui.label(caption(text));
}

/// A bar showing work in progress. `None` means the amount of work isn't known.
pub fn progress(ui: &mut Ui, fraction: Option<f32>, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 3.0), Sense::hover());
    paint_progress(ui, rect, fraction);
}

pub fn paint_progress(ui: &Ui, rect: egui::Rect, fraction: Option<f32>) {
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(1), CONTROL);
    let bar = match fraction {
        Some(f) => egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * f.clamp(0.0, 1.0), rect.height())),
        None => {
            // Keeps moving, so an unknown wait never looks like a hang.
            let t = (ui.input(|i| i.time) as f32 * 0.8).fract();
            let x = rect.left() + (rect.width() * 1.3) * t - rect.width() * 0.3;
            ui.ctx().request_repaint();
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(rect.width() * 0.3, rect.height())).intersect(rect)
        }
    };
    if bar.is_positive() {
        painter.rect_filled(bar, egui::CornerRadius::same(1), TEXT_2);
    }
}

/// A modal dialog over a dimmed window. Closes with Esc or a click outside, by returning true.
pub fn modal(ctx: &egui::Context, name: &str, width: f32, content: impl FnOnce(&mut Ui)) -> bool {
    let screen = ctx.content_rect();
    let mut dismissed = false;
    egui::Area::new(egui::Id::unique((name, "scrim"))).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let response = ui.allocate_response(screen.size(), Sense::click());
        ui.painter().rect_filled(screen, 0.0, alpha(BASE, 170));
        dismissed = response.clicked();
    });
    egui::Area::new(egui::Id::unique((name, "body")))
        .order(egui::Order::Tooltip)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(RAISED)
                .stroke(egui::Stroke::new(1.0, BORDER))
                .corner_radius(R_L)
                .inner_margin(egui::Margin::same(SP_4 as i8))
                .shadow(shadow())
                .show(ui, |ui| {
                    ui.set_width(width);
                    content(ui);
                });
        });
    dismissed || ctx.input(|i| i.key_pressed(egui::Key::Escape))
}
