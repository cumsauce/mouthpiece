//! Painting clips and handling direct manipulation. The arranger and the detail editor share this,
//! so a clip behaves the same everywhere.

use crate::app::App;
use crate::ds::*;
use crate::panels::timeline::TimeView;
use crate::panels::waveform::draw_clip_waveform;
use eframe::egui::{self, Align2, CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind};
use mp_engine::render_cache::RenderKey;
use mp_model::{Clip, ClipId, Track, TrackId};

pub const HEADER_H: f32 = 14.0;
const EDGE_PX: f32 = 6.0;
const SNAP_PX: f64 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zone {
    Body,
    TrimStart,
    TrimEnd,
    StretchStart,
    StretchEnd,
    FadeIn,
    FadeOut,
}

impl Zone {
    pub fn cursor(self) -> CursorIcon {
        match self {
            Zone::Body => CursorIcon::Grab,
            Zone::TrimStart | Zone::TrimEnd => CursorIcon::ResizeHorizontal,
            Zone::StretchStart | Zone::StretchEnd => CursorIcon::ResizeColumn,
            Zone::FadeIn | Zone::FadeOut => CursorIcon::Crosshair,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Zone::Body => "Drag to move. Alt-drag to copy. In a word, click again to select one piece. Double-click to edit.",
            Zone::TrimStart | Zone::TrimEnd => "Drag to trim. Alt-drag to stretch.",
            Zone::StretchStart | Zone::StretchEnd => "Drag to stretch.",
            Zone::FadeIn | Zone::FadeOut => "Drag to set the fade.",
        }
    }
}

pub struct ClipDrag {
    pub zone: Zone,
    pub anchor: ClipId,
    /// Pointer time minus the anchor's start, at press.
    pub grab: f64,
    /// Selected clips at press, with their start and track index.
    pub originals: Vec<(ClipId, f64, usize)>,
    pub press_track: usize,
}

pub fn clip_rect(view: &TimeView, lane: Rect, clip: &Clip) -> Rect {
    Rect::from_min_max(egui::pos2(view.x(clip.start), lane.top()), egui::pos2(view.x(clip.end()), lane.bottom()))
}

/// Which part of a clip a position is over. Alt turns edge trims into stretches.
pub fn hit_zone(view: &TimeView, lane: Rect, clip: &Clip, pos: Pos2, alt: bool) -> Option<Zone> {
    let r = clip_rect(view, lane, clip);
    if !r.expand2(egui::vec2(2.0, 0.0)).contains(pos) {
        return None;
    }
    // Narrow clips are all body, or they could never be moved.
    if r.width() < EDGE_PX * 3.0 {
        return Some(Zone::Body);
    }
    if pos.y < r.top() + HEADER_H {
        let fade_in_x = view.x(clip.start + clip.fade_in);
        let fade_out_x = view.x(clip.end() - clip.fade_out);
        if (pos.x - fade_in_x).abs() < 7.0 && pos.x > r.left() + EDGE_PX {
            return Some(Zone::FadeIn);
        }
        if (pos.x - fade_out_x).abs() < 7.0 && pos.x < r.right() - EDGE_PX {
            return Some(Zone::FadeOut);
        }
    }
    if pos.x - r.left() < EDGE_PX {
        return Some(if alt { Zone::StretchStart } else { Zone::TrimStart });
    }
    if r.right() - pos.x < EDGE_PX {
        return Some(if alt { Zone::StretchEnd } else { Zone::TrimEnd });
    }
    Some(Zone::Body)
}

pub struct PaintOptions {
    pub selected: bool,
    pub hovered: bool,
    pub show_waveform: bool,
    pub track_muted: bool,
}

