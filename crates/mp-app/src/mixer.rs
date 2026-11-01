//! The mixer: one vertical channel strip per track, plus master.

use crate::app::App;
use crate::arranger::truncate;
use crate::theme::*;
use crate::widgets::{fader, letter_toggle, meter};
use eframe::egui::{self, CornerRadius, Sense, Ui};
use mp_model::TrackId;

const STRIP_W: f32 = 76.0;

impl App {
    pub fn mixer_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.add_space(4.0);
                let ids: Vec<TrackId> = self.project.tracks.iter().map(|t| t.id).collect();
                for (index, id) in ids.iter().enumerate() {
                    self.track_strip(ui, index, *id);
                }
                ui.add_space(8.0);
                self.master_strip(ui);
            });
        });
    }

    fn strip_frame(ui: &mut Ui, selected: bool, content: impl FnOnce(&mut Ui)) {
        let height = ui.available_height().max(100.0);
        egui::Frame::NONE
            .fill(if selected { BG_3 } else { BG_2 })
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(5))
            .show(ui, |ui| {
                ui.set_width(STRIP_W - 10.0);
                ui.set_height(height - 10.0);
                ui.vertical_centered(content);
            });
    }

    fn track_strip(&mut self, ui: &mut Ui, index: usize, id: TrackId) {
        let Some(track) = self.project.track(id).cloned() else { return };
        let peaks = self.decayed_meter(index);
        let selected = self.selected_track == Some(id) && !self.master_selected;
        Self::strip_frame(ui, selected, |ui| {
            let name = ui.add(egui::Label::new(egui::RichText::new(truncate(&track.name, 10)).size(11.0).color(track_color(track.color))).sense(Sense::click()));
            if name.clicked() {
                self.selected_track = Some(id);
                self.master_selected = false;
            }
            if name.double_clicked() {
                self.open_track_in_editor(id);
            }
            let mut pan = track.pan;
            let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false));
            if r.changed() {
                if let Some(t) = self.project.track_mut(id) {
                    t.pan = pan;
                }
                self.needs_compile = true;
            }
            if r.drag_started() {
                self.begin_gesture("Change track pan");
            }
            if r.drag_stopped() {
                self.end_gesture();
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(12.0);
                if letter_toggle(ui, "M", track.mute, MUTE, "Mute").clicked() {
                    self.edit("Mute track", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.mute = !t.mute;
                        }
                    });
                }
                if letter_toggle(ui, "S", track.solo, SOLO, "Solo").clicked() {
                    self.edit("Solo track", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.solo = !t.solo;
                        }
                    });
                }
            });
            let mut db = track.gain_db;
            let height = (ui.available_height() - 18.0).max(40.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let f = fader(ui, &mut db, egui::vec2(16.0, height));
                if f.started {
                    self.begin_gesture("Change track gain");
                }
                if f.changed {
                    if let Some(t) = self.project.track_mut(id) {
                        t.gain_db = db;
                    }
                    self.needs_compile = true;
                }
                if f.finished {
                    self.end_gesture();
                }
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, height), Sense::hover());
                meter(ui.painter(), rect, peaks);
            });
            ui.label(egui::RichText::new(format!("{db:+.1}")).font(mono(10.0)).color(TEXT_DIM));
        });
    }

    fn master_strip(&mut self, ui: &mut Ui) {
        let peak = self.engine.shared.master_peak();
        for c in 0..2 {
            self.master_hold[c] = peak[c].max(self.master_hold[c] * 0.86);
        }
        let hold = self.master_hold;
        Self::strip_frame(ui, self.master_selected, |ui| {
            let name = ui.add(egui::Label::new(egui::RichText::new("Master").size(11.0).strong().color(TEXT)).sense(Sense::click()));
            if name.clicked() {
                self.master_selected = true;
            }
            ui.add_space(50.0);
            let mut db = self.project.master.gain_db;
            let height = (ui.available_height() - 18.0).max(40.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let f = fader(ui, &mut db, egui::vec2(16.0, height));
                if f.started {
                    self.begin_gesture("Change master gain");
                }
                if f.changed {
                    self.project.master.gain_db = db;
                    self.needs_compile = true;
                }
                if f.finished {
                    self.end_gesture();
                }
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, height), Sense::hover());
                meter(ui.painter(), rect, hold);
            });
            ui.label(egui::RichText::new(format!("{db:+.1}")).font(mono(10.0)).color(TEXT_DIM));
        });
    }
}
