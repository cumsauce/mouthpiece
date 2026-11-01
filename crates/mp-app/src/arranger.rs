//! The composition view: track headers, clip lanes, ruler and overview.

use crate::app::{App, BottomTab};
use crate::lane::{clip_rect, hit_zone, paint_playhead, paint_snap_line, PaintOptions, Zone};
use crate::theme::*;
use crate::timeline::{draw_grid, draw_ruler};
use crate::widgets::{self, fader, letter_toggle, meter};
use eframe::egui::{self, Align2, CornerRadius, CursorIcon, Key, Rect, Sense, Stroke, StrokeKind, Ui};
use mp_model::{ClipId, TrackId};

const HEADER_W: f32 = 196.0;
const RULER_H: f32 = 28.0;
const OVERVIEW_H: f32 = 22.0;
const MIN_TRACK_H: f32 = 44.0;
const MAX_TRACK_H: f32 = 320.0;

impl App {
    pub fn arranger(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, Sense::hover());
        let ruler = Rect::from_min_max(egui::pos2(area.left() + HEADER_W, area.top()), egui::pos2(area.right(), area.top() + RULER_H));
        let overview = Rect::from_min_max(egui::pos2(area.left() + HEADER_W, area.bottom() - OVERVIEW_H), area.right_bottom());
        let lanes = Rect::from_min_max(egui::pos2(ruler.left(), ruler.bottom()), egui::pos2(area.right(), overview.top()));
        let headers = Rect::from_min_max(egui::pos2(area.left(), ruler.bottom()), egui::pos2(ruler.left(), overview.top()));

        self.arr_view.left = lanes.left();
        self.arr_view.width = lanes.width();
        if self.engine.is_playing() && self.clip_drag.is_none() {
            self.arr_view.follow(self.playhead);
        }
        ui.painter().rect_filled(lanes, 0.0, BG_0);

        self.arranger_scroll(ui, lanes.union(headers).union(ruler));
        self.clamp_track_scroll(lanes.height());
        draw_grid(ui.painter(), lanes, &self.project, &self.arr_view);
        self.track_lanes(ui, lanes);
        self.track_headers(ui, headers, lanes);
        self.ruler_ui(ui, ruler);
        self.overview(ui, overview);

