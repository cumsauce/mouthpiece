//! The render dialog.

use crate::app::{App, ToastKind};
use crate::ds::*;
use crate::panels::timeline::format_time;
use eframe::egui::{self, Ui};
use mp_export::{ExportSettings, Format};
use mp_model::Project;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RangeChoice {
    Project,
    Loop,
    Selection,
}

pub struct ExportDialog {
    pub format: Format,
    pub range: RangeChoice,
    pub tail_seconds: f64,
    pub title: String,
}

impl ExportDialog {
    pub fn new(project: &Project) -> Self {
        Self { format: Format::Mp3 { kbps: 320 }, range: RangeChoice::Project, tail_seconds: 1.0, title: project.name.clone() }
    }
}

impl App {
    fn range_seconds(&self, range: RangeChoice) -> (f64, f64) {
        match range {
            RangeChoice::Project => (0.0, self.project.duration()),
            RangeChoice::Loop => (self.project.loop_region.start, self.project.loop_region.end),
            RangeChoice::Selection => {
                let clips: Vec<_> = self.selection.iter().filter_map(|c| self.project.clip(*c)).collect();
                let a = clips.iter().map(|c| c.start).fold(f64::MAX, f64::min);
                let b = clips.iter().map(|c| c.end()).fold(0.0, f64::max);
                if clips.is_empty() { (0.0, 0.0) } else { (a, b) }
            }
        }
    }

    pub fn start_export(&mut self, path: PathBuf, format: Format, range: RangeChoice, tail_seconds: f32, title: String) {
        let (a, b) = self.range_seconds(range);
        if b <= a {
            self.toast(ToastKind::Bad, "Nothing to render in that range");
            return;
        }
        let settings = ExportSettings {
            format,
            start: self.seconds_to_frames(a),
            end: self.seconds_to_frames(b),
            tail_seconds,
            title,
        };
        let peaks = self.runtime.iter().map(|(id, r)| (*id, r.peaks.clone())).collect();
        self.jobs.export(self.project.clone(), self.bank.clone(), self.cache.clone(), self.sample_rate, settings, peaks, path);
    }

    pub fn render_modal(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.export_dialog.take() else { return };
        let mut go = false;
        let mut cancel = false;
        let dismissed = modal(ctx, "render", 480.0, |ui| {
            ui.label(heading("Render"));
            ui.add_space(SP_3);
            self.render_form(ui, &mut dialog);
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                go = ui.add(Button::primary("Render…")).clicked();
                cancel = ui.add(Button::ghost("Cancel")).clicked();
            });
        });
        if go {
            let extension = dialog.format.extension();
            let picker = rfd::FileDialog::new()
                .add_filter(extension.to_uppercase(), &[extension])
                .set_file_name(format!("{}.{extension}", dialog.title));
            if let Some(path) = picker.save_file() {
                self.start_export(path, dialog.format, dialog.range, dialog.tail_seconds as f32, dialog.title.clone());
                return;
            }
        }
        if !dismissed && !cancel {
            self.export_dialog = Some(dialog);
        }
    }

    fn render_form(&mut self, ui: &mut Ui, dialog: &mut ExportDialog) {
        let can_render_video = self.project.has_video() && mp_model::video::ffmpeg_path().is_some();
        row(ui, "Format", |ui| {
            let mut options = vec![
                (Format::Mp3 { kbps: 320 }, "MP3 320"),
                (Format::Mp3 { kbps: 192 }, "MP3 192"),
                (Format::Wav16, "WAV 16"),
                (Format::Wav24, "WAV 24"),
            ];
            if can_render_video {
                options.push((Format::Mp4 { height: 720 }, "MP4 720p"));
                options.push((Format::Mp4 { height: 1080 }, "1080p"));
            }
            segmented(ui, &mut dialog.format, &options);
        });
        ui.add_space(SP_1);
        row(ui, "Range", |ui| {
            let mut options = vec![(RangeChoice::Project, "Whole project"), (RangeChoice::Loop, "Loop")];
            if !self.selection.is_empty() {
                options.push((RangeChoice::Selection, "Selection"));
            }
            segmented(ui, &mut dialog.range, &options);
        });
        ui.add_space(SP_1);
        row(ui, "Tail", |ui| {
            ui.add(ValueField::new("Ring-out", &mut dialog.tail_seconds, 0.0..=10.0).unit(" s").default(1.0));
        });
        ui.add_space(SP_1);
        row(ui, "Title", |ui| {
            text_field(ui, &mut dialog.title, "Title", ui.available_width());
        });
        let (a, b) = self.range_seconds(dialog.range);
        ui.add_space(SP_2);
        let what = if matches!(dialog.format, Format::Mp4 { .. }) { "what you hear, with each clip's picture" } else { "what you hear" };
        ui.label(caption(format!("Renders {} of {what}. Muted tracks stay out.", format_time((b - a).max(0.0)))));
    }
}
