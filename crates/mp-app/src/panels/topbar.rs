//! The top bar: project on the left, transport in the centre, panels and Render on the right.

use crate::actions::Action;
use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::timeline::{format_bars, format_time};
use eframe::egui::{self, Align2, Rect, Sense, Ui};
use mp_model::MusicalKey;

const CENTER_W: f32 = 566.0;
const RIGHT_W: f32 = 272.0;

impl App {
    pub fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        // Centred when there's room. On narrow windows it gives way to the fixed controls on the right.
        let center_x = bar.center().x.min(bar.right() - RIGHT_W - CENTER_W * 0.5);
        let center = Rect::from_center_size(egui::pos2(center_x, bar.center().y), egui::vec2(CENTER_W, bar.height()));
        let left = Rect::from_min_max(bar.min, egui::pos2(center.left() - SP_2, bar.bottom()));
        let right = Rect::from_min_max(egui::pos2(center.right() + SP_2, bar.top()), bar.max);

        let mut left_ui = ui.new_child(egui::UiBuilder::new().max_rect(left).layout(egui::Layout::left_to_right(egui::Align::Center)));
        left_ui.set_clip_rect(left);
        self.project_controls(&mut left_ui);
        let mut center_ui = ui.new_child(egui::UiBuilder::new().max_rect(center).layout(egui::Layout::left_to_right(egui::Align::Center)));
        self.transport_controls(&mut center_ui);
        let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        self.output_controls(&mut right_ui);
    }

    fn project_controls(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = SP_1;
        menu_button(ui, "Mouthpiece", |ui| {
            for (action, name, key) in [
                (Action::New, "New project", "⌘N"),
                (Action::Open, "Open…", "⌘O"),
                (Action::Save, "Save", "⌘S"),
                (Action::SaveAs, "Save as…", "⇧⌘S"),
                (Action::Import, "Add audio files…", "⌘I"),
                (Action::FindSong, "Find a song online…", "⇧⌘F"),
                (Action::Export, "Render…", "⇧⌘R"),
                (Action::AudioSettings, "Audio settings…", "⌘,"),
            ] {
                if menu_item(ui, name, Some(key)).clicked() {
                    self.run(action);
                    ui.close();
                }
            }
            if self.has_autosave() && menu_item(ui, "Recover autosave", None).clicked() {
                self.recover_autosave();
                ui.close();
            }
            menu_separator(ui);
            if menu_item(ui, "Keyboard shortcuts", Some("?")).clicked() {
                self.show_shortcuts = true;
                ui.close();
            }
        });
        let undo = self.history.undo_label().map(|l| format!("Undo {l}"));
        let redo = self.history.redo_label().map(|l| format!("Redo {l}"));
        if ui.add(IconButton::new(Icon::Undo, undo.as_deref().unwrap_or("Nothing to undo")).shortcut("⌘Z").enabled(undo.is_some())).clicked() {
            self.undo();
        }
        if ui.add(IconButton::new(Icon::Redo, redo.as_deref().unwrap_or("Nothing to redo")).shortcut("⇧⌘Z").enabled(redo.is_some())).clicked() {
            self.redo();
        }
        // On narrow windows the name gives way, as the title bar also shows it.
        if ui.available_width() > 90.0 {
            ui.add_space(SP_1);
            let dirty = if self.is_dirty() { " ●" } else { "" };
            ui.label(caption(truncate(&format!("{}{dirty}", self.project.name), 18)));
        }
    }

    fn transport_controls(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = SP_1;
        if ui.add(IconButton::new(Icon::ToStart, "Stop and return").shortcut("Return").size(28.0)).clicked() {
            self.run(Action::Stop);
        }
        let playing = self.engine.is_playing();
        let (icon, name) = if playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
        if ui.add(IconButton::new(icon, name).shortcut("Space").active(playing).tint(GOOD).size(28.0)).clicked() {
            self.run(Action::PlayPause);
        }
        if ui.add(IconButton::new(Icon::Loop, "Loop. Shift-drag on the ruler to set it.").shortcut("⌘L").active(self.project.loop_region.enabled).size(28.0)).clicked() {
            self.run(Action::ToggleLoop);
        }
        if ui.add(IconButton::new(Icon::Metronome, "Metronome").shortcut("K").active(self.project.metronome).size(28.0)).clicked() {
            self.run(Action::ToggleMetronome);
        }
        ui.add_space(SP_1);
        self.time_readout(ui);
        ui.add_space(SP_1);

        let mut bpm = self.project.bpm;
        let r = ui.add(ValueField::new("BPM", &mut bpm, 40.0..=240.0).decimals(1).default(120.0).width(92.0));
        if r.changed() {
            self.begin_gesture("Change tempo");
            self.tempo_user_set = true;
            self.project.bpm = bpm;
            self.needs_compile = true;
        }
        if ui.add(Button::ghost("Tap").tip("Tap along to set the tempo", None)).clicked() {
            self.tap_tempo();
        }
        let current = self.project.key;
        let mut chosen = current;
        dropdown(ui, "key", current.map_or("No key".to_string(), |k| k.label()), 56.0, |ui| {
            menu_choice(ui, &mut chosen, None, "No key");
            for minor in [false, true] {
                for root in 0..12 {
                    let key = MusicalKey { root, minor };
                    menu_choice(ui, &mut chosen, Some(key), &key.label());
                }
            }
        });
        if chosen != current {
            self.edit("Change key", |p| p.key = chosen);
        }
        ui.add_space(SP_1);
        if ui.add(IconButton::new(Icon::Magnet, "Snap. Hold ⌘ while dragging to bypass.").shortcut("S").active(self.snap)).clicked() {
            self.snap = !self.snap;
        }
        dropdown(ui, "grid", grid_label(self.grid_division), 40.0, |ui| {
            for value in [4.0, 2.0, 1.0, 0.5, 0.25, 0.125] {
                menu_choice(ui, &mut self.grid_division, value, &grid_label(value));
            }
        });
    }

    fn time_readout(&mut self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(134.0, 30.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, R_S, BASE);
        painter.text(rect.left_center() + egui::vec2(SP_2, 0.0), Align2::LEFT_CENTER, format_bars(&self.project, self.playhead), font_num(14.0), TEXT);
        painter.text(rect.right_center() - egui::vec2(SP_2, 0.0), Align2::RIGHT_CENTER, format_time(self.playhead), font_num(10.0), TEXT_3);
    }

    fn output_controls(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = SP_1;
        ui.add_space(SP_1);
        if ui.add(Button::primary("Render").tip("Render to MP3 or WAV", Some("⇧⌘R"))).clicked() {
            self.run(Action::Export);
        }
        ui.add_space(SP_2);
        let peak = self.engine.shared.master_peak();
        for c in 0..2 {
            self.master_hold[c] = peak[c].max(self.master_hold[c] * 0.86);
        }
        let (rect, response) = ui.allocate_exact_size(egui::vec2(72.0, 10.0), Sense::hover());
        meter(ui.painter(), rect, self.master_hold);
        tip(response, "Master level", None);
        ui.add_space(SP_2);
        if ui.add(IconButton::new(Icon::PanelRight, "Say panel").shortcut("⌘B").active(self.show_say)).clicked() {
            self.run(Action::ToggleSay);
        }
        if ui.add(IconButton::new(Icon::PanelBottom, "Bottom panel").shortcut("Tab").active(self.show_bottom)).clicked() {
            self.run(Action::ToggleEditor);
        }
        if ui.add(IconButton::new(Icon::PanelLeft, "Inspector").shortcut("I").active(self.show_inspector)).clicked() {
            self.run(Action::ToggleInspector);
        }
        if ui.add(IconButton::new(Icon::Commands, "All commands").shortcut("⌘K")).clicked() {
            self.run(Action::Palette);
        }
    }
}

fn grid_label(division: f64) -> String {
    if division >= 4.0 {
        "Bar".to_string()
    } else {
        format!("1/{}", (4.0 / division) as u32)
    }
}
