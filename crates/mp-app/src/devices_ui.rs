//! The device chain view: a row of device cards with knobs, like Bitwig's device panel.

use crate::app::App;
use crate::theme::*;
use crate::widgets::{knob, tiny_icon, Icon};
use eframe::egui::{self, CornerRadius, Sense, Shape, Stroke, Ui};
use mp_dsp::filter;
use mp_engine::devices::{params_of, EQ_BANDS};
use mp_model::{DeviceId, DeviceInstance, DeviceKind, TrackId};

const ADDABLE: [DeviceKind; 8] = [
    DeviceKind::Eq,
    DeviceKind::Compressor,
    DeviceKind::Gate,
    DeviceKind::Saturator,
    DeviceKind::Delay,
    DeviceKind::Reverb,
    DeviceKind::Utility,
    DeviceKind::Limiter,
];

impl App {
    fn chain_target(&self) -> Option<TrackId> {
        if self.master_selected { None } else { self.selected_track }
    }

    pub fn devices_toolbar(&mut self, ui: &mut Ui) {
        let name = if self.master_selected {
            "Master".to_string()
        } else {
            self.selected_track.and_then(|t| self.project.track(t)).map_or("No track selected".to_string(), |t| t.name.clone())
        };
        ui.label(egui::RichText::new(name).strong().color(TEXT));
        ui.add_space(6.0);
        if ui.selectable_label(self.master_selected, "Master").on_hover_text("Show the master chain").clicked() {
            self.master_selected = !self.master_selected;
        }
        ui.add_space(6.0);
        ui.menu_button("+ Add device", |ui| {
            for kind in ADDABLE {
                if ui.button(kind.name()).clicked() {
                    self.add_device(kind);
                    ui.close();
                }
            }
        });
    }

