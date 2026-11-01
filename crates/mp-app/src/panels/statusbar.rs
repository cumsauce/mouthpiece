//! The status bar: what the thing under the pointer does, and what's running.

use crate::app::App;
use crate::ds::*;
use eframe::egui::{self, Ui};

impl App {
    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = SP_2;
            ui.label(caption(self.hint.clone()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let load = self.engine.shared.cpu_load();
                let color = if load > 0.8 { BAD } else if load > 0.5 { WARN } else { TEXT_3 };
                ui.label(num(format!("{:.0}%", load * 100.0)).color(color));
                ui.label(micro("DSP"));
                let jobs: Vec<(u64, String, Option<f32>)> = self.jobs.active.iter().map(|j| (j.id, j.label.clone(), j.fraction)).collect();
                for (id, label, fraction) in jobs {
                    if ui.add(IconButton::new(Icon::Close, "Cancel").size(14.0)).clicked() {
                        if let Some(job) = self.jobs.active.iter().find(|j| j.id == id) {
                            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    progress(ui, fraction, 64.0);
                    ui.label(caption(label));
                }
                let renders = self.cache.pending();
                if renders > 0 {
                    ui.label(caption(format!("Rendering {renders} clip{}", if renders == 1 { "" } else { "s" })));
                }
            });
        });
    }
}
