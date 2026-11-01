//! The bar that floats over the timeline while clips are selected, with what you'd do to them next.

use crate::actions::Action;
use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Ui};
use mp_model::ClipMode;

impl App {
    pub fn selection_bar(&mut self, ctx: &egui::Context) {
        if self.selection.is_empty() || self.clip_drag.is_some() || self.hit_drag.is_some() || self.word_drag.is_some() || self.marquee.is_some() {
            return;
        }
        let rect = self.arr_lanes_rect;
        egui::Area::new(egui::Id::unique("selection-bar"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(rect.center().x, rect.bottom() - SP_2))
            .show(ctx, |ui| {
                floating_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_HALF;
                        self.selection_bar_contents(ui);
                    });
                });
            });
    }

    fn selection_bar_contents(&mut self, ui: &mut Ui) {
        let Some(first) = self.selection.first().and_then(|c| self.project.clip(*c)).cloned() else { return };
        let count = self.selection.len();
        let name = match &first.word_name {
            Some(word) if count > 1 && self.selection.len() == self.project.group_members(first.id).len() => truncate(word, 18),
            _ if count > 1 => format!("{count} clips"),
            _ => truncate(&self.clip_label(&first), 18),
        };
        ui.add_space(SP_1);
        ui.label(strong(name));
        ui.add_space(SP_1);

        if count == 1 {
            let takes = self.takes_of_clip(&first);
            if takes.len() > 1 {
                divider_v(ui);
                if ui.add(IconButton::new(Icon::ChevronLeft, "Previous take").shortcut("⇧T").small()).clicked() {
                    self.run(Action::PrevTake);
                }
                ui.label(num(format!("{}/{}", self.current_take(&first, &takes) + 1, takes.len())));
                if ui.add(IconButton::new(Icon::ChevronRight, "Next take").shortcut("T").small()).clicked() {
                    self.run(Action::NextTake);
                }
            }
        }

        divider_v(ui);
        if ui.add(IconButton::new(Icon::Minus, "Down a semitone").shortcut("↓").small()).clicked() {
            self.run(Action::PitchDown);
        }
        ui.label(num(if first.mode == ClipMode::Elastic { format!("{:+.1}st", first.pitch) } else { "tape".to_string() }));
        if ui.add(IconButton::new(Icon::Plus, "Up a semitone").shortcut("↑").small()).clicked() {
            self.run(Action::PitchUp);
        }
        if ui.add(IconButton::new(Icon::Note, "Tune to key").shortcut("P").small()).clicked() {
            self.run(Action::SnapPitchToKey);
        }

        divider_v(ui);
        if ui.add(IconButton::new(Icon::Slower, "Slower").shortcut("]").small()).clicked() {
            self.run(Action::Slower);
        }
        ui.label(num(format!("{:.0}%", 100.0 / first.stretch)));
        if ui.add(IconButton::new(Icon::Faster, "Faster").shortcut("[").small()).clicked() {
            self.run(Action::Faster);
        }

        divider_v(ui);
        if ui.add(IconButton::new(Icon::Reverse, "Reverse").shortcut("R").active(first.reversed).small()).clicked() {
            self.run(Action::Reverse);
        }
        if ui.add(IconButton::new(Icon::Repeat, "Repeat, to stutter").shortcut("⇧D").small()).clicked() {
            self.run(Action::Repeat);
        }
        if ui.add(IconButton::new(Icon::Split, "Split at the playhead").shortcut("S").small()).clicked() {
            self.run(Action::Split);
        }
        if count > 1 && ui.add(IconButton::new(Icon::Wand, "Close the gaps").shortcut("G").small()).clicked() {
            self.run(Action::CloseGaps);
        }

        divider_v(ui);
        if ui.add(IconButton::new(Icon::Search, "Try other sounds and takes in place").shortcut("F").small()).clicked() {
            self.run(Action::TrySounds);
        }
        if first.patch.is_some() && ui.add(IconButton::new(Icon::Compare, "Compare with the original").shortcut("A").small()).clicked() {
            self.run(Action::CompareChange);
        }
        if count > 1 && ui.add(IconButton::new(Icon::Crossfade, "Crossfade together. Drag to change how long.").shortcut("X").small()).clicked() {
            self.run(Action::Crossfade);
        }
        if first.group.is_some() {
            if ui.add(IconButton::new(Icon::Link, "Ungroup").shortcut("⇧⌘G").active(true).small()).clicked() {
                self.run(Action::Ungroup);
            }
        } else if count > 1 && ui.add(IconButton::new(Icon::Link, "Group").shortcut("⌘G").small()).clicked() {
            self.run(Action::Group);
        }
        if ui.add(IconButton::new(Icon::Bookmark, "Save as a word to use again").shortcut("⇧W").small()).clicked() {
            self.run(Action::SaveWord);
        }
        if first.made_word.is_some() && ui.add(IconButton::new(Icon::Undo, "Revert to the saved word").small()).clicked() {
            self.run(Action::RevertWord);
        }
        if ui.add(IconButton::new(Icon::Restore, "Restore to how it was first placed").shortcut("⇧R").small()).clicked() {
            self.run(Action::RestoreOriginal);
        }
        divider_v(ui);
        if ui.add(IconButton::new(Icon::Trash, "Delete").shortcut("⌫").small()).clicked() {
            self.run(Action::Delete);
        }
    }
}
