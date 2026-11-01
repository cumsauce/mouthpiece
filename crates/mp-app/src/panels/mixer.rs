//! The mixer: one vertical channel strip per track, plus master.

use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Sense, Ui};
use mp_model::TrackId;

const STRIP_W: f32 = 72.0;

impl App {
    pub fn mixer_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = SP_1;
                ui.add_space(SP_2);
                let ids: Vec<TrackId> = self.project.tracks.iter().map(|t| t.id).collect();
                for (index, id) in ids.iter().enumerate() {
                    self.track_strip(ui, index, *id);
                }
                ui.add_space(SP_2);
                self.master_strip(ui);
            });
        });
    }

    fn strip(ui: &mut Ui, selected: bool, content: impl FnOnce(&mut Ui)) {
        let height = ui.available_height().max(100.0);
        let frame = if selected { card_frame().fill(CONTROL) } else { card_frame() };
        frame.show(ui, |ui| {
            ui.set_width(STRIP_W - SP_4);
            ui.set_height(height - SP_4);
            ui.vertical_centered(content);
        });
    }

    fn strip_fader(&mut self, ui: &mut Ui, db: &mut f32, peaks: [f32; 2]) -> FaderOutput {
        let height = (ui.available_height() - 18.0).max(40.0);
        let output = ui
            .horizontal(|ui| {
                ui.add_space(10.0);
                let f = fader(ui, db, egui::vec2(16.0, height));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, height), Sense::hover());
                meter(ui.painter(), rect, peaks);
                f
            })
            .inner;
        ui.label(num(format!("{db:+.1}")));
        output
    }

    fn track_strip(&mut self, ui: &mut Ui, index: usize, id: TrackId) {
        let Some(track) = self.project.track(id).cloned() else { return };
        let peaks = self.decayed_meter(index);
        let selected = self.selected_track == Some(id) && !self.master_selected;
        Self::strip(ui, selected, |ui| {
            let name = ui.add(egui::Label::new(strong(truncate(&track.name, 8)).color(track_color(track.color))).truncate().sense(Sense::click()));
            if name.clicked() {
                self.selected_track = Some(id);
                self.master_selected = false;
                self.track_focus = true;
                self.selection.clear();
            }
            if name.double_clicked() {
                self.open_track_in_editor(id);
            }
            let mut pan = track.pan as f64;
            let r = ui.add(ValueField::new("", &mut pan, -1.0..=1.0).decimals(1).bipolar().default(0.0));
            if r.changed() {
                self.begin_gesture("Change track pan");
                if let Some(t) = self.project.track_mut(id) {
                    t.pan = pan as f32;
                }
                self.needs_compile = true;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP_HALF;
                ui.add_space(SP_2);
                if letter_toggle(ui, "M", track.mute, MUTE, "Mute", None).clicked() {
                    self.edit("Mute track", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.mute = !t.mute;
                        }
                    });
                }
                if letter_toggle(ui, "S", track.solo, SOLO, "Solo", None).clicked() {
                    self.edit("Solo track", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.solo = !t.solo;
                        }
                    });
                }
            });
            let mut db = track.gain_db;
            let f = self.strip_fader(ui, &mut db, peaks);
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
        });
    }

    fn master_strip(&mut self, ui: &mut Ui) {
        let peak = self.engine.shared.master_peak();
        for c in 0..2 {
            self.master_hold[c] = peak[c].max(self.master_hold[c] * 0.86);
        }
        let hold = self.master_hold;
        Self::strip(ui, self.master_selected, |ui| {
            let name = ui.add(egui::Label::new(strong("Master")).sense(Sense::click()));
            if name.clicked() {
                self.master_selected = true;
            }
            ui.add_space(CONTROL_H + CONTROL_SM + SP_1);
            let mut db = self.project.master.gain_db;
            let f = self.strip_fader(ui, &mut db, hold);
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
        });
    }
}
