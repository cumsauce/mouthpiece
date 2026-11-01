//! Effects on the picture. They look and work like audio devices, and any knob can move: with
//! keyframes, on the beat, or following a track. The motion lane lines up with the arrangement above it,
//! so a keyframe sits under the clip it belongs to. Effects can also be dragged on the preview itself.

use crate::app::{App, BottomTab, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui};
use eframe::{egui_wgpu, wgpu};
use mp_analysis::video::{PROXY_HEIGHT, PROXY_WIDTH};
use mp_engine::ParamInfo;
use mp_model::picture::*;
use mp_model::{DeviceId, SourceId, TrackId, TrackKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChainMode {
    #[default]
    Sound,
    Picture,
}

#[derive(Default)]
pub struct PictureState {
    pub mode: ChainMode,
    /// The effect and parameter whose motion is showing.
    pub selected: Option<(DeviceId, u32)>,
    pub selected_key: Option<usize>,
    /// Shows the picture above the arrangement, big enough to work on.
    pub large: bool,
    gpu: Option<Gpu>,
}

struct Gpu {
    renderer: mp_vfx::Renderer,
    texture: egui::TextureId,
    frame: Option<Option<(SourceId, usize)>>,
    drawn: Option<(Option<(SourceId, usize)>, Vec<mp_vfx::Pass>, f32)>,
}

// Effects work at twice the proxy's size, so that blurs and warps stay smooth when the picture is large.
const STAGE_WIDTH: u32 = PROXY_WIDTH as u32 * 2;
const STAGE_HEIGHT: u32 = PROXY_HEIGHT as u32 * 2;
const ASPECT: f32 = PROXY_WIDTH as f32 / PROXY_HEIGHT as f32;
const MOTION_H: f32 = 84.0;
const KEY_HIT: f32 = 8.0;

const BEATS: [(f32, &str); 6] = [(0.25, "Every 1/4 beat"), (0.5, "Every 1/2 beat"), (1.0, "Every beat"), (2.0, "Every 2 beats"), (4.0, "Every 4 beats"), (8.0, "Every 8 beats")];
const SHAPES: [(PulseShape, &str); 4] = [(PulseShape::Decay, "Hit"), (PulseShape::Sine, "Wave"), (PulseShape::Saw, "Ramp"), (PulseShape::Square, "On/off")];

/// Starting points that show what motion can do, and are often all that's needed.
#[derive(Clone, Copy)]
pub enum Recipe {
    ZoomPunch,
    SlowZoom,
    ShakeOnBar,
    FlashWithDrums,
    SplitOnBeat,
    Spotlight,
}

const RECIPES: [(Recipe, &str); 6] = [
    (Recipe::ZoomPunch, "Zoom punch on the beat"),
    (Recipe::SlowZoom, "Slow zoom in from the playhead"),
    (Recipe::ShakeOnBar, "Shake on every bar"),
    (Recipe::FlashWithDrums, "Flash with the drums"),
    (Recipe::SplitOnBeat, "RGB split on the beat"),
    (Recipe::Spotlight, "Black and white, except an area"),
];

fn key_diamond(c: Pos2, r: f32) -> Vec<Pos2> {
    vec![c + egui::vec2(0.0, -r), c + egui::vec2(r, 0.0), c + egui::vec2(0.0, r), c + egui::vec2(-r, 0.0)]
}

impl App {
    pub fn add_picture_menu(&mut self, ui: &mut Ui) {
        for (recipe, name) in RECIPES {
            if menu_item(ui, name, None).clicked() {
                self.add_recipe(recipe);
                ui.close();
            }
        }
        menu_separator(ui);
        for kind in PictureKind::ALL {
            if menu_item(ui, kind.name(), None).clicked() {
                self.add_picture_effect(kind);
                ui.close();
            }
        }
    }

    fn add_picture_effect(&mut self, kind: PictureKind) -> Option<DeviceId> {
        let target = if self.master_selected { None } else { self.selected_track };
        if target.is_none() && !self.master_selected {
            self.toast(ToastKind::Info, "Select a track first, or Master");
            return None;
        }
        let id = self.edit(&format!("Add {}", kind.name()), |p| p.add_picture_effect(target, kind));
        self.picture.selected = mp_vfx::params_of(kind).first().map(|info| (id, info.id));
        self.picture.selected_key = None;
        self.picture.mode = ChainMode::Picture;
        self.bottom_tab = BottomTab::Devices;
        self.show_bottom = true;
        Some(id)
    }

    /// For the test script.
    pub fn add_recipe_number(&mut self, number: usize) {
        if let Some((recipe, _)) = RECIPES.get(number) {
            self.add_recipe(*recipe);
        }
    }

    fn add_recipe(&mut self, recipe: Recipe) {
        let (kind, param) = match recipe {
            Recipe::ZoomPunch | Recipe::SlowZoom => (PictureKind::Transform, 2),
            Recipe::ShakeOnBar => (PictureKind::Shake, 0),
            Recipe::FlashWithDrums => (PictureKind::Flash, 0),
            Recipe::SplitOnBeat => (PictureKind::RgbSplit, 0),
            Recipe::Spotlight => (PictureKind::Color, 2),
        };
        let Some(id) = self.add_picture_effect(kind) else { return };
        let beat = |every, amount| Some(BeatMotion { every, shape: PulseShape::Decay, amount });
        let start = self.playhead;
        let bars = self.project.beat_seconds() * self.project.beats_per_bar as f64 * 4.0;
        let drums = self.project.tracks.iter().find(|t| t.kind == TrackKind::Drums).map(|t| t.id);
        let no_drums = recipe_needs_drums(recipe) && drums.is_none();
        let Some(effect) = self.project.picture_effect_mut(id) else { return };
        let mut motion = Motion::default();
        match recipe {
            Recipe::ZoomPunch => motion.beat = beat(1.0, 0.25),
            Recipe::SlowZoom => {
                motion.set_key(start, 1.0);
                motion.set_key(start + bars, 1.4);
            }
            Recipe::ShakeOnBar => {
                effect.params.insert(0, 0.0);
                motion.beat = beat(4.0, 0.06);
            }
            Recipe::FlashWithDrums => match drums {
                Some(track) => motion.follow = Some(FollowMotion { track, amount: 0.7 }),
                None => motion.beat = beat(1.0, 0.6),
            },
            Recipe::SplitOnBeat => {
                effect.params.insert(0, 0.0);
                motion.beat = beat(1.0, 0.03);
            }
            Recipe::Spotlight => {
                effect.params.insert(2, 0.0);
                effect.area = true;
                effect.area_invert = true;
            }
        }
        if !motion.is_empty() {
            effect.motion.insert(param, motion);
        }
        self.picture.selected = Some((id, param));
        if no_drums {
            self.toast(ToastKind::Info, "No drum track, so this flashes on the beat instead");
        }
    }

    /// What a parameter is set to at the playhead, without beat or follow motion on top.
    fn set_value_of(&self, effect: &PictureEffect, info: &ParamInfo) -> f32 {
        keyed_value(effect, info, self.playhead)
    }

    fn set_picture_value(&mut self, id: DeviceId, param: u32, value: f32) {
        let at = self.playhead;
        if let Some(effect) = self.project.picture_effect_mut(id) {
            if let Some(info) = mp_vfx::param_info(effect.kind, param) {
                effect.set_value(param, value.clamp(info.min, info.max), at);
            }
        }
    }

    pub fn picture_chain_panel(&mut self, ui: &mut Ui, target: Option<TrackId>) {
        let area = ui.available_rect_before_wrap();
        let Some(chain) = self.project.picture_chain(target).cloned() else { return };
        if chain.is_empty() {
            ui.allocate_rect(area, Sense::hover());
            empty_state(ui, area, "No picture effects yet", "Use Add effect on the left. It starts with ready-made ideas like a zoom on the beat.", None);
            return;
        }
        let tint = target.and_then(|t| self.project.track(t)).map_or(TEXT_2, |t| track_color(t.color));
        let selected = self.picture.selected.filter(|(id, _)| chain.iter().any(|e| e.id == *id));
        let bar = selected.map_or(0.0, |_| MOTION_H);
        let cards = Rect::from_min_max(area.min, egui::pos2(area.right(), area.bottom() - bar));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cards));
        egui::ScrollArea::both().id_salt("picture-chain").auto_shrink(false).show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                for (i, effect) in chain.iter().enumerate() {
                    self.picture_card(ui, target, i, effect, chain.len(), tint);
                }
            });
        });
        ui.allocate_rect(area, Sense::hover());
        if let Some((id, param)) = selected {
            let lane = Rect::from_min_max(egui::pos2(area.left(), area.bottom() - bar), area.max);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(lane));
            self.motion_bar(&mut child, id, param, tint);
        }
    }

    fn picture_card(&mut self, ui: &mut Ui, target: Option<TrackId>, index: usize, effect: &PictureEffect, total: usize, tint: Color32) {
        const KNOB_W: f32 = 60.0;
        const PER_ROW: usize = 5;
        let params = mp_vfx::params_of(effect.kind);
        let own = params.len().clamp(3, PER_ROW) as f32 * KNOB_W;
        // The area's knobs go alongside the effect's own, to keep cards one row tall.
        let width = own + if effect.area { mp_vfx::AREA_PARAMS.len() as f32 * KNOB_W + SP_3 } else { 0.0 };
        let frame = if effect.enabled { card_frame() } else { card_frame().fill(PANEL) };
        frame.show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.x = SP_HALF;
                    let id = effect.id;
                    if ui.add(IconButton::new(Icon::Power, if effect.enabled { "Bypass" } else { "Enable" }).active(effect.enabled).size(18.0)).clicked() {
                        self.edit("Toggle effect", |p| {
                            if let Some(e) = p.picture_effect_mut(id) {
                                e.enabled = !e.enabled;
                            }
                        });
                    }
                    ui.label(if effect.enabled { strong(effect.kind.name()) } else { caption(effect.kind.name()) });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(IconButton::new(Icon::Close, "Remove").size(18.0)).clicked() {
                            self.edit("Remove effect", |p| {
                                if let Some(c) = p.picture_chain_mut(target) {
                                    c.remove(index);
                                }
                            });
                        }
                        if index + 1 < total && ui.add(IconButton::new(Icon::ChevronRight, "Move later in the chain").size(18.0)).clicked() {
                            self.edit("Move effect", |p| {
                                if let Some(c) = p.picture_chain_mut(target) {
                                    c.swap(index, index + 1);
                                }
                            });
                        }
                        if index > 0 && ui.add(IconButton::new(Icon::ChevronLeft, "Move earlier in the chain").size(18.0)).clicked() {
                            self.edit("Move effect", |p| {
                                if let Some(c) = p.picture_chain_mut(target) {
                                    c.swap(index, index - 1);
                                }
                            });
                        }
                        if effect.area {
                            if ui.add(Button::ghost("Outside").small().active(effect.area_invert).tip("Apply outside the area instead", None)).clicked() {
                                self.edit("Invert area", |p| {
                                    if let Some(e) = p.picture_effect_mut(id) {
                                        e.area_invert = !e.area_invert;
                                    }
                                });
                            }
                            if ui.add(Button::ghost("Ellipse").small().active(effect.area_ellipse).tip("An ellipse, or a rectangle when off", None)).clicked() {
                                self.edit("Change area shape", |p| {
                                    if let Some(e) = p.picture_effect_mut(id) {
                                        e.area_ellipse = !e.area_ellipse;
                                    }
                                });
                            }
                        }
                        if ui.add(Button::ghost("Area").small().active(effect.area).tip("Limit this effect to part of the picture", None)).clicked() {
                            self.edit("Limit to an area", |p| {
                                if let Some(e) = p.picture_effect_mut(id) {
                                    e.area = !e.area;
                                }
                            });
                            self.picture.selected = Some((id, AREA_X));
                        }
                    });
                });
                card_rule(ui);
                ui.horizontal_top(|ui| {
                    self.knob_grid(ui, effect, params, PER_ROW, tint, "own");
                    if effect.area {
                        ui.add_space(SP_2);
                        self.knob_grid(ui, effect, mp_vfx::AREA_PARAMS, PER_ROW, tint, "area");
                    }
                });
            });
        });
    }

    fn knob_grid(&mut self, ui: &mut Ui, effect: &PictureEffect, params: &[ParamInfo], per_row: usize, tint: Color32, salt: &str) {
        egui::Grid::new(("picture", effect.id.0, salt)).spacing(egui::vec2(SP_1, 0.0)).show(ui, |ui| {
            for (i, info) in params.iter().enumerate() {
                self.picture_knob(ui, effect, info, tint);
                if (i + 1) % per_row == 0 {
                    ui.end_row();
                }
            }
        });
    }

    fn picture_knob(&mut self, ui: &mut Ui, effect: &PictureEffect, info: &ParamInfo, tint: Color32) {
        let value = self.set_value_of(effect, info);
        let mut norm = info.normalize(value);
        let center = info.min < 0.0 && info.max > 0.0;
        let display = info.format(value);
        let output = knob(ui, &mut norm, info.normalize(info.default), info.name, &display, center, tint);
        let rect = output.response.rect;
        if output.started || output.response.clicked() {
            self.picture.selected = Some((effect.id, info.id));
            self.picture.selected_key = None;
        }
        if output.started {
            self.begin_gesture(&format!("Change {}", info.name));
        }
        if output.changed {
            self.set_picture_value(effect.id, info.id, info.denormalize(norm));
        }
        if output.finished {
            self.end_gesture();
        }
        if self.picture.selected == Some((effect.id, info.id)) {
            ui.painter().rect_stroke(rect, R_S, Stroke::new(1.0, TEXT_3), egui::StrokeKind::Inside);
        }
        if effect.motion.get(&info.id).is_some_and(|m| !m.is_empty()) {
            ui.painter().add(Shape::convex_polygon(key_diamond(rect.right_top() + egui::vec2(-8.0, 8.0), 3.5), tint, Stroke::NONE));
        }
        tip(output.response, &format!("{}: {display}. Click to give it motion. Double-click to reset, hold Shift to fine-tune.", info.name), None);
    }

    /// Controls for one parameter's motion, and its keyframes in line with the arrangement.
    fn motion_bar(&mut self, ui: &mut Ui, id: DeviceId, param: u32, tint: Color32) {
        let Some(effect) = self.project.picture_effect(id).cloned() else { return };
        let Some(info) = mp_vfx::param_info(effect.kind, param).copied() else { return };
        let motion = effect.motion.get(&param).cloned().unwrap_or_default();
        let span = (info.max - info.min) as f64;
        let area = ui.available_rect_before_wrap();
        ui.painter().line_segment([area.left_top(), area.right_top()], Stroke::new(1.0, DIVIDER));
        ui.add_space(SP_1);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_1;
            ui.label(strong(format!("{}: {}", effect.kind.name(), info.name)));
            let current = self.set_value_of(&effect, &info);
            if ui.add(IconButton::new(Icon::Keyframe, "Add a keyframe at the playhead").active(!motion.keys.is_empty()).small()).clicked() {
                let at = self.playhead;
                self.edit_motion(id, param, "Add keyframe", |m| {
                    m.set_key(at, current);
                });
            }
            ui.add_space(SP_2);
            ui.label(caption("Beat"));
            let beat_name = motion.beat.and_then(|b| BEATS.iter().find(|(e, _)| *e == b.every)).map_or("Off", |(_, n)| n);
            dropdown(ui, "motion-beat", beat_name, 104.0, |ui| {
                if menu_item(ui, "Off", None).clicked() {
                    self.edit_motion(id, param, "Change beat motion", |m| m.beat = None);
                    ui.close();
                }
                for (every, name) in BEATS {
                    if menu_item(ui, name, None).clicked() {
                        let amount = motion.beat.map_or((span * 0.25) as f32, |b| b.amount);
                        let shape = motion.beat.map_or(PulseShape::Decay, |b| b.shape);
                        self.edit_motion(id, param, "Change beat motion", |m| m.beat = Some(BeatMotion { every, shape, amount }));
                        ui.close();
                    }
                }
            });
            if let Some(beat) = motion.beat {
                let shape_name = SHAPES.iter().find(|(s, _)| *s == beat.shape).map_or("", |(_, n)| n);
                dropdown(ui, "motion-shape", shape_name, 64.0, |ui| {
                    for (shape, name) in SHAPES {
                        if menu_item(ui, name, None).clicked() {
                            self.edit_motion(id, param, "Change beat motion", |m| m.beat = Some(BeatMotion { shape, ..beat }));
                            ui.close();
                        }
                    }
                });
                let mut amount = beat.amount as f64;
                if ui.add(ValueField::new("Amount", &mut amount, -span..=span).decimals(2).bipolar().width(104.0)).changed() {
                    self.begin_gesture("Change beat motion");
                    self.edit_motion_live(id, param, |m| m.beat = Some(BeatMotion { amount: amount as f32, ..beat }));
                }
            }
            ui.add_space(SP_2);
            ui.label(caption("Follow"));
            let follow_name = motion.follow.and_then(|f| self.project.track(f.track)).map_or("Off".to_string(), |t| truncate(&t.name, 14));
            let tracks: Vec<(TrackId, String)> = self.project.tracks.iter().map(|t| (t.id, t.name.clone())).collect();
            dropdown(ui, "motion-follow", follow_name, 96.0, |ui| {
                if menu_item(ui, "Off", None).clicked() {
                    self.edit_motion(id, param, "Change follow", |m| m.follow = None);
                    ui.close();
                }
                for (track, name) in &tracks {
                    if menu_item(ui, name, None).clicked() {
                        let amount = motion.follow.map_or((span * 0.5) as f32, |f| f.amount);
                        let track = *track;
                        self.edit_motion(id, param, "Change follow", |m| m.follow = Some(FollowMotion { track, amount }));
                        ui.close();
                    }
                }
            });
            if let Some(follow) = motion.follow {
                let mut amount = follow.amount as f64;
                if ui.add(ValueField::new("Amount", &mut amount, -span..=span).decimals(2).bipolar().width(104.0)).changed() {
                    self.begin_gesture("Change follow");
                    self.edit_motion_live(id, param, |m| m.follow = Some(FollowMotion { amount: amount as f32, ..follow }));
                }
            }
            if !motion.is_empty() && ui.add(Button::ghost("Clear").small().tip("Remove all motion from this knob", None)).clicked() {
                self.edit_motion(id, param, "Clear motion", |m| *m = Motion::default());
                self.picture.selected_key = None;
            }
        });
        ui.add_space(SP_1);
        let lane = Rect::from_min_max(egui::pos2(area.left(), ui.cursor().top()), egui::pos2(area.right(), area.bottom() - SP_1));
        self.key_lane(ui, lane, id, &effect, &info, tint);
    }

    fn edit_motion(&mut self, id: DeviceId, param: u32, label: &str, f: impl FnOnce(&mut Motion)) {
        self.edit(label, |p| {
            if let Some(effect) = p.picture_effect_mut(id) {
                let motion = effect.motion.entry(param).or_default();
                f(motion);
                if motion.is_empty() {
                    effect.motion.remove(&param);
                }
            }
        });
    }

    /// For use during a gesture, which makes one undo step of the whole drag.
    fn edit_motion_live(&mut self, id: DeviceId, param: u32, f: impl FnOnce(&mut Motion)) {
        if let Some(effect) = self.project.picture_effect_mut(id) {
            f(effect.motion.entry(param).or_default());
        }
    }

    fn key_lane(&mut self, ui: &mut Ui, lane: Rect, id: DeviceId, effect: &PictureEffect, info: &ParamInfo, tint: Color32) {
        let param = info.id;
        let view = self.arr_view.clone();
        let response = ui.interact(lane, ui.scope_id().with("key-lane"), Sense::click_and_drag());
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 0.0, BASE);
        crate::panels::timeline::draw_grid(&painter, lane, &self.project, &view);
        let y_of = |value: f32| lane.bottom() - 3.0 - info.normalize(value) * (lane.height() - 6.0);
        let value_at_y = |y: f32| info.denormalize(((lane.bottom() - 3.0 - y) / (lane.height() - 6.0)).clamp(0.0, 1.0));
        let x_key = |key: &Key| view.x(key.t);

        let level = |track: TrackId, t: f64| self.level_of(track, t);
        let curve: Vec<Pos2> = (0..=lane.width() as i32)
            .step_by(2)
            .map(|px| {
                let x = lane.left() + px as f32;
                let value = mp_vfx::value_at(effect, info, &self.project, view.t(x), &level);
                egui::pos2(x, y_of(value))
            })
            .collect();
        painter.add(Shape::line(curve, Stroke::new(1.5, tint)));

        let keys = effect.motion.get(&param).map(|m| m.keys.clone()).unwrap_or_default();
        let key_at = |p: Pos2| {
            keys.iter()
                .enumerate()
                .map(|(i, k)| (i, (egui::pos2(x_key(k), y_of(k.value)) - p).length()))
                .filter(|(_, d)| *d <= KEY_HIT)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        for (i, key) in keys.iter().enumerate() {
            let selected = self.picture.selected_key == Some(i);
            let c = egui::pos2(x_key(key), y_of(key.value));
            painter.add(Shape::convex_polygon(key_diamond(c, if selected { 6.0 } else { 4.5 }), if selected { TEXT } else { tint }, Stroke::new(1.0, BASE)));
        }
        let motion_empty = effect.motion.get(&param).is_none_or(Motion::is_empty);
        if motion_empty {
            painter.text(egui::pos2(lane.left() + SP_2, lane.top() + SP_2), egui::Align2::LEFT_TOP, "Double-click to add a keyframe, or choose Beat or Follow above", font_caption(), TEXT_3);
        }
        let x = view.x(self.playhead);
        painter.line_segment([egui::pos2(x, lane.top()), egui::pos2(x, lane.bottom())], Stroke::new(1.0, ACCENT));

        crate::panels::timeline::middle_drag_nav(ui, &mut self.arr_view, &response);
        let cmd = ui.input(|i| i.modifiers.command);
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(i) = ui.input(|i| i.pointer.press_origin()).and_then(key_at) {
                self.picture.selected_key = Some(i);
                self.begin_gesture("Move keyframe");
            } else {
                self.picture.selected_key = None;
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            if let (Some(i), Some(p)) = (self.picture.selected_key, response.interact_pointer_pos()) {
                let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                self.snap_line = None;
                let value = value_at_y(p.y);
                if let Some(motion) = self.project.picture_effect_mut(id).and_then(|e| e.motion.get_mut(&param)) {
                    self.picture.selected_key = Some(motion.move_key(i, t, value));
                }
            }
        }
        if response.double_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                match key_at(p) {
                    Some(i) => {
                        self.edit_motion(id, param, "Delete keyframe", |m| {
                            m.keys.remove(i);
                        });
                        self.picture.selected_key = None;
                    }
                    None => {
                        let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                        self.snap_line = None;
                        let value = value_at_y(p.y);
                        self.edit_motion(id, param, "Add keyframe", |m| {
                            m.set_key(t, value);
                        });
                    }
                }
            }
        } else if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                self.picture.selected_key = key_at(p);
                if self.picture.selected_key.is_none() {
                    let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                    self.snap_line = None;
                    self.seek(t);
                }
            }
        }
        if response.secondary_clicked() {
            self.picture.selected_key = response.interact_pointer_pos().and_then(key_at);
        }
        let selected_key = self.picture.selected_key;
        response.context_menu(|ui| {
            let Some(i) = selected_key.filter(|i| *i < keys.len()) else {
                ui.close();
                return;
            };
            for (ease, name) in [(Ease::Smooth, "Smooth"), (Ease::Linear, "Straight"), (Ease::Hold, "Hold until the next")] {
                let mut current = keys[i].ease;
                if menu_choice(ui, &mut current, ease, name).clicked() {
                    self.edit_motion(id, param, "Change keyframe", |m| m.keys[i].ease = ease);
                    ui.close();
                }
            }
            menu_separator(ui);
            if menu_item(ui, "Delete keyframe", None).clicked() {
                self.delete_selected_key();
                ui.close();
            }
        });
    }

    /// Deletes the keyframe selected in the motion lane. Returns whether there was one.
    pub fn delete_selected_key(&mut self) -> bool {
        let (Some((id, param)), Some(i)) = (self.picture.selected, self.picture.selected_key) else { return false };
        if self.bottom_tab != BottomTab::Devices || self.picture.mode != ChainMode::Picture {
            return false;
        }
        self.edit_motion(id, param, "Delete keyframe", |m| {
            if i < m.keys.len() {
                m.keys.remove(i);
            }
        });
        self.picture.selected_key = None;
        true
    }

    pub fn level_of(&self, track: TrackId, t: f64) -> f32 {
        let peaks = |source: SourceId| self.runtime.get(&source).map(|r| r.peaks.as_ref());
        mp_vfx::track_level(&self.project, track, t, &peaks, self.sample_rate as f64)
    }

    /// Draws the picture with its effects into `rect`.
    pub fn draw_picture(&mut self, ui: &mut Ui, rect: Rect, render_state: Option<&egui_wgpu::RenderState>) {
        ui.painter().rect_filled(rect, R_S, BASE);
        let picture = self.current_picture();
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let Some(state) = render_state else {
            // The window isn't on the GPU, which leaves the plain picture.
            if let Some((source, seconds)) = picture {
                self.update_video_texture(ui, source, seconds);
                if let Some(texture) = &self.video_texture {
                    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                }
            }
            return;
        };
        if self.picture.gpu.is_none() {
            let renderer = mp_vfx::Renderer::new(state.device.clone(), state.queue.clone(), STAGE_WIDTH, STAGE_HEIGHT);
            let texture = state.renderer.write().register_native_texture(&state.device, &renderer.display_view(), wgpu::FilterMode::Linear);
            self.picture.gpu = Some(Gpu { renderer, texture, frame: None, drawn: None });
        }
        let frame = picture.and_then(|(source, seconds)| Some((source, self.video.get(&source)?.frame_index(seconds)?)));
        // A word being auditioned is shown plain, since it isn't where the effects are.
        let auditioning = self.engine.shared.audition_total.load(std::sync::atomic::Ordering::Relaxed) > 0;
        let passes = if auditioning {
            Vec::new()
        } else {
            let level = |track: TrackId, t: f64| self.level_of(track, t);
            mp_vfx::resolve_frame(&self.project, self.playhead, &level)
        };
        let time = self.playhead as f32;
        let bytes = frame.and_then(|(source, index)| self.video.get(&source)?.frame(index));
        let Some(gpu) = &mut self.picture.gpu else { return };
        if gpu.frame != Some(frame) {
            let rgba: Vec<u8> = match bytes {
                Some(rgb) => rgb.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
                None => [0, 0, 0, 255].repeat(PROXY_WIDTH * PROXY_HEIGHT),
            };
            gpu.renderer.upload(&rgba, PROXY_WIDTH as u32, PROXY_HEIGHT as u32);
            gpu.frame = Some(frame);
        }
        let wanted = (frame, passes, time);
        if gpu.drawn.as_ref() != Some(&wanted) {
            gpu.renderer.render(&wanted.1, time, ASPECT);
            gpu.drawn = Some(wanted);
        }
        ui.painter().image(gpu.texture, rect, uv, Color32::WHITE);
    }

    /// Handles on the picture for the selected effect: drag to place it, or use its corner to size it.
    pub fn picture_handles(&mut self, ui: &mut Ui, rect: Rect) {
        if !self.show_bottom || self.bottom_tab != BottomTab::Devices || self.picture.mode != ChainMode::Picture {
            return;
        }
        let Some((id, _)) = self.picture.selected else { return };
        let Some(effect) = self.project.picture_effect(id).cloned() else { return };
        let now = self.playhead;
        let value = |param: u32| mp_vfx::param_info(effect.kind, param).map_or(0.0, |info| keyed_value(&effect, info, now));
        let size = rect.size();
        let at = |u: f32, v: f32| rect.min + egui::vec2(u * size.x, v * size.y);
        let painter = ui.painter_at(rect);
        let line = Stroke::new(1.0, TEXT);
        // Where it is, and where its size handle goes.
        let (centre, corner);
        if effect.area {
            let (x, y, w, h) = (value(AREA_X), value(AREA_Y), value(AREA_W), value(AREA_H));
            centre = at(x, y);
            corner = at(x + w * 0.5, y + h * 0.5);
            let half = corner - centre;
            if effect.area_ellipse {
                let outline = (0..=48).map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    centre + egui::vec2(a.cos() * half.x, a.sin() * half.y)
                });
                painter.add(Shape::line(outline.collect(), line));
            } else {
                painter.rect_stroke(Rect::from_center_size(centre, half * 2.0), 0.0, line, egui::StrokeKind::Middle);
            }
        } else if effect.kind == PictureKind::Warp {
            let radius = value(2) * size.y;
            centre = at(value(0), value(1));
            corner = centre + egui::vec2(radius, 0.0);
            painter.circle_stroke(centre, radius, line);
        } else if effect.kind == PictureKind::Transform {
            centre = at(0.5 + value(0) * 0.5, 0.5 - value(1) * 0.5);
            let half = size * 0.5 * value(2);
            corner = centre + half;
            painter.rect_stroke(Rect::from_center_size(centre, half * 2.0), 0.0, line, egui::StrokeKind::Middle);
        } else {
            return;
        }

        let body = ui.interact(rect, ui.scope_id().with("picture-body"), Sense::drag());
        let grip = Rect::from_center_size(corner, egui::vec2(12.0, 12.0));
        let handle = ui.interact(grip, ui.scope_id().with("picture-grip"), Sense::drag());
        painter.rect_filled(Rect::from_center_size(corner, egui::vec2(8.0, 8.0)), 1.0, TEXT);
        if handle.hovered() || handle.dragged_by(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        } else if body.hovered() || body.dragged_by(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if body.drag_started_by(egui::PointerButton::Primary) || handle.drag_started_by(egui::PointerButton::Primary) {
            self.begin_gesture("Move on the picture");
        }
        if body.dragged_by(egui::PointerButton::Primary) {
            let d = body.drag_delta();
            let (du, dv) = (d.x / size.x, d.y / size.y);
            if effect.area {
                self.set_picture_value(id, AREA_X, value(AREA_X) + du);
                self.set_picture_value(id, AREA_Y, value(AREA_Y) + dv);
            } else if effect.kind == PictureKind::Warp {
                self.set_picture_value(id, 0, value(0) + du);
                self.set_picture_value(id, 1, value(1) + dv);
            } else {
                self.set_picture_value(id, 0, value(0) + du * 2.0);
                self.set_picture_value(id, 1, value(1) - dv * 2.0);
            }
        }
        if handle.dragged_by(egui::PointerButton::Primary) {
            if let Some(p) = handle.interact_pointer_pos() {
                let offset = p - centre;
                if effect.area {
                    self.set_picture_value(id, AREA_W, offset.x.abs() * 2.0 / size.x);
                    self.set_picture_value(id, AREA_H, offset.y.abs() * 2.0 / size.y);
                } else if effect.kind == PictureKind::Warp {
                    self.set_picture_value(id, 2, offset.length() / size.y);
                } else {
                    self.set_picture_value(id, 2, offset.x.abs() * 2.0 / size.x);
                }
            }
        }
    }

    /// The picture, large, above the arrangement.
    pub fn picture_stage(&mut self, ui: &mut Ui, render_state: Option<&egui_wgpu::RenderState>) {
        let area = ui.available_rect_before_wrap().shrink(SP_2);
        ui.allocate_rect(area, Sense::hover());
        let height = area.height().min(area.width() / ASPECT);
        let rect = Rect::from_center_size(area.center(), egui::vec2(height * ASPECT, height));
        self.draw_picture(ui, rect, render_state);
        self.picture_handles(ui, rect);
        self.large_toggle(ui, rect);
    }

    pub fn large_toggle(&mut self, ui: &mut Ui, rect: Rect) {
        const SIZE: f32 = 24.0;
        let spot = Rect::from_min_size(rect.right_top() + egui::vec2(-SIZE - SP_1, SP_1), egui::vec2(SIZE, SIZE));
        let text = if self.picture.large { "Back to small" } else { "Make the picture large" };
        // In a child of its own, since placing it directly would move the layout's cursor back up here.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(spot));
        if child.add(IconButton::new(Icon::Expand, text).active(self.picture.large).size(SIZE)).clicked() {
            self.picture.large = !self.picture.large;
        }
    }
}

fn keyed_value(effect: &PictureEffect, info: &ParamInfo, t: f64) -> f32 {
    let base = effect.params.get(&info.id).copied().unwrap_or(info.default);
    effect.motion.get(&info.id).map_or(base, |m| m.keyed(base, t)).clamp(info.min, info.max)
}

fn recipe_needs_drums(recipe: Recipe) -> bool {
    matches!(recipe, Recipe::FlashWithDrums)
}
