//! The left panel: properties of whatever is selected, and what to do next when nothing is.

use crate::actions::Action;
use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::timeline::format_time;
use eframe::egui::{self, Sense, Ui};
use mp_model::{ClipId, ClipMode, TrackKind};

impl App {
    pub fn inspector_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if !self.selection.is_empty() {
                self.clip_inspector(ui);
            } else if self.selected_track.is_some() {
                self.track_inspector(ui);
            } else {
                self.project_inspector(ui);
            }
        });
    }

    fn set_selected(&mut self, f: impl Fn(&mut mp_model::Clip)) {
        for id in self.selection.clone() {
            if let Some(c) = self.project.clip_mut(id) {
                f(c);
            }
        }
        self.needs_compile = true;
    }

    fn clip_inspector(&mut self, ui: &mut Ui) {
        let Some(first) = self.selection.first().and_then(|c| self.project.clip(*c)).cloned() else { return };
        let n = self.selection.len();
        let name = match &first.word_name {
            Some(word) if n > 1 && n == self.project.group_members(first.id).len() => word.clone(),
            _ if n > 1 => format!("{n} clips"),
            _ => self.clip_label(&first),
        };
        ui.label(title(truncate(&name, 22)));
        if let Some(s) = self.project.source(first.source) {
            ui.label(caption(truncate(&s.name, 30)));
        }

        if n == 1 {
            let takes = self.takes_of_clip(&first);
            if takes.len() > 1 {
                section(ui, "Take");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SP_1;
                    if ui.add(IconButton::new(Icon::ChevronLeft, "Previous take").shortcut("⇧T")).clicked() {
                        self.run(Action::PrevTake);
                    }
                    ui.label(num(format!("{} of {}", self.current_take(&first, &takes) + 1, takes.len())));
                    if ui.add(IconButton::new(Icon::ChevronRight, "Next take").shortcut("T")).clicked() {
                        self.run(Action::NextTake);
                    }
                });
            }
        }

        section(ui, "Pitch");
        let elastic = first.mode == ClipMode::Elastic;
        let mut semis = first.pitch as f64;
        let r = ui.add(ValueField::new("Shift", &mut semis, -24.0..=24.0).unit(" st").decimals(2).bipolar().default(0.0).enabled(elastic));
        if r.changed() {
            self.begin_gesture("Change pitch");
            self.set_selected(|c| c.pitch = semis as f32);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_1;
            if ui.add(Button::secondary("Tune to key").small().icon(Icon::Note).tip("Move to the nearest note of the key", Some("P"))).clicked() {
                self.snap_selection_pitch();
            }
            self.pitch_readout(ui, first.id);
        });
        let mut formants = first.preserve_formants;
        if elastic && switch(ui, &mut formants, "Keep voice character").changed() {
            self.begin_gesture("Change voice character");
            self.set_selected(|c| c.preserve_formants = formants);
            self.end_gesture();
        }

        section(ui, "Time");
        let mut speed = 100.0 / first.stretch;
        let r = ui.add(ValueField::new("Speed", &mut speed, 10.0..=400.0).log().unit("%").decimals(0).default(100.0));
        if r.changed() {
            self.begin_gesture("Change speed");
            for id in self.selection.clone() {
                self.project.set_stretch(id, 100.0 / speed);
            }
            self.needs_compile = true;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_1;
            let mut mode = first.mode;
            if segmented(ui, &mut mode, &[(ClipMode::Elastic, "Keep pitch"), (ClipMode::Varispeed, "Tape")]) {
                let ids = self.selection.clone();
                self.edit("Change stretch mode", |p| {
                    ids.iter().filter_map(|i| p.clip_mut(*i).map(|c| c.mode = mode)).for_each(drop);
                });
            }
            if ui.add(Button::ghost("Fit tempo").small().tip("Stretch to the project's tempo", None)).clicked() {
                self.run(Action::FitToTempo);
            }
        });

        section(ui, "Level");
        let mut gain = first.gain_db as f64;
        let r = ui.add(ValueField::new("Gain", &mut gain, -48.0..=24.0).unit(" dB").default(0.0));
        if r.changed() {
            self.begin_gesture("Change clip gain");
            self.set_selected(|c| c.gain_db = gain as f32);
        }
        let mut pan = first.pan as f64;
        let r = ui.add(ValueField::new("Pan", &mut pan, -1.0..=1.0).decimals(2).bipolar().default(0.0));
        if r.changed() {
            self.begin_gesture("Change clip pan");
            self.set_selected(|c| c.pan = pan as f32);
        }

        section(ui, "Fades");
        let mut fade_in = first.fade_in * 1000.0;
        let r = ui.add(ValueField::new("In", &mut fade_in, 0.0..=2000.0).unit(" ms").decimals(0).default(4.0));
        if r.changed() {
            self.begin_gesture("Change fade");
            self.set_selected(|c| c.fade_in = (fade_in * 0.001).min(c.length()));
        }
        let mut fade_out = first.fade_out * 1000.0;
        let r = ui.add(ValueField::new("Out", &mut fade_out, 0.0..=2000.0).unit(" ms").decimals(0).default(4.0));
        if r.changed() {
            self.begin_gesture("Change fade");
            self.set_selected(|c| c.fade_out = (fade_out * 0.001).min(c.length()));
        }

        section(ui, "Clip");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
            if ui.add(Button::secondary("Reverse").small().active(first.reversed).tip("Play backwards", Some("R"))).clicked() {
                self.run(Action::Reverse);
            }
            if ui.add(Button::secondary("Mute").small().active(first.muted).tip("Silence this clip", Some("M"))).clicked() {
                self.run(Action::MuteClip);
            }
            if ui.add(Button::secondary("Slice to words").small().tip("One clip per word", Some("W"))).clicked() {
                self.run(Action::SliceToWords);
            }
            if ui.add(Button::secondary("Quantize").small().tip("Snap the start to the grid", Some("Q"))).clicked() {
                self.run(Action::Quantize);
            }
            if ui.add(Button::secondary("Save as a word").small().tip("Find it again in Say", Some("⇧W"))).clicked() {
                self.run(Action::SaveWord);
            }
            if first.made_word.is_some() && ui.add(Button::secondary("Revert to saved").small().tip("Back to the word as you saved it", None)).clicked() {
                self.run(Action::RevertWord);
            }
            if ui.add(Button::secondary("Restore original").small().tip("Undo everything done to this clip, keeping its position. The recording itself is never changed.", Some("⇧R"))).clicked() {
                self.run(Action::RestoreOriginal);
            }
        });
        ui.add_space(SP_2);
        info_row(ui, "Starts", &format_time(first.start));
        info_row(ui, "Length", &format!("{:.3} s", first.length()));
    }

    /// Shows a selected word's note, which is what you need when tuning by hand.
    fn pitch_readout(&mut self, ui: &mut Ui, clip: ClipId) {
        let Some(c) = self.project.clip(clip).cloned() else { return };
        // Long clips don't have one note to show.
        if c.src_len() > 3.0 {
            return;
        }
        if let Some(hz) = self.clip_pitch(c.source, c.src_in, c.src_out) {
            let heard = mp_dsp::hz_to_midi(hz) + c.effective_pitch();
            ui.label(num(mp_dsp::midi_name(heard)));
        }
    }

    fn track_inspector(&mut self, ui: &mut Ui) {
        let Some(id) = self.selected_track else { return };
        let Some(track) = self.project.track(id).cloned() else { return };
        let mut name = track.name.clone();
        let r = text_field(ui, &mut name, "Track name", ui.available_width());
        if r.changed() {
            self.begin_gesture("Rename track");
            if let Some(t) = self.project.track_mut(id) {
                t.name = name;
            }
        }
        ui.add_space(SP_1);
        let clips = track.clips.len();
        ui.label(caption(format!("{clips} clip{}", if clips == 1 { "" } else { "s" })));

        section(ui, "Level");
        let mut gain = track.gain_db as f64;
        let r = ui.add(ValueField::new("Gain", &mut gain, -60.0..=12.0).unit(" dB").default(0.0));
        if r.changed() {
            self.begin_gesture("Change track gain");
            if let Some(t) = self.project.track_mut(id) {
                t.gain_db = gain as f32;
            }
            self.needs_compile = true;
        }
        let mut pan = track.pan as f64;
        let r = ui.add(ValueField::new("Pan", &mut pan, -1.0..=1.0).decimals(2).bipolar().default(0.0));
        if r.changed() {
            self.begin_gesture("Change track pan");
            if let Some(t) = self.project.track_mut(id) {
                t.pan = pan as f32;
            }
            self.needs_compile = true;
        }

        section(ui, "Type");
        let mut kind = track.kind;
        if segmented(ui, &mut kind, &[(TrackKind::Vocal, "Vocal"), (TrackKind::Drums, "Drums"), (TrackKind::Bass, "Bass"), (TrackKind::Music, "Music")]) {
            self.edit("Change track type", |p| {
                if let Some(t) = p.track_mut(id) {
                    t.kind = kind;
                }
            });
        }

        section(ui, "Colour");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_HALF;
            for c in 0..8u8 {
                let (r, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                ui.painter().circle_filled(r.center(), if track.color % 8 == c || response.hovered() { 8.0 } else { 6.0 }, track_color(c));
                if response.clicked() {
                    self.edit("Change colour", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.color = c;
                        }
                    });
                }
            }
        });

        section(ui, "Devices");
        for d in &track.devices {
            ui.label(body(d.kind.name()));
        }
        if ui.add(Button::secondary(if track.devices.is_empty() { "Add a device" } else { "Edit devices" }).small().tip("Open the Devices tab", Some("2"))).clicked() {
            self.run(Action::ShowDevices);
        }
    }

    /// With nothing selected, this says where you are in the work and what comes next.
    fn project_inspector(&mut self, ui: &mut Ui) {
        ui.label(title(truncate(&self.project.name, 22)));
        ui.label(caption(format!("{} tracks · {}", self.project.tracks.len(), format_time(self.project.duration()))));

        section(ui, "Next steps");
        let has_stems = !self.project.tracks.is_empty();
        let has_words = self.index.word_count() > 0;
        let has_line = self.project.tracks.iter().any(|t| t.name == "Sentence" && !t.clips.is_empty());
        let steps = [
            (has_stems, "Drop your stems in"),
            (has_words, "Let the vocal transcribe"),
            (has_line, "Say something"),
            (false, "Render it"),
        ];
        let next = steps.iter().position(|(done, _)| !done);
        for (i, (done, text)) in steps.iter().enumerate() {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                if *done {
                    draw_icon(ui.painter(), r.shrink(2.0), Icon::Check, GOOD);
                } else {
                    ui.painter().circle_stroke(r.center(), 4.0, eframe::egui::Stroke::new(1.0, if next == Some(i) { TEXT } else { CONTROL_HOVER }));
                }
                let label = if next == Some(i) { strong(*text) } else { caption(*text) };
                ui.label(label);
            });
        }
        section(ui, "Worth knowing");
        for (key, what) in [("/", "Say something"), ("⌘K", "Every command"), ("T", "Next take"), ("X", "Crossfade clips"), ("⇧W", "Save as a word"), ("Tab", "Bottom panel"), ("?", "All shortcuts")] {
            hint_row(ui, key, what);
        }
    }
}
