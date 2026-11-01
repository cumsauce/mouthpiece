//! The left panel: properties of whatever is selected.

use crate::actions::Action;
use crate::app::App;
use crate::arranger::truncate;
use crate::theme::*;
use crate::timeline::format_time;
use crate::widgets;
use eframe::egui::{self, Response, Ui};
use mp_model::{ClipId, ClipMode};

impl App {
    /// Ties a value widget to the undo system: a drag or text edit counts as one undo step.
    fn gesture_for(&mut self, response: &Response, label: &str) {
        if response.drag_started() || response.gained_focus() {
            self.begin_gesture(label);
        }
        if response.drag_stopped() || response.lost_focus() {
            self.end_gesture();
        }
    }

    pub fn inspector_panel(&mut self, ui: &mut Ui) {
        // Sliders sized to fit, so the panel doesn't change width with the selection.
        ui.spacing_mut().slider_width = 92.0;
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

    fn section(ui: &mut Ui, title: &str) {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(title.to_uppercase()).size(9.5).color(TEXT_FAINT));
        ui.add_space(1.0);
    }

    fn clip_inspector(&mut self, ui: &mut Ui) {
        let Some(first) = self.selection.first().and_then(|c| self.project.clip(*c)).cloned() else { return };
        let n = self.selection.len();
        let title = if n > 1 { format!("{n} clips") } else { self.clip_label(&first) };
        ui.label(egui::RichText::new(truncate(&title, 26)).size(14.0).color(TEXT).strong());
        let source = self.project.source(first.source).cloned();
        if let Some(s) = &source {
            ui.label(egui::RichText::new(truncate(&s.name, 30)).size(10.5).color(TEXT_FAINT));
        }

        Self::section(ui, "Pitch");
        let mut semis = first.pitch;
        let elastic = first.mode == ClipMode::Elastic;
        ui.add_enabled_ui(elastic, |ui| {
            let r = ui.add(egui::Slider::new(&mut semis, -24.0..=24.0).suffix(" st").fixed_decimals(2).step_by(0.01));
            if r.changed() {
                self.set_selected(|c| c.pitch = semis);
            }
            self.gesture_for(&r, "Change pitch");
        });
        ui.horizontal(|ui| {
            if ui.small_button("−1").clicked() {
                self.run(Action::PitchDown);
            }
            if ui.small_button("+1").clicked() {
                self.run(Action::PitchUp);
            }
            if ui.small_button("Snap to key").on_hover_text("Tune to the nearest note of the project's key (P)").clicked() {
                self.snap_selection_pitch();
            }
            if ui.small_button("0").on_hover_text("Reset pitch").clicked() {
                let ids = self.selection.clone();
                self.edit("Reset pitch", |p| {
                    ids.iter().filter_map(|i| p.clip_mut(*i).map(|c| c.pitch = 0.0)).for_each(drop);
                });
            }
        });
        self.pitch_readout(ui, first.id);
        let mut formants = first.preserve_formants;
        if ui.add_enabled(elastic, egui::Checkbox::new(&mut formants, "Keep voice character")).on_hover_text("Preserves formants, so voices don't sound like chipmunks when repitched").changed() {
            self.edit("Change formants", |_| {});
            self.begin_gesture("Change formants");
            self.set_selected(|c| c.preserve_formants = formants);
            self.end_gesture();
        }

        Self::section(ui, "Time");
        let mut speed = (100.0 / first.stretch) as f32;
        let r = ui.add(egui::Slider::new(&mut speed, 10.0..=400.0).logarithmic(true).suffix("% speed").fixed_decimals(1));
        if r.changed() {
            let ratio = 100.0 / speed as f64;
            for id in self.selection.clone() {
                self.project.set_stretch(id, ratio);
            }
            self.needs_compile = true;
        }
        self.gesture_for(&r, "Change speed");
        ui.horizontal(|ui| {
            for (label, value) in [("½×", 2.0), ("1×", 1.0), ("2×", 0.5)] {
                if ui.small_button(label).clicked() {
                    let ids = self.selection.clone();
                    self.edit("Change speed", |p| ids.iter().for_each(|i| p.set_stretch(*i, value)));
                }
            }
            if ui.small_button("Fit tempo").on_hover_text("Stretch to the project's tempo").clicked() {
                self.run(Action::FitToTempo);
            }
        });
        ui.horizontal(|ui| {
            let mut mode = first.mode;
            ui.selectable_value(&mut mode, ClipMode::Elastic, "Elastic").on_hover_text("Time and pitch change independently");
            ui.selectable_value(&mut mode, ClipMode::Varispeed, "Tape").on_hover_text("Speed changes pitch, like a record");
            if mode != first.mode {
                let ids = self.selection.clone();
                self.edit("Change stretch mode", |p| {
                    ids.iter().filter_map(|i| p.clip_mut(*i).map(|c| c.mode = mode)).for_each(drop);
                });
            }
        });

        Self::section(ui, "Level");
        let mut gain = first.gain_db;
        let r = ui.add(egui::Slider::new(&mut gain, -48.0..=24.0).suffix(" dB").fixed_decimals(1));
        if r.changed() {
            self.set_selected(|c| c.gain_db = gain);
        }
        self.gesture_for(&r, "Change clip gain");
        let mut pan = first.pan;
        let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).text("Pan").fixed_decimals(2));
        if r.changed() {
            self.set_selected(|c| c.pan = pan);
        }
        self.gesture_for(&r, "Change clip pan");

        Self::section(ui, "Fades");
        let (mut fade_in, mut fade_out) = ((first.fade_in * 1000.0) as f32, (first.fade_out * 1000.0) as f32);
        let r = ui.add(egui::Slider::new(&mut fade_in, 0.0..=2000.0).logarithmic(true).suffix(" ms in").fixed_decimals(0));
        if r.changed() {
            self.set_selected(|c| c.fade_in = (fade_in as f64 * 0.001).min(c.length()));
        }
        self.gesture_for(&r, "Change fade");
        let r = ui.add(egui::Slider::new(&mut fade_out, 0.0..=2000.0).logarithmic(true).suffix(" ms out").fixed_decimals(0));
        if r.changed() {
            self.set_selected(|c| c.fade_out = (fade_out as f64 * 0.001).min(c.length()));
        }
        self.gesture_for(&r, "Change fade");

        Self::section(ui, "Actions");
        ui.horizontal_wrapped(|ui| {
            let reversed = first.reversed;
            if ui.selectable_label(reversed, "Reverse").clicked() {
                self.run(Action::Reverse);
            }
            if ui.selectable_label(first.muted, "Mute").clicked() {
                self.run(Action::MuteClip);
            }
            if ui.button("Split").clicked() {
                self.run(Action::Split);
            }
            if ui.button("Slice to words").clicked() {
                self.run(Action::SliceToWords);
            }
            if ui.button("Quantize").clicked() {
                self.run(Action::Quantize);
            }
            if n > 1 && ui.button("Close gaps").clicked() {
                self.run(Action::CloseGaps);
            }
        });

        Self::section(ui, "Source");
        let dim = |ui: &mut Ui, text: String| {
            ui.label(egui::RichText::new(text).size(11.0).color(TEXT_DIM));
        };
        dim(ui, format!("Starts {}", format_time(first.start)));
        dim(ui, format!("Length {:.3} s", first.length()));
        dim(ui, format!("From {} to {}", format_time(first.src_in), format_time(first.src_out)));
    }

    /// Shows a selected word's note, which is what you need when tuning by hand.
    fn pitch_readout(&self, ui: &mut Ui, clip: ClipId) {
        let Some(c) = self.project.clip(clip) else { return };
        let Some(runtime) = self.runtime.get(&c.source) else { return };
        // Long clips have no single note, and measuring them each frame would be wasteful.
        if c.src_len() > 3.0 {
            return;
        }
        let sr = self.sample_rate as f32;
        let a = ((c.src_in * sr as f64) as usize).min(runtime.mono.len());
        let b = ((c.src_out * sr as f64) as usize).min(runtime.mono.len());
        if let Some(hz) = mp_dsp::pitch::median_pitch(&runtime.mono[a..b.max(a)], sr) {
            let original = mp_dsp::hz_to_midi(hz);
            let heard = original + c.effective_pitch();
            ui.label(egui::RichText::new(format!("Note {} ({:.0} Hz)", mp_dsp::midi_name(heard), hz)).size(11.0).color(TEXT_DIM));
        }
    }

    fn track_inspector(&mut self, ui: &mut Ui) {
        let Some(id) = self.selected_track else { return };
        let Some(track) = self.project.track(id).cloned() else { return };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, track_color(track.color));
            let mut name = track.name.clone();
            let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0).font(egui::TextStyle::Body));
            if edit.changed() {
                if let Some(t) = self.project.track_mut(id) {
                    t.name = name;
                }
            }
            self.gesture_for(&edit, "Rename track");
        });
        ui.label(egui::RichText::new(format!("{} · {} clip{}", track.kind.label(), track.clips.len(), if track.clips.len() == 1 { "" } else { "s" })).size(11.0).color(TEXT_FAINT));

        Self::section(ui, "Level");
        let mut gain = track.gain_db;
        let r = ui.add(egui::Slider::new(&mut gain, -60.0..=12.0).suffix(" dB").fixed_decimals(1));
        if r.changed() {
            if let Some(t) = self.project.track_mut(id) {
                t.gain_db = gain;
            }
            self.needs_compile = true;
        }
        self.gesture_for(&r, "Change track gain");
        let mut pan = track.pan;
        let r = ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).text("Pan").fixed_decimals(2));
        if r.changed() {
            if let Some(t) = self.project.track_mut(id) {
                t.pan = pan;
            }
            self.needs_compile = true;
        }
        self.gesture_for(&r, "Change track pan");

        Self::section(ui, "Color");
        ui.horizontal(|ui| {
            for c in 0..8u8 {
                let (r, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), if track.color % 8 == c { 8.0 } else { 6.0 }, track_color(c));
                if response.clicked() {
                    self.edit("Change color", |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.color = c;
                        }
                    });
                }
            }
        });

        Self::section(ui, "Devices");
        if track.devices.is_empty() {
            ui.label(egui::RichText::new("None. Open the Devices tab to add some.").size(11.0).color(TEXT_FAINT));
        }
        for d in &track.devices {
            ui.label(egui::RichText::new(d.kind.name()).size(11.5).color(if d.enabled { TEXT } else { TEXT_FAINT }));
        }
        ui.add_space(4.0);
        if ui.button("Edit devices").clicked() {
            self.run(Action::ShowDevices);
        }
    }

    fn project_inspector(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new(self.project.name.clone()).size(14.0).color(TEXT).strong());
        ui.label(egui::RichText::new(format!("{} tracks · {}", self.project.tracks.len(), format_time(self.project.duration()))).size(11.0).color(TEXT_FAINT));
        Self::section(ui, "Shortcuts worth knowing");
        for (key, what) in [("⌘K", "Every command"), ("⌘F", "Search words"), ("⌘B", "Sentence Builder"), ("S", "Split"), ("Tab", "Show or hide editor"), ("⇧/", "All shortcuts")] {
            ui.horizontal(|ui| {
                widgets::keycap(ui, &widgets::keys(key));
                ui.label(egui::RichText::new(what).size(11.0).color(TEXT_DIM));
            });
        }
    }
}
