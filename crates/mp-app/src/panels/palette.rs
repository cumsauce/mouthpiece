//! The command palette: every command, found by typing.

use crate::actions::{shortcut_text, ACTIONS};
use crate::app::App;
use crate::ds::*;
use eframe::egui::{self, Align2, Key, Sense};

impl App {
    pub fn palette(&mut self, ctx: &egui::Context) {
        let query = self.palette_query.to_lowercase();
        let matches: Vec<usize> = ACTIONS
            .iter()
            .enumerate()
            .filter(|(_, (_, name, _, group))| {
                let hay = format!("{} {}", name.to_lowercase(), group.to_lowercase());
                query.split_whitespace().all(|q| hay.contains(q))
            })
            .map(|(i, _)| i)
            .collect();
        let screen = ctx.content_rect();
        let mut run = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("palette-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let response = ui.allocate_response(screen.size(), Sense::click());
            ui.painter().rect_filled(screen, 0.0, alpha(BASE, 140));
            close = response.clicked();
        });
        egui::Area::new(egui::Id::unique("palette"))
            .order(egui::Order::Tooltip)
            .fixed_pos(egui::pos2(screen.center().x - 280.0, screen.top() + 84.0))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(RAISED)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(R_L)
                    .inner_margin(egui::Margin::same(SP_2 as i8))
                    .shadow(shadow())
                    .show(ui, |ui| {
                        ui.set_width(560.0);
                        let (up, down, enter, escape) = ui.input_mut(|i| {
                            (
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                                i.key_pressed(Key::Enter),
                                i.key_pressed(Key::Escape),
                            )
                        });
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.palette_query)
                                .id(egui::Id::unique("palette-input"))
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::NONE)
                                .font(font_input())
                                .hint_text("Type a command"),
                        );
                        edit.request_focus();
                        if edit.changed() {
                            self.palette_index = 0;
                        }
                        if down {
                            self.palette_index = (self.palette_index + 1).min(matches.len().saturating_sub(1));
                        }
                        if up {
                            self.palette_index = self.palette_index.saturating_sub(1);
                        }
                        close |= escape;
                        if enter {
                            run = matches.get(self.palette_index).map(|i| ACTIONS[*i].0);
                            close = true;
                        }
                        divider_h(ui);
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (row, index) in matches.iter().enumerate() {
                                let (action, name, shortcut, group) = ACTIONS[*index];
                                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
                                let selected = row == self.palette_index;
                                if selected || response.hovered() {
                                    ui.painter().rect_filled(rect, R_S, if selected { CONTROL_HOVER } else { CONTROL });
                                }
                                if selected && (up || down) {
                                    ui.scroll_to_rect(rect, None);
                                }
                                ui.painter().text(rect.left_center() + egui::vec2(SP_2, 0.0), Align2::LEFT_CENTER, name, font_body(), TEXT);
                                let right = match shortcut {
                                    Some(s) => format!("{group}    {}", shortcut_text(&s)),
                                    None => group.to_string(),
                                };
                                ui.painter().text(rect.right_center() - egui::vec2(SP_2, 0.0), Align2::RIGHT_CENTER, right, font_caption(), TEXT_3);
                                if response.clicked() {
                                    run = Some(action);
                                    close = true;
                                }
                            }
                            if matches.is_empty() {
                                ui.add_space(SP_2);
                                ui.label(caption("No commands match"));
                            }
                        });
                    });
            });
        if close {
            self.palette_open = false;
        }
        if let Some(action) = run {
            self.run(action);
        }
    }
}