    pub fn devices_panel(&mut self, ui: &mut Ui) {
        let target = self.chain_target();
        if target.is_none() && !self.master_selected {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Select a track to see its devices").color(TEXT_FAINT).size(14.0));
            });
            return;
        }
        let Some(chain) = self.project.device_chain(target).cloned() else { return };
        if chain.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No devices. Use + Add device to start a chain.").color(TEXT_FAINT).size(14.0));
            });
            return;
        }
        egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(4.0);
                for (i, device) in chain.iter().enumerate() {
                    self.device_card(ui, target, i, device, chain.len());
                    ui.add_space(2.0);
                }
            });
        });
    }

    fn device_card(&mut self, ui: &mut Ui, target: Option<TrackId>, index: usize, device: &DeviceInstance, total: usize) {
        let params = params_of(&device.kind);
        let accent = if device.enabled { ACCENT } else { TEXT_FAINT };
        const PER_ROW: usize = 6;
        const KNOB_W: f32 = 64.0;
        let columns = params.len().clamp(2, PER_ROW);
        let width = columns as f32 * KNOB_W;
        egui::Frame::NONE
            .fill(if device.enabled { BG_2 } else { BG_1 })
            .stroke(Stroke::new(1.0, BG_3))
            .corner_radius(CornerRadius::same(7))
            .inner_margin(egui::Margin::same(7))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if tiny_icon(ui, Icon::Power, accent, "Enable or bypass").clicked() {
                            self.edit("Toggle device", |p| {
                                if let Some(d) = p.device_chain_mut(target).and_then(|c| c.get_mut(index)) {
                                    d.enabled = !d.enabled;
                                }
                            });
                        }
                        ui.label(egui::RichText::new(device.kind.name()).strong().color(if device.enabled { TEXT } else { TEXT_DIM }));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if tiny_icon(ui, Icon::Close, TEXT_FAINT, "Remove").clicked() {
                                self.edit("Remove device", |p| {
                                    if let Some(c) = p.device_chain_mut(target) {
                                        c.remove(index);
                                    }
                                });
                            }
                            if index + 1 < total && ui.small_button("›").on_hover_text("Move right").clicked() {
                                self.edit("Move device", |p| {
                                    if let Some(c) = p.device_chain_mut(target) {
                                        c.swap(index, index + 1);
                                    }
                                });
                            }
                            if index > 0 && ui.small_button("‹").on_hover_text("Move left").clicked() {
                                self.edit("Move device", |p| {
                                    if let Some(c) = p.device_chain_mut(target) {
                                        c.swap(index, index - 1);
                                    }
                                });
                            }
                        });
                    });
                    if device.kind == DeviceKind::Eq {
                        self.eq_curve(ui, device, width);
                    }
                    egui::Grid::new(("device", device.id.0)).spacing(egui::vec2(6.0, 0.0)).num_columns(columns).show(ui, |ui| {
                        for (i, info) in params.iter().enumerate() {
                            let value = device.params.get(&info.id).copied().unwrap_or(info.default);
                            let mut norm = info.normalize(value);
                            let center = info.min < 0.0 && info.max > 0.0;
                            let display = info.format(value);
                            let output = knob(ui, &mut norm, info.normalize(info.default), info.name, &display, center);
                            if output.started {
                                self.begin_gesture(&format!("Change {}", info.name));
                            }
                            if output.changed {
                                self.set_device_param(target, device.id, info.id, info.denormalize(norm));
                            }
                            if output.finished {
                                self.end_gesture();
                            }
                            output.response.on_hover_text(format!("{}: {}\nDouble-click to reset. Hold Shift to fine-tune.", info.name, display));
                            if (i + 1) % PER_ROW == 0 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
    }

    fn set_device_param(&mut self, target: Option<TrackId>, id: DeviceId, param: u32, value: f32) {
        if let Some(d) = self.project.device_chain_mut(target).and_then(|c| c.iter_mut().find(|d| d.id == id)) {
            d.params.insert(param, value);
            self.needs_compile = true;
        }
    }

    /// Draws the combined response of all EQ bands.
    fn eq_curve(&self, ui: &mut Ui, device: &DeviceInstance, width: f32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(4), BG_0);
        let sr = self.sample_rate as f32;
        let param = |id: u32| {
            let info = params_of(&DeviceKind::Eq).iter().find(|p| p.id == id);
            device.params.get(&id).copied().or(info.map(|p| p.default)).unwrap_or(0.0)
        };
        const RANGE_DB: f32 = 24.0;
        let y_of = |db: f32| rect.center().y - (db / RANGE_DB).clamp(-1.0, 1.0) * rect.height() * 0.48;
        for db in [-12.0, 0.0, 12.0] {
            painter.line_segment([egui::pos2(rect.left(), y_of(db)), egui::pos2(rect.right(), y_of(db))], Stroke::new(1.0, BG_3));
        }
        for hz in [100.0f32, 1000.0, 10000.0] {
            let x = rect.left() + (hz / 20.0).ln() / (20000f32 / 20.0).ln() * rect.width();
            painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.0, BG_3));
        }
        let points: Vec<egui::Pos2> = (0..=rect.width() as i32)
            .step_by(2)
            .map(|px| {
                let hz = 20.0 * (20000f32 / 20.0).powf(px as f32 / rect.width());
                let db: f32 = EQ_BANDS
                    .iter()
                    .map(|(kind, f, g, q)| {
                        let (freq, gain) = (param(*f), g.map_or(0.0, param));
                        let quality = q.map_or(0.707, param);
                        let neutral = match kind {
                            filter::FilterKind::HighPass => freq <= 20.5,
                            filter::FilterKind::LowPass => freq >= 21500.0,
                            _ => gain.abs() <= 0.01,
                        };
                        if neutral { 0.0 } else { filter::response_db(*kind, freq, quality, gain, sr, hz) }
                    })
                    .sum();
                egui::pos2(rect.left() + px as f32, y_of(db))
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(1.5, ACCENT)));
    }
}