        // Top-left corner: project time, and the Add Track button.
        let corner = Rect::from_min_max(area.min, egui::pos2(ruler.left(), ruler.bottom()));
        ui.painter().rect_filled(corner, 0.0, BG_2);
        let foot = Rect::from_min_max(egui::pos2(area.left(), overview.top()), egui::pos2(overview.left(), area.bottom()));
        ui.painter().rect_filled(foot, 0.0, BG_2);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(corner.shrink2(egui::vec2(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if widgets::icon_button(&mut child, widgets::Icon::Plus, &widgets::keys("Add track (⌘T)")).clicked() {
            self.run(crate::actions::Action::AddTrack);
        }
        child.label(egui::RichText::new(format!("{} tracks", self.project.tracks.len())).color(TEXT_FAINT).size(11.0));

        if self.project.tracks.is_empty() {
            self.empty_state(ui, lanes.union(headers));
        }
        self.drop_word_on_lanes(ui, lanes);
    }

    fn arranger_scroll(&mut self, ui: &mut Ui, area: Rect) {
        let hovered = ui.rect_contains_pointer(area);
        if !hovered {
            return;
        }
        let (scroll, zoom, cmd, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.pointer.hover_pos()));
        let x = pointer.map_or(self.arr_view.left, |p| p.x);
        if zoom != 1.0 {
            self.arr_view.zoom_at(zoom as f64, x);
        } else if cmd && scroll.y != 0.0 {
            self.arr_view.zoom_at((scroll.y as f64 * 0.005).exp(), x);
        } else {
            if scroll.x != 0.0 {
                self.arr_view.pan(-scroll.x);
            }
            self.arr_scroll_y -= scroll.y;
        }
    }

    fn clamp_track_scroll(&mut self, visible: f32) {
        let total: f32 = self.project.tracks.iter().map(|t| t.height + 1.0).sum();
        self.arr_scroll_y = self.arr_scroll_y.clamp(0.0, (total + 60.0 - visible).max(0.0));
    }

    /// Screen rect of each track's lane in `area`.
    fn lane_rects(&self, area: Rect) -> Vec<(usize, Rect)> {
        let mut y = area.top() - self.arr_scroll_y;
        self.project
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let r = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(area.width(), t.height));
                y += t.height + 1.0;
                (i, r)
            })
            .collect()
    }

    fn track_index_at(&self, lanes: &[(usize, Rect)], y: f32) -> Option<usize> {
        lanes.iter().find(|(_, r)| y >= r.top() && y < r.bottom() + 1.0).map(|(i, _)| *i)
    }

    fn clip_at(&self, lanes: &[(usize, Rect)], pos: egui::Pos2, alt: bool) -> Option<(ClipId, Zone, usize)> {
        let (index, lane) = lanes.iter().find(|(_, r)| r.contains(pos))?;
        let track = &self.project.tracks[*index];
        track
            .clips
            .iter()
            .rev()
            .find_map(|c| hit_zone(&self.arr_view, *lane, c, pos, alt).map(|z| (c.id, z, *index)))
    }

    fn track_lanes(&mut self, ui: &mut Ui, area: Rect) {
        let view = self.arr_view;
        let lanes = self.lane_rects(area);
        let response = ui.interact(area, ui.scope_id().with("arranger-lanes"), Sense::click_and_drag());
        let pointer = response.hover_pos().or(response.interact_pointer_pos());
        let (alt, cmd, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.modifiers.shift));

        let hover_clip = pointer.and_then(|p| if area.contains(p) { self.clip_at(&lanes, p, alt) } else { None });
        if self.clip_drag.is_none() {
            if let Some((_, zone, _)) = hover_clip {
                ui.ctx().set_cursor_icon(zone.cursor());
                self.hint = zone.hint().to_string();
            }
        } else if let Some(d) = &self.clip_drag {
            ui.ctx().set_cursor_icon(if d.zone == Zone::Body { CursorIcon::Grabbing } else { d.zone.cursor() });
        }

        // Press and drag.
        if response.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                if let Some((clip, zone, track)) = self.clip_at(&lanes, origin, alt) {
                    self.selected_track = Some(self.project.tracks[track].id);
                    self.master_selected = false;
                    if cmd && !self.selection.contains(&clip) {
                        self.selection.push(clip);
                    }
                    let copy = alt && zone == Zone::Body;
                    self.begin_clip_drag(clip, zone, view.t(origin.x), track, copy);
                } else {
                    if !shift && !cmd {
                        self.selection.clear();
                    }
                    self.marquee = Some(Rect::from_two_pos(origin, origin));
                }
            }
        }
        if response.dragged() {
            if let Some(p) = response.interact_pointer_pos() {
                if self.clip_drag.is_some() {
                    let track = self.track_index_at(&lanes, p.y);
                    self.update_clip_drag(view.t(p.x), track, &view, cmd);
                }
                if let Some(m) = &mut self.marquee {
                    *m = Rect::from_two_pos(m.min, p);
                    let m = *m;
                    self.select_in_marquee(&lanes, m);
                }
                // Scroll when dragging against the edge of the view.
                let edge = 24.0;
                if p.x > area.right() - edge {
                    self.arr_view.pan(6.0);
                } else if p.x < area.left() + edge {
                    self.arr_view.pan(-6.0);
                }
            }
        }
        if response.drag_stopped() {
            if self.clip_drag.is_some() {
                self.end_clip_drag();
            }
            self.marquee = None;
        }

        // Clicks.
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                match self.clip_at(&lanes, p, false) {
                    Some((clip, _, track)) => {
                        if cmd || shift {
                            if let Some(i) = self.selection.iter().position(|c| *c == clip) {
                                self.selection.remove(i);
                            } else {
                                self.selection.push(clip);
                            }
                        } else {
                            self.selection = vec![clip];
                        }
                        self.selected_track = Some(self.project.tracks[track].id);
                        self.master_selected = false;
                    }
                    None => {
                        self.selection.clear();
                        if let Some(track) = self.track_index_at(&lanes, p.y) {
                            self.selected_track = Some(self.project.tracks[track].id);
                            self.master_selected = false;
                        }
                        let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                        self.snap_line = None;
                        self.seek(t);
                    }
                }
            }
        }
        if response.double_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                if let Some((clip, _, _)) = self.clip_at(&lanes, p, false) {
                    self.open_clip_in_editor(clip);
                }
            }
        }
        if response.secondary_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                if let Some((clip, _, track)) = self.clip_at(&lanes, p, false) {
                    if !self.selection.contains(&clip) {
                        self.selection = vec![clip];
                    }
                    self.selected_track = Some(self.project.tracks[track].id);
                }
            }
        }
        response.context_menu(|ui| self.clip_context_menu(ui));

        // Paint.
        let painter = ui.painter_at(area);
        let hover_id = hover_clip.map(|h| h.0);
        for (index, lane) in &lanes {
            if lane.bottom() < area.top() || lane.top() > area.bottom() {
                continue;
            }
            let track = &self.project.tracks[*index];
            let is_selected = self.selected_track == Some(track.id);
            if is_selected {
                painter.rect_filled(*lane, 0.0, alpha(track_color(track.color), 10));
            }
            painter.line_segment([lane.left_bottom(), lane.right_bottom()], Stroke::new(1.0, LINE));
            let muted = track.mute || (self.project.tracks.iter().any(|t| t.solo) && !track.solo);
            for clip in &track.clips {
                let r = clip_rect(&view, *lane, clip);
                if r.right() < area.left() || r.left() > area.right() {
                    continue;
                }
                self.paint_clip(
                    &painter,
                    &view,
                    *lane,
                    track,
                    clip,
                    PaintOptions {
                        selected: self.selection.contains(&clip.id),
                        hovered: hover_id == Some(clip.id),
                        show_waveform: lane.height() > 28.0,
                        track_muted: muted,
                    },
                );
                self.paint_word_labels(&painter, &view, *lane, clip);
            }
        }

        // Loop region tint, snap line, marquee and playhead go over the clips.
        let lp = self.project.loop_region;
        if lp.enabled {
            let (a, b) = (view.x(lp.start), view.x(lp.end));
            painter.rect_filled(Rect::from_min_max(egui::pos2(a, area.top()), egui::pos2(b, area.bottom())), 0.0, alpha(ACCENT, 10));
        }
        if let Some(t) = self.snap_line {
            paint_snap_line(&painter, view.x(t), area.top(), area.bottom());
        }
        if let Some(m) = self.marquee {
            painter.rect(m, 2.0, alpha(ACCENT, 25), Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        }
        paint_playhead(&painter, view.x(self.playhead), area.top(), area.bottom());
    }

    /// Shows the words inside a vocal clip once zoomed in enough to read them.
    fn paint_word_labels(&self, painter: &egui::Painter, view: &crate::timeline::TimeView, lane: Rect, clip: &mp_model::Clip) {
        // Word-sized clips already carry their label in the header.
        if clip.label.is_some() || view.px_per_s < 60.0 {
            return;
        }
        let Some(transcript) = self.index.transcript(clip.source.0) else { return };
        let y = lane.top() + crate::lane::HEADER_H + 1.0;
        let clip_area = painter.clip_rect().intersect(clip_rect(view, lane, clip));
        let clipped = painter.with_clip_rect(clip_area);
        for word in &transcript.words {
            if word.end < clip.src_in || word.start > clip.src_out {
                continue;
            }
            let (a, b) = (view.x(clip.time_of_source(word.start)), view.x(clip.time_of_source(word.end)));
            let (a, b) = (a.min(b), a.max(b));
            if b < clip_area.left() || a > clip_area.right() {
                continue;
            }
            let galley = clipped.layout_no_wrap(word.text.clone(), ui_font(10.5), TEXT);
            if galley.size().x < b - a {
                let bg = Rect::from_min_size(egui::pos2(a, y), egui::vec2(b - a, 14.0));
                clipped.rect_filled(bg, CornerRadius::same(2), alpha(BG_0, 170));
                clipped.galley(egui::pos2(a + 2.0, y), galley, TEXT);
            }
        }
    }

    fn select_in_marquee(&mut self, lanes: &[(usize, Rect)], m: Rect) {
        let view = self.arr_view;
        self.selection = lanes
            .iter()
            .flat_map(|(i, lane)| {
                self.project.tracks[*i]
                    .clips
                    .iter()
                    .filter(move |c| clip_rect(&view, *lane, c).intersects(m))
                    .map(|c| c.id)
            })
            .collect();
    }

    pub fn clip_context_menu(&mut self, ui: &mut Ui) {
        use crate::actions::Action as A;
        let items = [
            ("Split at playhead", "S", A::Split),
            ("Duplicate", "⌘D", A::Duplicate),
            ("Delete", "⌫", A::Delete),
            ("Reverse", "R", A::Reverse),
            ("Mute", "M", A::MuteClip),
            ("Slice to words", "W", A::SliceToWords),
            ("Snap pitch to key", "P", A::SnapPitchToKey),
            ("Quantize", "Q", A::Quantize),
            ("Close gaps", "G", A::CloseGaps),
            ("Fit to tempo", "", A::FitToTempo),
            ("Reset stretch and pitch", "⇧R", A::ResetStretch),
            ("Transcribe", "⇧⌘T", A::Transcribe),
        ];
        for (name, key, action) in items {
            if ui.add(egui::Button::new(name).shortcut_text(widgets::keys(key))).clicked() {
                self.run(action);
                ui.close();
            }
        }
    }

    pub fn open_clip_in_editor(&mut self, clip: ClipId) {
        let Some(track) = self.project.track_of_clip(clip) else { return };
        self.selection = vec![clip];
        self.selected_track = Some(track);
        self.show_bottom = true;
        self.bottom_tab = BottomTab::Editor;
        if let Some(c) = self.project.clip(clip) {
            let (a, b) = (c.start, c.end());
            self.editor.view.fit(a, b);
            self.editor.needs_fit = false;
        }
    }

    pub fn open_track_in_editor(&mut self, track: TrackId) {
        self.selected_track = Some(track);
        self.master_selected = false;
        self.show_bottom = true;
        self.bottom_tab = BottomTab::Editor;
        self.editor.needs_fit = true;
    }

    fn track_headers(&mut self, ui: &mut Ui, area: Rect, lanes_area: Rect) {
        let rows = self.lane_rects(area);
        ui.painter().rect_filled(area, 0.0, BG_1);
        let clip = ui.clip_rect().intersect(area);
        let mut remove_gesture = false;
        for (index, row) in rows {
            if row.bottom() < area.top() || row.top() > area.bottom() {
                continue;
            }
            let peaks = self.decayed_meter(index);
            let track_id = self.project.tracks[index].id;
            let selected = self.selected_track == Some(track_id);
            let color = track_color(self.project.tracks[index].color);
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(("hdr", track_id.0)).max_rect(row));
            child.set_clip_rect(clip);

            // The background goes in first so widgets stay on top of it.
            let back = child.interact(row, child.scope_id().with("back"), Sense::click());
            let fill = if selected { BG_3 } else if back.hovered() { BG_2 } else { BG_1 };
            child.painter().rect_filled(row, 0.0, fill);
            child.painter().rect_filled(Rect::from_min_size(row.min, egui::vec2(4.0, row.height())), 0.0, color);
            child.painter().line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, LINE));
            if back.clicked() {
                self.selected_track = Some(track_id);
                self.master_selected = false;
            }
            if back.double_clicked() {
                self.open_track_in_editor(track_id);
            }
            if back.hovered() {
                self.hint = "Double-click to open this track in the editor.".to_string();
            }
            back.context_menu(|ui| self.track_context_menu(ui, track_id));

            let mut content = child.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(row.min + egui::vec2(10.0, 5.0), row.max - egui::vec2(16.0, 3.0))));
            let (name, kind, mute, solo) = {
                let t = &self.project.tracks[index];
                (t.name.clone(), t.kind, t.mute, t.solo)
            };
            content.horizontal(|ui| {
                if self.renaming_track.as_ref().is_some_and(|(t, _)| *t == track_id) {
                    let mut text = self.renaming_track.as_ref().map_or(String::new(), |r| r.1.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
                    r.request_focus();
                    let done = r.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter));
                    self.renaming_track = Some((track_id, text.clone()));
                    if done {
                        self.renaming_track = None;
                        self.edit("Rename track", |p| {
                            if let Some(t) = p.track_mut(track_id) {
                                t.name = text;
                            }
                        });
                    }
                } else {
                    ui.label(egui::RichText::new(truncate(&name, 20)).color(if selected { TEXT } else { mix(TEXT, TEXT_DIM, 0.3) }).size(12.5).strong());
                }
            });
            content.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if letter_toggle(ui, "M", mute, MUTE, "Mute").clicked() {
                    self.edit("Mute track", |p| {
                        if let Some(t) = p.track_mut(track_id) {
                            t.mute = !t.mute;
                        }
                    });
                }
                if letter_toggle(ui, "S", solo, SOLO, "Solo").clicked() {
                    self.edit("Solo track", |p| {
                        if let Some(t) = p.track_mut(track_id) {
                            t.solo = !t.solo;
                        }
                    });
                }
                ui.add_space(3.0);
                let mut db = self.project.tracks[index].gain_db;
                let f = fader(ui, &mut db, egui::vec2(96.0, 16.0));
                if f.started {
                    self.begin_gesture("Change track gain");
                }
                if f.changed {
                    if let Some(t) = self.project.track_mut(track_id) {
                        t.gain_db = db;
                    }
                    self.needs_compile = true;
                }
                if f.finished {
                    remove_gesture = true;
                }
                f.response.on_hover_text(format!("{db:+.1} dB"));
            });
            if row.height() > 62.0 {
                let label = if self.index.transcript(self.project.tracks[index].clips.first().map_or(0, |c| c.source.0)).is_some() {
                    format!("{} · {} words", kind.label(), self.project.tracks[index].clips.first().and_then(|c| self.index.transcript(c.source.0)).map_or(0, |t| t.words.len()))
                } else {
                    kind.label().to_string()
                };
                content.label(egui::RichText::new(label).color(TEXT_FAINT).size(10.5));
            }

            // Meter along the right edge.
            let meter_rect = Rect::from_min_max(egui::pos2(row.right() - 11.0, row.top() + 5.0), egui::pos2(row.right() - 4.0, row.bottom() - 5.0));
            meter(child.painter(), meter_rect, peaks);

            // Drag the bottom edge to resize, like a window.
            let grip = Rect::from_min_max(egui::pos2(row.left(), row.bottom() - 3.0), egui::pos2(lanes_area.right(), row.bottom() + 2.0));
            let resize = ui.interact(grip, ui.scope_id().with(("resize", track_id.0)), Sense::drag());
            if resize.hovered() || resize.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if resize.dragged() {
                if let Some(t) = self.project.track_mut(track_id) {
                    t.height = (t.height + resize.drag_delta().y).clamp(MIN_TRACK_H, MAX_TRACK_H);
                }
            }
        }
        if remove_gesture {
            self.end_gesture();
        }
        ui.painter().line_segment([area.right_top(), area.right_bottom()], Stroke::new(1.0, LINE));
    }

    fn track_context_menu(&mut self, ui: &mut Ui, track: TrackId) {
        self.selected_track = Some(track);
        if ui.button("Open in editor").clicked() {
            self.open_track_in_editor(track);
            ui.close();
        }
        if ui.button("Rename").clicked() {
            let name = self.project.track(track).map_or(String::new(), |t| t.name.clone());
            self.renaming_track = Some((track, name));
            ui.close();
        }
        if ui.button("Transcribe").clicked() {
            self.run(crate::actions::Action::Transcribe);
            ui.close();
        }
        if ui.button("Slice to words").clicked() {
            self.selection.clear();
            self.slice_selection_to_words();
            ui.close();
        }
        ui.menu_button("Type", |ui| {
            for kind in [mp_model::TrackKind::Vocal, mp_model::TrackKind::Drums, mp_model::TrackKind::Bass, mp_model::TrackKind::Music] {
                if ui.button(kind.label()).clicked() {
                    self.edit("Change track type", |p| {
                        if let Some(t) = p.track_mut(track) {
                            t.kind = kind;
                        }
                    });
                    ui.close();
                }
            }
        });
        ui.separator();
        let position = self.project.tracks.iter().position(|t| t.id == track).unwrap_or(0);
        if ui.add_enabled(position > 0, egui::Button::new("Move up")).clicked() {
            self.edit("Move track", |p| p.tracks.swap(position, position - 1));
            ui.close();
        }
        if ui.add_enabled(position + 1 < self.project.tracks.len(), egui::Button::new("Move down")).clicked() {
            self.edit("Move track", |p| p.tracks.swap(position, position + 1));
            ui.close();
        }
        if ui.button("Delete track").clicked() {
            self.run(crate::actions::Action::DeleteTrack);
            ui.close();
        }
    }

    pub fn decayed_meter(&mut self, index: usize) -> [f32; 2] {
        let peak = self.engine.shared.track_peak(index);
        let hold = self.meter_hold.entry(index).or_insert([0.0; 2]);
        for c in 0..2 {
            hold[c] = peak[c].max(hold[c] * 0.86);
        }
        *hold
    }

    fn ruler_ui(&mut self, ui: &mut Ui, ruler: Rect) {
        draw_ruler(ui.painter(), ruler, &self.project, &self.arr_view);
        let view = self.arr_view;
        let response = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        response.clone().on_hover_cursor(CursorIcon::PointingHand);
        if response.hovered() {
            self.hint = "Click to move the playhead. Shift-drag to set the loop. Drag to scrub.".to_string();
        }
        let loop_band = Rect::from_min_max(egui::pos2(view.x(self.project.loop_region.start), ruler.bottom() - 5.0), egui::pos2(view.x(self.project.loop_region.end), ruler.bottom()));
        let color = if self.project.loop_region.enabled { ACCENT } else { BG_4 };
        ui.painter().with_clip_rect(ruler).rect_filled(loop_band, 0.0, color);

        if shift && response.drag_started() {
            self.begin_gesture("Set loop");
        }
        if response.dragged() {
            if let Some(p) = response.interact_pointer_pos() {
                if shift {
                    if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                        let a = self.snap_time(view.t(origin.x), &view, &[], cmd);
                        let b = self.snap_time(view.t(p.x), &view, &[], cmd);
                        let lp = &mut self.project.loop_region;
                        lp.start = a.min(b).max(0.0);
                        lp.end = a.max(b);
                        lp.enabled = true;
                        self.needs_compile = true;
                    }
                } else {
                    self.seek(view.t(p.x));
                }
            }
        }
        if response.drag_stopped() {
            self.snap_line = None;
            self.end_gesture();
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                self.snap_line = None;
                self.seek(t);
            }
        }
        // Playhead cap.
        let x = view.x(self.playhead);
        if x >= ruler.left() && x <= ruler.right() {
            let pts = vec![egui::pos2(x - 5.0, ruler.bottom() - 8.0), egui::pos2(x + 5.0, ruler.bottom() - 8.0), egui::pos2(x, ruler.bottom())];
            ui.painter().add(egui::Shape::convex_polygon(pts, ACCENT, Stroke::NONE));
        }
    }

    /// Minimap of the whole project. Click or drag to move the view.
    fn overview(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, BG_2);
        let end = self.project.duration().max(self.arr_view.right_time()).max(1.0);
        let to_x = |t: f64| rect.left() + (t / end) as f32 * rect.width();
        let n = self.project.tracks.len().max(1);
        let row_h = (rect.height() - 4.0) / n as f32;
        for (i, track) in self.project.tracks.iter().enumerate() {
            let c = alpha(track_color(track.color), 150);
            let y = rect.top() + 2.0 + i as f32 * row_h;
            for clip in &track.clips {
                let r = Rect::from_min_max(egui::pos2(to_x(clip.start), y), egui::pos2(to_x(clip.end()).max(to_x(clip.start) + 1.0), y + (row_h - 1.0).max(1.0)));
                ui.painter().rect_filled(r, 0.0, c);
            }
        }
        let window = Rect::from_min_max(egui::pos2(to_x(self.arr_view.scroll), rect.top()), egui::pos2(to_x(self.arr_view.right_time()), rect.bottom()));
        ui.painter().rect(window, 2.0, alpha(TEXT, 18), Stroke::new(1.0, alpha(TEXT, 90)), StrokeKind::Inside);
        let x = to_x(self.playhead);
        ui.painter().line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.0, ACCENT));

        let response = ui.interact(rect, ui.scope_id().with("overview"), Sense::click_and_drag());
        if response.clicked() || response.dragged() {
            if let Some(p) = response.interact_pointer_pos() {
                let span = self.arr_view.right_time() - self.arr_view.scroll;
                let t = (p.x - rect.left()) as f64 / rect.width() as f64 * end;
                self.arr_view.scroll = (t - span * 0.5).max(0.0);
            }
        }
    }

    fn empty_state(&mut self, ui: &mut Ui, area: Rect) {
        let c = area.center();
        let painter = ui.painter();
        let dash = Rect::from_center_size(c, egui::vec2(520.0, 250.0));
        painter.rect_stroke(dash, CornerRadius::same(14), Stroke::new(1.0, BG_3), StrokeKind::Inside);
        painter.text(c + egui::vec2(0.0, -78.0), Align2::CENTER_CENTER, "Drop your stems here", ui_font(26.0), TEXT);
        painter.text(c + egui::vec2(0.0, -46.0), Align2::CENTER_CENTER, "Vocals are transcribed, tempo and key detected, tracks named and coloured.", ui_font(13.0), TEXT_DIM);
        let steps = [
            ("1", "Drop stems", "WAV, MP3, FLAC, AIFF, M4A, OGG"),
            ("2", "Find words", "Search or type a whole sentence"),
            ("3", "Render", "Export the finished mix to MP3"),
        ];
        for (i, (n, title, sub)) in steps.iter().enumerate() {
            let x = c.x + (i as f32 - 1.0) * 165.0;
            let y = c.y + 22.0;
            painter.circle_filled(egui::pos2(x, y), 13.0, ACCENT_SOFT);
            painter.text(egui::pos2(x, y), Align2::CENTER_CENTER, *n, ui_font(13.0), ACCENT);
            painter.text(egui::pos2(x, y + 26.0), Align2::CENTER_CENTER, *title, ui_font(13.0), TEXT);
            painter.text(egui::pos2(x, y + 44.0), Align2::CENTER_CENTER, *sub, ui_font(10.0), TEXT_FAINT);
        }
        let button_rect = Rect::from_center_size(c + egui::vec2(0.0, 98.0), egui::vec2(130.0, 28.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(button_rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
        if widgets::accent_button(&mut child, "Choose files…").clicked() {
            self.import_dialog();
        }
    }

    /// While a word is dragged from the browser, shows where it will land, and places it on release.
    fn drop_word_on_lanes(&mut self, ui: &mut Ui, area: Rect) {
        let Some(hit) = self.hit_drag.clone() else { return };
        let view = self.arr_view;
        let (pointer, released, cmd) = ui.input(|i| (i.pointer.hover_pos().or(i.pointer.latest_pos()), i.pointer.any_released(), i.modifiers.command));
        let Some(pos) = pointer else { return };
        let rows = self.lane_rects(area);
        let track_index = self.track_index_at(&rows, pos.y);
        let inside = area.contains(pos);
        let duration = hit.end - hit.start;
        let start = if inside { self.snap_time(view.t(pos.x), &view, &[], cmd).max(0.0) } else { 0.0 };

        if inside {
            let lane = track_index.and_then(|i| rows.get(i).map(|r| r.1)).unwrap_or_else(|| {
                let y = rows.last().map_or(area.top(), |r| r.1.bottom() + 1.0);
                Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(area.width(), 60.0))
            });
            let ghost = Rect::from_min_max(egui::pos2(view.x(start), lane.top()), egui::pos2(view.x(start + duration).max(view.x(start) + 24.0), lane.bottom()));
            let painter = ui.painter_at(area);
            painter.rect(ghost, 3.0, alpha(ACCENT, 70), Stroke::new(1.5, ACCENT), StrokeKind::Inside);
            painter.text(ghost.left_top() + egui::vec2(4.0, 2.0), Align2::LEFT_TOP, &hit.text, ui_font(11.0), TEXT);
        }
        if released {
            self.hit_drag = None;
            self.snap_line = None;
            if inside {
                let target = track_index.map(|i| self.project.tracks[i].id);
                let placed = self.edit(&format!("Place \"{}\"", hit.text), |p| {
                    let track = target.unwrap_or_else(|| App::sentence_track(p));
                    App::place_hit_at(p, &hit, track, start).map(|c| (c, track))
                });
                if let Some((clip, track)) = placed {
                    self.selection = vec![clip];
                    self.selected_track = Some(track);
                }
            }
        }
    }
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