impl App {
    /// Marks what's joined on a lane: an X across crossfades, and a bracket with a name over each group.
    pub fn paint_joins(&self, painter: &Painter, view: &TimeView, lane: Rect, track: &Track) {
        let visible = |a: f32, b: f32| b >= lane.left().min(view.left) && a <= view.left + view.width;
        let mut clips: Vec<&Clip> = track.clips.iter().collect();
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        for pair in clips.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b.start < a.end() && b.end() > a.end() {
                let (x0, x1) = (view.x(b.start), view.x(a.end()));
                if x1 - x0 > 3.0 && visible(x0, x1) {
                    let (top, bottom) = (lane.top() + HEADER_H, lane.bottom());
                    let stroke = Stroke::new(1.0, alpha(TEXT, 170));
                    painter.line_segment([egui::pos2(x0, top), egui::pos2(x1, bottom)], stroke);
                    painter.line_segment([egui::pos2(x0, bottom), egui::pos2(x1, top)], stroke);
                }
            }
        }
        let mut done: Vec<mp_model::GroupId> = Vec::new();
        for clip in &clips {
            let Some(group) = clip.group else { continue };
            if done.contains(&group) {
                continue;
            }
            done.push(group);
            let members = clips.iter().filter(|c| c.group == Some(group));
            let (a, b) = members.fold((f64::MAX, 0f64), |(a, b), c| (a.min(c.start), b.max(c.end())));
            let (x0, x1) = (view.x(a), view.x(b));
            if !visible(x0, x1) {
                continue;
            }
            let y = lane.bottom() - SP_HALF;
            painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], Stroke::new(2.0, TEXT_2));
            if let Some(name) = &clip.word_name {
                let galley = painter.layout_no_wrap(name.clone(), font_micro(), TEXT);
                if galley.size().x + SP_1 < x1 - x0 {
                    painter.galley(egui::pos2(x0 + SP_HALF, y - galley.size().y - SP_HALF), galley, TEXT);
                }
            }
        }
    }

    pub fn clip_label(&self, clip: &Clip) -> String {
        if let Some(label) = &clip.label {
            return label.clone();
        }
        self.project.source(clip.source).map_or_else(String::new, |s| s.name.clone())
    }

    /// Whether a stretch or repitch render for this clip is still in progress.
    pub fn clip_rendering(&self, clip: &Clip) -> bool {
        if !clip.needs_render() {
            return false;
        }
        let sr = self.sample_rate as f64;
        let key = RenderKey::of(clip, (clip.src_in * sr).round() as usize, (clip.src_out * sr).round() as usize);
        !self.cache.is_ready(&key)
    }

    pub fn paint_clip(&self, painter: &Painter, view: &TimeView, lane: Rect, track: &Track, clip: &Clip, o: PaintOptions) {
        let body = clip_rect(view, lane, clip);
        let visible = body.intersect(painter.clip_rect());
        if !visible.is_positive() {
            return;
        }
        let mut base = track_color(track.color);
        if clip.muted || o.track_muted {
            base = mix(base, CONTROL, 0.8);
        }
        let radius = R_S;
        painter.rect_filled(body, radius, mix(PANEL, base, 0.2));

        let waveform_rect = Rect::from_min_max(egui::pos2(body.left(), lane.top() + HEADER_H), body.max);
        if o.show_waveform {
            if let Some(runtime) = self.runtime.get(&clip.source) {
                let clipped = painter.with_clip_rect(painter.clip_rect().intersect(body));
                draw_clip_waveform(
                    &clipped,
                    waveform_rect,
                    painter.clip_rect(),
                    clip,
                    &runtime.peaks,
                    self.sample_rate as f64,
                    mix(PANEL, base, 0.85),
                    mix(base, TEXT, 0.6),
                );
            }
        }

        // Header strip with name and state badges.
        let header = Rect::from_min_size(body.min, egui::vec2(body.width(), HEADER_H));
        painter.rect_filled(
            header,
            egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
            mix(PANEL, base, if o.selected { 0.75 } else { 0.42 }),
        );
        if body.width() > 16.0 {
            let clipped = painter.with_clip_rect(painter.clip_rect().intersect(header));
            let text_color = TEXT;
            let x = body.left().max(view.left) + 4.0;
            clipped.text(egui::pos2(x, header.center().y), Align2::LEFT_CENTER, self.clip_label(clip), font_micro(), text_color);
        }
        self.paint_badges(painter, body, clip);

        // Fades, as a shaded corner and a handle that appears on hover.
        let fade_fill = alpha(BASE, 150);
        if clip.fade_in > 0.012 {
            let x = view.x(clip.start + clip.fade_in);
            painter.add(egui::Shape::convex_polygon(
                vec![body.left_top(), egui::pos2(x, body.top()), body.left_bottom()],
                fade_fill,
                Stroke::NONE,
            ));
        }
        if clip.fade_out > 0.012 {
            let x = view.x(clip.end() - clip.fade_out);
            painter.add(egui::Shape::convex_polygon(
                vec![egui::pos2(x, body.top()), body.right_top(), body.right_bottom()],
                fade_fill,
                Stroke::NONE,
            ));
        }
        if o.hovered && body.width() > EDGE_PX * 3.0 {
            for x in [view.x(clip.start + clip.fade_in), view.x(clip.end() - clip.fade_out)] {
                let x = x.clamp(body.left() + 3.0, body.right() - 3.0);
                painter.circle_filled(egui::pos2(x, body.top() + 3.0), 3.5, TEXT);
            }
        }

        if self.clip_rendering(clip) {
            let t = painter.ctx().input(|i| i.time) as f32;
            let c = alpha(WARN, 60 + ((t * 5.0).sin().abs() * 80.0) as u8);
            painter.rect_filled(Rect::from_min_size(body.left_bottom() - egui::vec2(0.0, 2.0), egui::vec2(body.width(), 2.0)), 0.0, c);
        }

        let outline = if o.selected { Stroke::new(1.5, ACCENT) } else { Stroke::new(1.0, mix(BASE, base, 0.25)) };
        painter.rect_stroke(body, radius, outline, StrokeKind::Inside);
        if let Some(born) = self.flash.get(&clip.id) {
            let fade = 1.0 - born.elapsed().as_secs_f32() / MOTION_FLASH;
            if fade > 0.0 {
                painter.rect_filled(body, radius, alpha(ACCENT, (fade * 90.0) as u8));
            }
        }
    }

    fn paint_badges(&self, painter: &Painter, body: Rect, clip: &Clip) {
        if body.width() < 48.0 {
            return;
        }
        let mut x = body.right() - 4.0;
        let mut badge = |text: String| {
            let galley = painter.layout_no_wrap(text, font_micro(), TEXT);
            let w = galley.size().x;
            if x - w > body.left() + 30.0 {
                painter.galley(egui::pos2(x - w, body.top() + 2.0), galley, TEXT);
                x -= w + 5.0;
            }
        };
        if clip.pitch.abs() > 0.005 && clip.mode == mp_model::ClipMode::Elastic {
            badge(format!("{:+.1}st", clip.pitch));
        }
        if (clip.stretch - 1.0).abs() > 0.001 {
            let label = if clip.mode == mp_model::ClipMode::Varispeed { "tape " } else { "" };
            badge(format!("{label}{:.0}%", 100.0 / clip.stretch));
        }
        if clip.gain_db.abs() > 0.05 {
            badge(format!("{:+.1}dB", clip.gain_db));
        }
        if clip.reversed {
            let r = Rect::from_center_size(egui::pos2(x - 6.0, body.top() + 7.0), egui::vec2(10.0, 8.0));
            draw_icon(painter, r, Icon::Reverse, TEXT);
        }
    }

    // ----- Snapping -----

    /// Magnetic snapping: `t` jumps to the nearest grid line, clip edge or the playhead if one is
    /// within a few pixels. Holding Command turns it off for the moment.
    pub fn snap_time(&mut self, t: f64, view: &TimeView, ignore: &[ClipId], cmd_held: bool) -> f64 {
        self.snap_line = None;
        if !self.snap || cmd_held {
            return t;
        }
        let threshold = SNAP_PX / view.px_per_s;
        let mut best: Option<(f64, f64)> = None;
        let mut consider = |candidate: f64, bias: f64| {
            let d = (candidate - t).abs() * bias;
            if d <= threshold && best.is_none_or(|(_, b)| d < b) {
                best = Some((candidate, d));
            }
        };
        consider(self.project.snap_to_grid(t, self.grid_division), 1.0);
        consider(self.playhead, 0.8);
        for clip in self.project.tracks.iter().flat_map(|tr| &tr.clips) {
            if !ignore.contains(&clip.id) {
                // Clip edges win over grid lines at equal distance, as splices matter most here.
                consider(clip.start, 0.7);
                consider(clip.end(), 0.7);
            }
        }
        match best {
            Some((candidate, _)) => {
                self.snap_line = Some(candidate);
                candidate
            }
            None => t,
        }
    }

    // ----- Dragging -----

    /// Begins dragging a clip. `alt_copy` duplicates the selection and drags the copies.
    pub fn begin_clip_drag(&mut self, clip: ClipId, zone: Zone, pointer_t: f64, press_track: usize, alt_copy: bool) {
        if !self.selection.contains(&clip) {
            self.selection = if zone == Zone::Body { self.project.group_members(clip) } else { vec![clip] };
        }
        self.begin_gesture(match zone {
            Zone::Body => "Move clip",
            Zone::TrimStart | Zone::TrimEnd => "Trim clip",
            Zone::StretchStart | Zone::StretchEnd => "Stretch clip",
            Zone::FadeIn | Zone::FadeOut => "Set fade",
        });
        let mut anchor = clip;
        if alt_copy && zone == Zone::Body {
            let originals = self.selection.clone();
            let copies = self.project.duplicate_clips(&originals, 0.0);
            if let Some(i) = originals.iter().position(|id| *id == clip) {
                anchor = copies.get(i).copied().unwrap_or(clip);
            }
            self.selection = copies;
        }
        let track_index = |app: &App, id: ClipId| {
            app.project.tracks.iter().position(|t| t.clips.iter().any(|c| c.id == id)).unwrap_or(0)
        };
        let originals = self
            .selection
            .iter()
            .filter_map(|id| Some((*id, self.project.clip(*id)?.start, track_index(self, *id))))
            .collect();
        let start = self.project.clip(anchor).map_or(0.0, |c| c.start);
        self.clip_drag = Some(ClipDrag { zone, anchor, grab: pointer_t - start, originals, press_track });
    }

    /// Applies the current drag for a pointer time and, when known, the track under the pointer.
    pub fn update_clip_drag(&mut self, pointer_t: f64, pointer_track: Option<usize>, view: &TimeView, cmd_held: bool) {
        let Some(drag) = self.clip_drag.take() else { return };
        let ignore: Vec<ClipId> = drag.originals.iter().map(|o| o.0).collect();
        match drag.zone {
            Zone::Body => {
                let anchor_start = drag.originals.iter().find(|o| o.0 == drag.anchor).map_or(0.0, |o| o.1);
                let wanted = pointer_t - drag.grab;
                let length = self.project.clip(drag.anchor).map_or(0.0, Clip::length);
                // Snap either edge, whichever is closer to a target.
                let start_snapped = self.snap_time(wanted, view, &ignore, cmd_held);
                let start_line = self.snap_line;
                let end_snapped = self.snap_time(wanted + length, view, &ignore, cmd_held) - length;
                let use_end = self.snap_line.is_some()
                    && (start_line.is_none() || (end_snapped - wanted).abs() < (start_snapped - wanted).abs());
                if !use_end {
                    self.snap_line = start_line;
                }
                let target = if use_end { end_snapped } else { start_snapped };
                let earliest = drag.originals.iter().map(|o| o.1).fold(f64::MAX, f64::min);
                let delta = (target - anchor_start).max(-earliest);
                let track_delta = pointer_track.map_or(0, |t| t as i64 - drag.press_track as i64);
                let track_ids: Vec<TrackId> = self.project.tracks.iter().map(|t| t.id).collect();
                for (id, start, track) in &drag.originals {
                    if let Some(c) = self.project.clip_mut(*id) {
                        c.start = start + delta;
                    }
                    let to = (*track as i64 + track_delta).clamp(0, track_ids.len() as i64 - 1) as usize;
                    if let Some(t) = track_ids.get(to) {
                        self.project.move_clip_to_track(*id, *t);
                    }
                }
            }
            Zone::TrimStart => {
                let t = self.snap_time(pointer_t, view, &ignore, cmd_held);
                self.project.trim_start(drag.anchor, t);
            }
            Zone::TrimEnd => {
                let t = self.snap_time(pointer_t, view, &ignore, cmd_held);
                self.project.trim_end(drag.anchor, t);
            }
            Zone::StretchEnd => {
                let t = self.snap_time(pointer_t, view, &ignore, cmd_held);
                if let Some(c) = self.project.clip(drag.anchor) {
                    let ratio = (t - c.start).max(0.01) / c.src_len();
                    self.project.set_stretch(drag.anchor, ratio);
                }
            }
            Zone::StretchStart => {
                let t = self.snap_time(pointer_t, view, &ignore, cmd_held);
                if let Some((end, src_len)) = self.project.clip(drag.anchor).map(|c| (c.end(), c.src_len())) {
                    let t = t.clamp(0.0, end - 0.01);
                    self.project.set_stretch(drag.anchor, (end - t) / src_len);
                    if let Some(c) = self.project.clip_mut(drag.anchor) {
                        c.start = end - c.length();
                    }
                }
            }
            Zone::FadeIn => {
                if let Some(c) = self.project.clip_mut(drag.anchor) {
                    c.fade_in_auto = false;
                    c.fade_in = (pointer_t - c.start).clamp(0.0, (c.length() - c.fade_out).max(0.0));
                }
            }
            Zone::FadeOut => {
                if let Some(c) = self.project.clip_mut(drag.anchor) {
                    c.fade_out_auto = false;
                    c.fade_out = (c.end() - pointer_t).clamp(0.0, (c.length() - c.fade_in).max(0.0));
                }
            }
        }
        // Overlaps crossfade as they happen, so that a drag can be judged by ear.
        if !matches!(drag.zone, Zone::FadeIn | Zone::FadeOut) {
            self.project.update_auto_crossfades();
        }
        self.needs_compile = true;
        self.clip_drag = Some(drag);
    }

    pub fn end_clip_drag(&mut self) {
        self.clip_drag = None;
        self.snap_line = None;
        self.end_gesture();
    }
}

/// Draws the playhead over a timeline area.
pub fn paint_playhead(painter: &Painter, x: f32, top: f32, bottom: f32) {
    painter.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], Stroke::new(1.5, ACCENT));
}

pub fn paint_snap_line(painter: &Painter, x: f32, top: f32, bottom: f32) {
    painter.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], Stroke::new(1.0, alpha(TEXT, 170)));
}
