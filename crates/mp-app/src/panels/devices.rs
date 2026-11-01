//! The device chain view: a row of device cards with knobs, like Bitwig's device panel.

use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::picture::ChainMode;
use eframe::egui::{self, Color32, Sense, Shape, Stroke, Ui};
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

    /// The chain's own controls. These live inside the panel, so they can't be mistaken for its tabs.
    fn chain_rail(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        ui.add_space(SP_1);
        ui.label(caption("Chain on"));
        let name = if self.master_selected {
            "Master".to_string()
        } else {
            self.selected_track.and_then(|t| self.project.track(t)).map_or("Choose a track".to_string(), |t| truncate(&t.name, 16))
        };
        let tracks: Vec<(TrackId, String)> = self.project.tracks.iter().map(|t| (t.id, t.name.clone())).collect();
        dropdown(ui, "chain-target", name, width - SP_2, |ui| {
            for (id, name) in &tracks {
                if menu_item(ui, name, None).clicked() {
                    self.selected_track = Some(*id);
                    self.master_selected = false;
                    ui.close();
                }
            }
            menu_separator(ui);
            if menu_item(ui, "Master", None).clicked() {
                self.master_selected = true;
                ui.close();
            }
        });
        ui.add_space(SP_2);
        segmented(ui, &mut self.picture.mode, &[(ChainMode::Sound, "Sound"), (ChainMode::Picture, "Picture")]);
        ui.add_space(SP_2);
        match self.picture.mode {
            ChainMode::Sound => menu_button(ui, "Add device", |ui| {
                for kind in ADDABLE {
                    if menu_item(ui, kind.name(), None).clicked() {
                        self.add_device(kind);
                        ui.close();
                    }
                }
            }),
            ChainMode::Picture => {
                menu_button(ui, "Add effect", |ui| self.add_picture_menu(ui));
                let shown = self.chain_target().and_then(|t| self.project.track(t)).is_none_or(|t| {
                    t.clips.iter().any(|c| self.project.source(c.source).is_some_and(|s| s.has_video))
                });
                ui.add_space(SP_2);
                if !self.project.has_video() {
                    ui.label(caption("Add a video to see these."));
                } else if !shown {
                    ui.label(caption("This track has no video. Master applies to the whole picture."));
                }
            }
        }
    }

    pub fn devices_panel(&mut self, ui: &mut Ui) {
        const RAIL_W: f32 = 148.0;
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let rail = egui::Rect::from_min_max(full.min + egui::vec2(SP_2, 0.0), egui::pos2(full.left() + SP_2 + RAIL_W, full.bottom() - SP_1));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rail).layout(egui::Layout::top_down(egui::Align::Min)));
        self.chain_rail(&mut child);
        let line_x = rail.right() + SP_2;
        ui.painter().line_segment([egui::pos2(line_x, full.top()), egui::pos2(line_x, full.bottom())], Stroke::new(1.0, DIVIDER));
        let content = egui::Rect::from_min_max(egui::pos2(line_x + 1.0, full.top()), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
        let target = self.chain_target();
        if target.is_none() && !self.master_selected {
            child.allocate_rect(content, Sense::hover());
            empty_state(&mut child, content, "No track selected", "Choose a track on the left, or Master.", None);
            return;
        }
        match self.picture.mode {
            ChainMode::Sound => self.sound_chain(&mut child, target),
            ChainMode::Picture => self.picture_chain_panel(&mut child, target),
        }
    }

    fn sound_chain(&mut self, ui: &mut Ui, target: Option<TrackId>) {
        let area = ui.available_rect_before_wrap();
        let Some(chain) = self.project.device_chain(target).cloned() else { return };
        if chain.is_empty() {
            ui.allocate_rect(area, Sense::hover());
            empty_state(ui, area, "No devices yet", "Use Add device on the left to start a chain.", None);
            return;
        }
        let tint = target.and_then(|t| self.project.track(t)).map_or(TEXT_2, |t| track_color(t.color));
        egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(SP_2);
                for (i, device) in chain.iter().enumerate() {
                    self.device_card(ui, target, i, device, chain.len(), tint);
                }
            });
        });
    }

    fn device_card(&mut self, ui: &mut Ui, target: Option<TrackId>, index: usize, device: &DeviceInstance, total: usize, tint: Color32) {
        let params = params_of(&device.kind);
        const PER_ROW: usize = 6;
        const KNOB_W: f32 = 60.0;
        let columns = params.len().clamp(2, PER_ROW);
        let width = columns as f32 * KNOB_W;
        let frame = if device.enabled { card_frame() } else { card_frame().fill(PANEL) };
        frame.show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.x = SP_HALF;
                    if ui.add(IconButton::new(Icon::Power, if device.enabled { "Bypass" } else { "Enable" }).active(device.enabled).size(18.0)).clicked() {
                        self.edit("Toggle device", |p| {
                            if let Some(d) = p.device_chain_mut(target).and_then(|c| c.get_mut(index)) {
                                d.enabled = !d.enabled;
                            }
                        });
                    }
                    ui.label(if device.enabled { strong(device.kind.name()) } else { caption(device.kind.name()) });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(IconButton::new(Icon::Close, "Remove").size(18.0)).clicked() {
                            self.edit("Remove device", |p| {
                                if let Some(c) = p.device_chain_mut(target) {
                                    c.remove(index);
                                }
                            });
                        }
                        if index + 1 < total && ui.add(IconButton::new(Icon::ChevronRight, "Move later in the chain").size(18.0)).clicked() {
                            self.edit("Move device", |p| {
                                if let Some(c) = p.device_chain_mut(target) {
                                    c.swap(index, index + 1);
                                }
                            });
                        }
                        if index > 0 && ui.add(IconButton::new(Icon::ChevronLeft, "Move earlier in the chain").size(18.0)).clicked() {
                            self.edit("Move device", |p| {
                                if let Some(c) = p.device_chain_mut(target) {
                                    c.swap(index, index - 1);
                                }
                            });
                        }
                    });
                });
                if device.kind == DeviceKind::Eq {
                    self.eq_curve(ui, device, width, tint);
                }
                ui.add_space(SP_1);
                egui::Grid::new(("device", device.id.0)).spacing(egui::vec2(SP_1, 0.0)).num_columns(columns).show(ui, |ui| {
                    for (i, info) in params.iter().enumerate() {
                        let value = device.params.get(&info.id).copied().unwrap_or(info.default);
                        let mut norm = info.normalize(value);
                        let center = info.min < 0.0 && info.max > 0.0;
                        let display = info.format(value);
                        let output = knob(ui, &mut norm, info.normalize(info.default), info.name, &display, center, tint);
                        if output.started {
                            self.begin_gesture(&format!("Change {}", info.name));
                        }
                        if output.changed {
                            self.set_device_param(target, device.id, info.id, info.denormalize(norm));
                        }
                        if output.finished {
                            self.end_gesture();
                        }
                        tip(output.response, &format!("{}: {}. Double-click to reset, hold Shift to fine-tune.", info.name, display), None);
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
    fn eq_curve(&self, ui: &mut Ui, device: &DeviceInstance, width: f32, tint: Color32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, R_S, BASE);
        let sr = self.sample_rate as f32;
        let param = |id: u32| {
            let info = params_of(&DeviceKind::Eq).iter().find(|p| p.id == id);
            device.params.get(&id).copied().or(info.map(|p| p.default)).unwrap_or(0.0)
        };
        const RANGE_DB: f32 = 24.0;
        let y_of = |db: f32| rect.center().y - (db / RANGE_DB).clamp(-1.0, 1.0) * rect.height() * 0.48;
        for db in [-12.0, 0.0, 12.0] {
            painter.line_segment([egui::pos2(rect.left(), y_of(db)), egui::pos2(rect.right(), y_of(db))], Stroke::new(1.0, CONTROL));
        }
        for hz in [100.0f32, 1000.0, 10000.0] {
            let x = rect.left() + (hz / 20.0).ln() / (20000f32 / 20.0).ln() * rect.width();
            painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.0, CONTROL));
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
        painter.add(Shape::line(points, Stroke::new(1.5, tint)));
    }
}
