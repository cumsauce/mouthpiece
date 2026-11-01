//! The window: arranges the panels, and handles what applies to the whole frame.

use crate::app::{AfterDiscard, App};
use crate::ds::*;
use eframe::egui::{self, Key, Ui};

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.dropped_files.append(&mut self.script_dropped);
        if let Some((events, modifiers)) = self.inject_frames.pop_front() {
            raw_input.events.push(egui::Event::ModifiersChanged(modifiers));
            raw_input.events.extend(events);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.render_state = frame.wgpu_render_state().cloned();
        self.hint.clear();
        self.frame_start();
        crate::script::tick(self, &ctx);
        self.handle_drops(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_discard = Some(AfterDiscard::Quit);
        }
        if self.any_modal_open() {
            if self.palette_open && ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::K))) {
                self.palette_open = false;
            }
        } else {
            self.trial_keys(&ctx);
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("topbar").frame(bar_frame(RAISED)).exact_size(TOPBAR_H).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").frame(bar_frame(PANEL)).exact_size(STATUSBAR_H).show(ui, |ui| self.status_bar(ui));
        // With nothing in the project there's nothing to edit, so the first screen stays simple.
        if self.show_bottom && !self.project.tracks.is_empty() {
            egui::Panel::bottom("bottom")
                .frame(egui::Frame::NONE.fill(PANEL))
                .resizable(true)
                .default_size(280.0)
                .size_range(150.0..=720.0)
                .show(ui, |ui| self.bottom_panel(ui));
        }
        if self.show_inspector {
            egui::Panel::left("inspector").frame(panel_frame()).resizable(true).default_size(216.0).size_range(190.0..=340.0).show(ui, |ui| self.inspector_panel(ui));
        }
        if self.show_say {
            egui::Panel::right("say").frame(panel_frame()).resizable(true).default_size(310.0).size_range(260.0..=520.0).show(ui, |ui| self.say_panel(ui));
        }
        if self.picture.large && self.show_video && self.project.has_video() {
            let render_state = self.render_state.clone();
            egui::Panel::top("stage").frame(egui::Frame::NONE.fill(BASE)).resizable(true).default_size(340.0).size_range(160.0..=720.0).show(ui, |ui| self.picture_stage(ui, render_state.as_ref()));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BASE)).show(ui, |ui| self.arranger(ui));

        self.selection_bar(&ctx);
        self.modals(&ctx);
        self.drag_ghost(&ctx);
        self.drop_overlay(&ctx);
        self.toasts_overlay(&ctx);

        self.set_title(format!("{}{} — Mouthpiece", self.project.name, if self.is_dirty() { " ●" } else { "" }));

        // Redraw when something on screen is moving. Idle otherwise, to leave the CPU to audio.
        let audition = self.engine.shared.audition_total.load(std::sync::atomic::Ordering::Relaxed) > 0;
        if self.engine.is_playing() || audition || self.master_hold.iter().any(|p| *p > 0.001) || !self.flash.is_empty() {
            ctx.request_repaint();
        } else if !self.jobs.active.is_empty() || self.cache.pending() > 0 || !self.toasts.is_empty() || self.needs_compile {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        if self.script.is_some() {
            ctx.request_repaint();
        }
    }
}

impl App {
    pub fn any_modal_open(&self) -> bool {
        self.palette_open || self.show_shortcuts || self.export_dialog.is_some() || self.pending_discard.is_some() || self.word_dialog.is_some() || self.rename_track.is_some()
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path().to_path_buf();
            if path.extension().is_some_and(|e| e == crate::app::FILE_EXTENSION) {
                self.after_discard(AfterDiscard::OpenPath(path));
            } else {
                self.import_path(path);
            }
        }
    }
}
