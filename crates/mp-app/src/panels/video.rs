//! The picture. There's no separate video editing: each clip shows its own part of its source, so
//! a sentence mix cuts the video by itself.

use crate::app::App;
use crate::ds::*;
use eframe::egui::{self, Rect, Sense, Ui};
use mp_analysis::video::{PROXY_HEIGHT, PROXY_WIDTH};
use mp_model::SourceId;
use std::sync::atomic::Ordering;

const INSTALL: &str = if cfg!(target_os = "macos") { "brew install ffmpeg" } else { "sudo apt install ffmpeg" };

impl App {
    /// What to show now: the word being auditioned if there is one, otherwise the timeline's picture.
    fn current_picture(&self) -> Option<(SourceId, f64)> {
        let shared = &self.engine.shared;
        if shared.audition_total.load(Ordering::Relaxed) > 0 {
            if let Some((source, from)) = self.audition_source {
                let played = shared.audition_played.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
                return Some((source, from + played));
            }
        }
        let clip = self.project.held_picture_clip(self.playhead)?;
        Some((clip.source, clip.source_time_at(self.playhead)))
    }

    pub fn video_preview(&mut self, ui: &mut Ui) {
        if !self.project.has_video() || !self.show_video {
            return;
        }
        if mp_model::video::ffmpeg_path().is_none() {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(strong("Install ffmpeg to see the picture"));
                ui.label(caption("Your edits will then cut the video by themselves. Sound works without it."));
                ui.add_space(SP_1);
                ui.horizontal(|ui| {
                    keycap(ui, INSTALL);
                    if ui.add(Button::secondary("Copy").small()).clicked() {
                        ui.ctx().copy_text(INSTALL.to_string());
                    }
                });
            });
            ui.add_space(SP_2);
            return;
        }
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, width * PROXY_HEIGHT as f32 / PROXY_WIDTH as f32), Sense::hover());
        ui.painter().rect_filled(rect, R_S, BASE);
        if let Some((source, seconds)) = self.current_picture() {
            self.update_video_texture(ui, source, seconds);
        } else {
            self.video_frame = None;
        }
        if let (Some(texture), Some(_)) = (&self.video_texture, self.video_frame) {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture.id(), rect, uv, egui::Color32::WHITE);
        }
        ui.add_space(SP_2);
    }

    fn update_video_texture(&mut self, ui: &Ui, source: SourceId, seconds: f64) {
        let Some(proxy) = self.video.get(&source) else { return };
        let Some(index) = proxy.frame_index(seconds) else { return };
        if self.video_frame == Some((source, index)) {
            return;
        }
        let Some(bytes) = proxy.frame(index) else { return };
        let image = egui::ColorImage::from_rgb([PROXY_WIDTH, PROXY_HEIGHT], bytes);
        match &mut self.video_texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => self.video_texture = Some(ui.ctx().load_texture("video", image, egui::TextureOptions::LINEAR)),
        }
        self.video_frame = Some((source, index));
    }
}
