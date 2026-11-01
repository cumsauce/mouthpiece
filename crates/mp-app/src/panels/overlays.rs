//! What appears over everything else: dialogs, toasts, and drag feedback.

use crate::actions::{shortcut_text, ACTIONS};
use crate::app::{App, ToastAction, ToastKind};
use crate::ds::*;
use crate::jobs::file_stem;
use eframe::egui::{self, Align2, Rect, Stroke, Ui};

impl App {
    pub fn modals(&mut self, ctx: &egui::Context) {
        self.render_modal(ctx);
        self.discard_modal(ctx);
        self.word_modal(ctx);
        self.rename_track_modal(ctx);
        if self.palette_open {
            self.palette(ctx);
        }
        if self.show_shortcuts {
            self.shortcuts_modal(ctx);
        }
    }

    fn rename_track_modal(&mut self, ctx: &egui::Context) {
        let Some((track, mut name, mut focused)) = self.rename_track.take() else { return };
        let mut confirm = false;
        let mut cancel = false;
        let dismissed = modal(ctx, "rename-track", 300.0, |ui: &mut Ui| {
            ui.label(heading("Rename track"));
            ui.add_space(SP_3);
            let id = egui::Id::unique("track-name");
            let edit = egui::Frame::NONE.fill(BASE).corner_radius(R_S).inner_margin(egui::Margin::same(SP_2 as i8)).show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut name).id(id).frame(egui::Frame::NONE).font(font_input()).desired_width(f32::INFINITY))
            }).inner;
            if !focused {
                edit.request_focus();
                focused = true;
            }
            confirm = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                confirm |= ui.add(Button::primary("Rename").enabled(!name.trim().is_empty())).clicked();
                cancel = ui.add(Button::ghost("Cancel")).clicked();
            });
        });
        let trimmed = name.trim().to_string();
        if confirm && !trimmed.is_empty() {
            self.edit("Rename track", |p| {
                if let Some(t) = p.track_mut(track) {
                    t.name = trimmed;
                }
            });
        } else if !dismissed && !cancel {
            self.rename_track = Some((track, name, focused));
        }
    }

    /// Names a word, whether it's being saved or renamed.
    fn word_modal(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.word_dialog.take() else { return };
        let mut confirm = false;
        let mut cancel = false;
        let existing = self.project.word_named(&dialog.name).map(|w| w.id);
        let updates = dialog.renaming.is_none() && existing.is_some();
        let dismissed = modal(ctx, "word", 340.0, |ui: &mut Ui| {
            ui.label(heading(if dialog.renaming.is_some() { "Rename word" } else { "Save as a word" }));
            ui.add_space(SP_1);
            ui.label(caption("Type it in Say to use it again. Copies you've placed won't change."));
            ui.add_space(SP_3);
            let id = egui::Id::unique("word-name");
            let edit = egui::Frame::NONE.fill(BASE).corner_radius(R_S).inner_margin(egui::Margin::same(SP_2 as i8)).show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut dialog.name).id(id).frame(egui::Frame::NONE).font(font_input()).desired_width(f32::INFINITY).hint_text("What it says"))
            }).inner;
            if !dialog.focused {
                edit.request_focus();
                dialog.focused = true;
            }
            confirm = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if updates { "Update word" } else if dialog.renaming.is_some() { "Rename" } else { "Save word" };
                confirm |= ui.add(Button::primary(label).enabled(!dialog.name.trim().is_empty())).clicked();
                cancel = ui.add(Button::ghost("Cancel")).clicked();
            });
        });
        let name = dialog.name.trim().to_string();
        if confirm && !name.is_empty() {
            match dialog.renaming {
                Some(word) => self.edit("Rename word", |p| {
                    if let Some(w) = p.words.iter_mut().find(|w| w.id == word) {
                        w.name = name.clone();
                    }
                }),
                None => {
                    let clips = dialog.clips.clone();
                    self.edit("Save word", |p| {
                        p.save_word(&name, &clips);
                    });
                    self.toast(ToastKind::Good, format!("Saved \"{name}\". Type it in Say to use it again."));
                }
            }
            self.say.dirty = true;
        } else if !dismissed && !cancel {
            self.word_dialog = Some(dialog);
        }
    }

    /// Asks what to do with unsaved changes before New, Open or Quit.
    fn discard_modal(&mut self, ctx: &egui::Context) {
        let Some(next) = self.pending_discard.clone() else { return };
        let mut choice = None;
        let mut cancel = false;
        let name = self.project.name.clone();
        let dismissed = modal(ctx, "unsaved", 340.0, |ui: &mut Ui| {
            ui.label(heading(format!("Save changes to \"{name}\"?")));
            ui.add_space(SP_1);
            ui.label(caption("Without saving, your changes are lost. This can't be undone."));
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(Button::primary("Save")).clicked() {
                    choice = Some(true);
                }
                if ui.add(Button::secondary("Don't save")).clicked() {
                    choice = Some(false);
                }
                cancel = ui.add(Button::ghost("Cancel")).clicked();
            });
        });
        if dismissed || cancel {
            self.pending_discard = None;
        }
        if let Some(save) = choice {
            self.pending_discard = None;
            if save {
                self.save();
            }
            // A cancelled or failed save leaves the changes unsaved, so stay put.
            if !save || !self.is_dirty() {
                self.perform(next);
            }
        }
    }

    fn shortcuts_modal(&mut self, ctx: &egui::Context) {
        let dismissed = modal(ctx, "shortcuts", 760.0, |ui: &mut Ui| {
            ui.label(heading("Keyboard shortcuts"));
            ui.add_space(SP_2);
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 180.0).show(ui, |ui| {
                ui.columns(3, |columns| {
                    let groups = ["Transport", "Sentence mixing", "Clip", "Edit", "View", "File", "Track"];
                    for (i, group) in groups.iter().enumerate() {
                        let ui = &mut columns[(i * 3) / groups.len()];
                        ui.label(strong(*group));
                        ui.add_space(SP_1);
                        for (_, name, shortcut, g) in ACTIONS {
                            if g == group {
                                if let Some(s) = shortcut {
                                    hint_row(ui, &shortcut_text(s), name);
                                }
                            }
                        }
                        ui.add_space(SP_3);
                    }
                });
                ui.label(strong("In the Say input"));
                ui.add_space(SP_1);
                for (key, what) in [
                    ("Return", "Place the line"),
                    ("⇧Return", "Preview the line"),
                    ("Tab", "Next word"),
                    ("↑ ↓", "Choose a take"),
                    ("Esc", "Clear"),
                ] {
                    hint_row(ui, key, what);
                }
                ui.add_space(SP_3);
                ui.label(strong("Mouse"));
                ui.add_space(SP_1);
                for line in [
                    "⌘-scroll or pinch zooms. Shift-scroll or a horizontal swipe pans.",
                    "Drag a clip's edge to trim it. ⌥-drag an edge to stretch. ⌥-drag a clip to copy it.",
                    "Drag the dots at a clip's top corners to set fades. Hold ⌘ to bypass snapping.",
                    "Double-click a clip or track to open it in the editor.",
                ] {
                    ui.label(caption(keys(line)));
                }
            });
        });
        if dismissed {
            self.show_shortcuts = false;
        }
    }

    /// Follows the pointer while a word is being dragged out of a list or the editor.
    pub fn drag_ghost(&mut self, ctx: &egui::Context) {
        let text = match (&self.hit_drag, self.word_drag) {
            (Some(hit), _) => hit.text.clone(),
            (None, Some(word)) => self.project.word(word).map_or(String::new(), |w| w.name.clone()),
            (None, None) => return,
        };
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(p) = ctx.pointer_latest_pos() {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("word-ghost")));
            let galley = painter.layout_no_wrap(text, font_label(), TEXT_ON_ACCENT);
            let r = Rect::from_min_size(p + egui::vec2(SP_3, SP_3), galley.size() + egui::vec2(SP_3, 6.0));
            painter.rect_filled(r, R_S, ACCENT);
            painter.galley(r.min + egui::vec2(6.0, 3.0), galley, TEXT_ON_ACCENT);
        }
        // Released away from the timeline: the arranger only handles it over the lanes.
        if ctx.input(|i| i.pointer.any_released()) {
            self.hit_drag = None;
            self.word_drag = None;
        }
    }

    pub fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering: Vec<String> = ctx.input(|i| i.raw.hovered_files.iter().filter_map(|f| f.path.as_ref().map(|p| file_stem(p))).collect());
        if hovering.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
        painter.rect_filled(screen, 0.0, alpha(BASE, 200));
        painter.rect_stroke(screen.shrink(SP_6), R_L, Stroke::new(1.5, TEXT_3), egui::StrokeKind::Inside);
        let count = hovering.len();
        painter.text(screen.center(), Align2::CENTER_CENTER, format!("Drop to add {count} track{}", if count == 1 { "" } else { "s" }), font_display(), TEXT);
    }

    pub fn toasts_overlay(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let mut action = None;
        let mut y = screen.top() + TOPBAR_H + SP_2;
        let items: Vec<(usize, String, ToastKind, Option<ToastAction>, f32)> =
            self.toasts.iter().enumerate().map(|(i, t)| (i, t.text.clone(), t.kind, t.action.clone(), t.born.elapsed().as_secs_f32())).collect();
        for (i, text, kind, toast_action, age) in items {
            let opacity = ((TOAST_SECONDS - age) / 0.3).clamp(0.0, 1.0);
            let area = egui::Area::new(egui::Id::unique(("toast", i))).order(egui::Order::Tooltip).pivot(Align2::CENTER_TOP).fixed_pos(egui::pos2(screen.center().x, y)).show(ctx, |ui| {
                ui.set_opacity(opacity);
                floating_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_2;
                        ui.add_space(SP_1);
                        let tone = match kind {
                            ToastKind::Info => Tone::Neutral,
                            ToastKind::Good => Tone::Good,
                            ToastKind::Bad => Tone::Bad,
                        };
                        let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, CONTROL_SM), egui::Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.0, tone.color());
                        ui.label(body(text).color(TEXT));
                        if let Some(a) = toast_action {
                            let label = match &a {
                                ToastAction::Undo => "Undo",
                                ToastAction::Reveal(_) => "Show file",
                            };
                            if ui.add(Button::secondary(label).small()).clicked() {
                                action = Some(a);
                            }
                        }
                        ui.add_space(SP_1);
                    });
                });
            });
            y += area.response.rect.height() + SP_1;
        }
        match action {
            Some(ToastAction::Undo) => {
                self.toasts.clear();
                self.undo();
            }
            Some(ToastAction::Reveal(path)) => reveal(&path),
            None => {}
        }
    }
}

/// Shows a file in the system's file manager.
fn reveal(path: &std::path::Path) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn()
    };
    if let Err(e) = result {
        eprintln!("could not show file: {e}");
    }
}
