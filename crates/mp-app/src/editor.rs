//! The detail editor: one track's clips up close, with its words, phonemes and spectrogram.

use crate::app::{App, BottomTab, SpectrogramState};
use crate::lane::{clip_rect, hit_zone, paint_playhead, paint_snap_line, PaintOptions, Zone};
use crate::theme::*;
use crate::timeline::{draw_grid, draw_ruler, TimeView};
use crate::widgets;
use eframe::egui::{self, Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui};
use mp_analysis::search::{Hit, HitKind};
use mp_model::{Clip, SourceId, Track, TrackId};

const RULER_H: f32 = 20.0;
const WORDS_H: f32 = 30.0;
const PHONEMES_H: f32 = 22.0;

pub struct EditorState {
    pub view: TimeView,
    pub needs_fit: bool,
    pub hovered: bool,
    /// Selected time range on the arrangement.
    pub range: Option<(f64, f64)>,
    pub range_drag_origin: Option<f64>,
    pub show_spectrogram: bool,
    pub show_words: bool,
    pub show_phonemes: bool,
    pub selected_word: Option<(u64, usize)>,
    pub last_track: Option<TrackId>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            view: TimeView::new(120.0),
            needs_fit: true,
            hovered: false,
            range: None,
            range_drag_origin: None,
            show_spectrogram: true,
            show_words: true,
            show_phonemes: true,
            selected_word: None,
            last_track: None,
        }
    }
}

/// A word or phoneme block as drawn: its source range and where it sits on the arrangement.
struct Chip {
    hit: Hit,
    x0: f32,
    x1: f32,
}

impl App {
    pub fn bottom_panel(&mut self, ui: &mut Ui) {
        let bar = ui.horizontal(|ui| {
            ui.add_space(4.0);
            for (name, tab) in [("Detail Editor", BottomTab::Editor), ("Devices", BottomTab::Devices), ("Mixer", BottomTab::Mixer)] {
                if widgets::tab(ui, name, self.bottom_tab == tab) {
                    self.bottom_tab = tab;
                }
            }
            ui.add_space(12.0);
            match self.bottom_tab {
                BottomTab::Editor => self.editor_toolbar(ui),
                BottomTab::Devices => self.devices_toolbar(ui),
                BottomTab::Mixer => {}
            }
        });
        ui.painter().line_segment(
            [egui::pos2(ui.max_rect().left(), bar.response.rect.bottom() + 2.0), egui::pos2(ui.max_rect().right(), bar.response.rect.bottom() + 2.0)],
            Stroke::new(1.0, LINE),
        );
        ui.add_space(3.0);
        match self.bottom_tab {
            BottomTab::Editor => self.detail_editor(ui),
            BottomTab::Devices => self.devices_panel(ui),
            BottomTab::Mixer => self.mixer_panel(ui),
        }
    }

    fn editor_toolbar(&mut self, ui: &mut Ui) {
        let Some(track) = self.selected_track.and_then(|t| self.project.track(t)) else { return };
        let color = track_color(track.color);
        ui.label(egui::RichText::new(&track.name).color(color).strong());
        let first_source = track.clips.first().map(|c| c.source);
        let transcribed = first_source.is_some_and(|s| self.index.transcript(s.0).is_some());
        ui.add_space(8.0);
        for (label, flag) in [("Spectrogram", 0), ("Words", 1), ("Phonemes", 2)] {
            let on = match flag {
                0 => self.editor.show_spectrogram,
                1 => self.editor.show_words,
                _ => self.editor.show_phonemes,
            };
            if ui.selectable_label(on, label).clicked() {
                match flag {
                    0 => self.editor.show_spectrogram = !on,
                    1 => self.editor.show_words = !on,
                    _ => self.editor.show_phonemes = !on,
                }
            }
        }
        ui.add_space(8.0);
        if ui.button("Fit").on_hover_text("Fit this track in the editor").clicked() {
            self.editor.needs_fit = true;
        }
        if transcribed {
            if ui.button("Slice to words").on_hover_text("Turn the selected clip, or the whole track, into one clip per word (W)").clicked() {
                self.slice_selection_to_words();
            }
        } else if first_source.is_some() && ui.button("Transcribe").on_hover_text(widgets::keys("Find the words in this track (⇧⌘T)")).clicked() {
            self.run(crate::actions::Action::Transcribe);
        }
        if let Some((a, b)) = self.editor.range {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("{:.3} s selected", b - a)).color(ACCENT).size(11.5));
            if ui.button("Play").clicked() {
                self.audition_editor_range();
            }
            if ui.button("Place on Sentence").on_hover_text("Copy this selection to the Sentence track at the playhead (E)").clicked() {
                self.place_editor_range();
            }
        }
    }

    /// The clip under an arrangement time on the selected track, as a source and source range.
    fn range_to_source(&self, a: f64, b: f64) -> Option<(SourceId, f64, f64)> {
        let track = self.project.track(self.selected_track?)?;
        let mid = (a + b) * 0.5;
        let clip = track.clips.iter().rev().find(|c| c.start <= mid && mid < c.end())?;
        let (x, y) = (clip.source_time_at(a.max(clip.start)), clip.source_time_at(b.min(clip.end())));
        Some((clip.source, x.min(y), x.max(y)))
    }

    fn audition_editor_range(&mut self) {
        if let Some((a, b)) = self.editor.range {
            if let Some((source, x, y)) = self.range_to_source(a, b) {
                self.audition(source, x, y, false);
            }
        }
    }

    pub fn place_editor_range(&mut self) {
        let Some((a, b)) = self.editor.range else {
            self.toast(crate::app::ToastKind::Info, "Drag across the waveform in the editor to select audio first");
            return;
        };
        if let Some((source, x, y)) = self.range_to_source(a, b) {
            let hit = Hit {
                source: source.0,
                start: x,
                end: y,
                text: "Slice".to_string(),
                context: None,
                kind: HitKind::Phonemes,
                confidence: 1.0,
                pitch_hz: None,
                word_index: 0,
            };
            self.place_hit_at_playhead(&hit);
        }
    }

    fn detail_editor(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, Sense::hover());
        let Some(track) = self.selected_track.and_then(|t| self.project.track(t)).cloned() else {
            ui.painter().text(
                area.center(),
                Align2::CENTER_CENTER,
                "Double-click a track or clip to edit it here",
                ui_font(14.0),
                TEXT_FAINT,
            );
            return;
        };
        for clip in &track.clips {
            if self.editor.show_spectrogram {
                self.ensure_spectrogram(clip.source);
            }
        }
        self.editor.view.left = area.left();
        self.editor.view.width = area.width();
        if self.editor.last_track != Some(track.id) && self.editor.last_track.is_some() {
            self.editor.needs_fit = true;
            self.editor.range = None;
        }
        self.editor.last_track = Some(track.id);
        if self.editor.needs_fit {
            let start = track.clips.iter().map(|c| c.start).fold(f64::MAX, f64::min);
            let end = track.clips.iter().map(Clip::end).fold(0.0, f64::max);
            if !track.clips.is_empty() {
                self.editor.view.fit(start, end);
            }
            self.editor.needs_fit = false;
        }
        if self.engine.is_playing() && self.clip_drag.is_none() {
            let head = self.playhead;
            self.editor.view.follow(head);
        }

        // Stack from the bottom: phonemes, words, then the waveform takes what remains.
        let mut bottom = area.bottom();
        let phonemes = self.editor.show_phonemes.then(|| {
            let r = Rect::from_min_max(egui::pos2(area.left(), bottom - PHONEMES_H), egui::pos2(area.right(), bottom));
            bottom -= PHONEMES_H;
            r
        });
        let words = self.editor.show_words.then(|| {
            let r = Rect::from_min_max(egui::pos2(area.left(), bottom - WORDS_H), egui::pos2(area.right(), bottom));
            bottom -= WORDS_H;
            r
        });
        let ruler = Rect::from_min_size(area.min, egui::vec2(area.width(), RULER_H));
        let stack = Rect::from_min_max(egui::pos2(area.left(), ruler.bottom()), egui::pos2(area.right(), bottom));
        let (wave, spectrum) = if self.editor.show_spectrogram && stack.height() > 90.0 {
            let split = stack.top() + stack.height() * 0.55;
            (
                Rect::from_min_max(stack.min, egui::pos2(stack.right(), split)),
                Some(Rect::from_min_max(egui::pos2(stack.left(), split + 1.0), stack.max)),
            )
        } else {
            (stack, None)
        };

        self.editor.hovered = ui.rect_contains_pointer(area);
        if self.editor.hovered {
            let (scroll, zoom, cmd, shift, pointer) = ui.input(|i| {
                (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.modifiers.shift, i.pointer.hover_pos())
            });
            let x = pointer.map_or(area.center().x, |p| p.x);
            if zoom != 1.0 {
                self.editor.view.zoom_at(zoom as f64, x);
            } else if cmd && scroll.y != 0.0 {
                self.editor.view.zoom_at((scroll.y as f64 * 0.005).exp(), x);
            } else {
                let dx = if shift { scroll.y } else { scroll.x };
                if dx != 0.0 {
                    self.editor.view.pan(-dx);
                }
            }
        }

        let view = self.editor.view;
        ui.painter().rect_filled(stack, 0.0, BG_0);
        draw_grid(&ui.painter_at(wave), wave, &self.project, &view);
        draw_ruler(ui.painter(), ruler, &self.project, &view);
        let ruler_response = ui.interact(ruler, ui.scope_id().with("ed-ruler"), Sense::click_and_drag());
        if ruler_response.clicked() || ruler_response.dragged() {
            if let Some(p) = ruler_response.interact_pointer_pos() {
                self.seek(view.t(p.x));
            }
        }

        self.editor_wave(ui, wave, &track);
        if let Some(rect) = spectrum {
            self.editor_spectrum(ui, rect, &track);
        }
        if let Some(rect) = words {
            self.editor_chip_lane(ui, rect, &track, false);
        }
        if let Some(rect) = phonemes {
            self.editor_chip_lane(ui, rect, &track, true);
        }

        let painter = ui.painter_at(area);
        if let Some((a, b)) = self.editor.range {
            let r = Rect::from_min_max(egui::pos2(view.x(a), stack.top()), egui::pos2(view.x(b), area.bottom()));
            painter.rect(r, 0.0, alpha(ACCENT, 34), Stroke::new(1.0, alpha(ACCENT, 170)), StrokeKind::Inside);
        }
        if let Some(t) = self.snap_line {
            paint_snap_line(&painter, view.x(t), stack.top(), area.bottom());
        }
        paint_playhead(&painter, view.x(self.playhead), area.top(), area.bottom());
    }

    fn editor_wave(&mut self, ui: &mut Ui, rect: Rect, track: &Track) {
        let view = self.editor.view;
        let response = ui.interact(rect, ui.scope_id().with("ed-wave"), Sense::click_and_drag());
        let pointer = response.hover_pos();
        let (alt, cmd) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
        let zone_at = |pos: egui::Pos2, alt: bool| {
            track.clips.iter().rev().find_map(|c| hit_zone(&view, rect, c, pos, alt).map(|z| (c.id, z)))
        };
        if let Some((_, zone)) = pointer.and_then(|p| zone_at(p, alt)) {
            if zone == Zone::Body {
                ui.ctx().set_cursor_icon(CursorIcon::Text);
                self.hint = "Drag to select audio. E places it on the Sentence track. Click to move the playhead.".to_string();
            } else {
                ui.ctx().set_cursor_icon(zone.cursor());
                self.hint = zone.hint().to_string();
            }
        }
        if response.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                match zone_at(origin, alt) {
                    Some((clip, zone)) if zone != Zone::Body => {
                        let track_index = self.project.tracks.iter().position(|t| t.id == track.id).unwrap_or(0);
                        self.begin_clip_drag(clip, zone, view.t(origin.x), track_index, false);
                    }
                    _ => {
                        let t = view.t(origin.x);
                        self.editor.range_drag_origin = Some(self.snap_time(t, &view, &[], cmd));
                    }
                }
            }
        }
        if response.dragged() {
            if let Some(p) = response.interact_pointer_pos() {
                if self.clip_drag.is_some() {
                    self.update_clip_drag(view.t(p.x), None, &view, cmd);
                } else if let Some(origin) = self.editor.range_drag_origin {
                    let t = self.snap_time(view.t(p.x), &view, &[], cmd);
                    self.editor.range = Some((origin.min(t).max(0.0), origin.max(t)));
                }
            }
        }
        if response.drag_stopped() {
            if self.clip_drag.is_some() {
                self.end_clip_drag();
            }
            if self.editor.range_drag_origin.take().is_some() {
                self.snap_line = None;
                self.audition_editor_range();
            }
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                self.editor.range = None;
                match zone_at(p, false) {
                    Some((clip, _)) => self.selection = vec![clip],
                    None => self.selection.clear(),
                }
                self.seek(view.t(p.x));
            }
        }
        response.context_menu(|ui| self.clip_context_menu(ui));

        let painter = ui.painter_at(rect);
        let muted = track.mute;
        let hover = pointer.and_then(|p| zone_at(p, alt)).map(|h| h.0);
        for clip in &track.clips {
            let r = clip_rect(&view, rect, clip);
            if r.right() < rect.left() || r.left() > rect.right() {
                continue;
            }
            self.paint_clip(
                &painter,
                &view,
                rect,
                track,
                clip,
                PaintOptions {
                    selected: self.selection.contains(&clip.id),
                    hovered: hover == Some(clip.id),
                    show_waveform: true,
                    track_muted: muted,
                },
            );
        }
    }

    fn editor_spectrum(&mut self, ui: &mut Ui, rect: Rect, track: &Track) {
        let view = self.editor.view;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 18, 34));
        for clip in &track.clips {
            let body = clip_rect(&view, rect, clip);
            if body.right() < rect.left() || body.left() > rect.right() {
                continue;
            }
            match self.spectrograms.get(&clip.source) {
                Some(SpectrogramState::Ready { spectrogram, tiles }) => {
                    const TILE: f64 = 2048.0;
                    let seconds_per_column = spectrogram.seconds_per_column;
                    let tile_seconds = TILE * seconds_per_column;
                    let first = (clip.src_in / tile_seconds) as usize;
                    let last = (clip.src_out / tile_seconds) as usize;
                    for i in first..=last.min(tiles.len().saturating_sub(1)) {
                        let tile = &tiles[i];
                        let tile_start = i as f64 * tile_seconds;
                        let tile_len = tile.size()[0] as f64 * seconds_per_column;
                        let a = clip.src_in.max(tile_start);
                        let b = clip.src_out.min(tile_start + tile_len);
                        if b <= a {
                            continue;
                        }
                        let (xa, xb) = (view.x(clip.time_of_source(a)), view.x(clip.time_of_source(b)));
                        let (ua, ub) = (((a - tile_start) / tile_len) as f32, ((b - tile_start) / tile_len) as f32);
                        // A reversed clip has xa > xb, and swapping the UVs mirrors the tile to match.
                        let (x0, x1, u0, u1) = if xa <= xb { (xa, xb, ua, ub) } else { (xb, xa, ub, ua) };
                        let screen = Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom()));
                        let uv = Rect::from_min_max(egui::pos2(u0, 0.0), egui::pos2(u1, 1.0));
                        painter.image(tile.id(), screen, uv, Color32::WHITE);
                    }
                }
                _ => {
                    painter.text(body.left_center().max(rect.left_center()) + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, "Computing spectrogram…", ui_font(11.0), TEXT_FAINT);
                }
            }
        }
    }

    /// Word or phoneme blocks at their positions on the arrangement.
    fn visible_chips(&self, rect: Rect, track: &Track, phonemes: bool) -> Vec<Chip> {
        let view = self.editor.view;
        let mut chips = Vec::new();
        for clip in &track.clips {
            let body = clip_rect(&view, rect, clip);
            if body.right() < rect.left() || body.left() > rect.right() {
                continue;
            }
            let Some(transcript) = self.index.transcript(clip.source.0) else { continue };
            let first = transcript.words.partition_point(|w| w.end <= clip.src_in);
            for (index, word) in transcript.words.iter().enumerate().skip(first) {
                if word.start >= clip.src_out {
                    break;
                }
                let mut push = |text: &str, start: f64, end: f64, kind: HitKind, context: Option<String>| {
                    let (a, b) = (start.max(clip.src_in), end.min(clip.src_out));
                    let (x0, x1) = (view.x(clip.time_of_source(a)), view.x(clip.time_of_source(b)));
                    let (x0, x1) = (x0.min(x1), x0.max(x1));
                    if x1 >= rect.left() && x0 <= rect.right() {
                        chips.push(Chip {
                            hit: Hit {
                                source: clip.source.0,
                                start,
                                end,
                                text: text.to_string(),
                                context,
                                kind,
                                confidence: word.confidence,
                                pitch_hz: word.pitch_hz,
                                word_index: index,
                            },
                            x0,
                            x1,
                        });
                    }
                };
                if phonemes {
                    for p in &word.phonemes {
                        push(&p.symbol, p.start, p.end, HitKind::Phonemes, Some(word.text.clone()));
                    }
                } else {
                    push(&word.text, word.start, word.end, HitKind::Exact, None);
                }
            }
        }
        chips
    }

    fn editor_chip_lane(&mut self, ui: &mut Ui, rect: Rect, track: &Track, phonemes: bool) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BG_1);
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, LINE));
        let chips = self.visible_chips(rect, track, phonemes);
        if chips.is_empty() {
            if !phonemes && !track.clips.is_empty() {
                painter.text(rect.left_center() + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, "No words here yet. Transcribe this track to find them.", ui_font(11.0), TEXT_FAINT);
            }
            return;
        }
        let id = ui.scope_id().with(if phonemes { "ed-ph" } else { "ed-wd" });
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let pointer = response.hover_pos();
        let chip_at = |x: f32| chips.iter().find(|c| x >= c.x0 && x < c.x1.max(c.x0 + 2.0));
        let hovered = pointer.and_then(|p| chip_at(p.x));
        if let Some(c) = hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            self.hint = match &c.hit.context {
                Some(word) => format!("/{}/ in \"{word}\". Click to hear, double-click to place, or drag to the timeline.", c.hit.text),
                None => format!("\"{}\". Click to hear, double-click to place, or drag to the timeline.", c.hit.text),
            };
        }
        let mut clicked: Option<Hit> = None;
        let mut double: Option<Hit> = None;
        if response.clicked() {
            clicked = response.interact_pointer_pos().and_then(|p| chip_at(p.x)).map(|c| c.hit.clone());
        }
        if response.double_clicked() {
            double = response.interact_pointer_pos().and_then(|p| chip_at(p.x)).map(|c| c.hit.clone());
        }
        if response.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                self.hit_drag = chip_at(origin.x).map(|c| c.hit.clone());
            }
        }

        let selected_word = self.editor.selected_word;
        for chip in &chips {
            let r = Rect::from_min_max(egui::pos2(chip.x0 + 0.5, rect.top() + 3.0), egui::pos2(chip.x1 - 0.5, rect.bottom() - 2.0));
            let is_hover = hovered.is_some_and(|h| h.hit.start == chip.hit.start && h.hit.end == chip.hit.end);
            let is_selected = !phonemes && selected_word == Some((chip.hit.source, chip.hit.word_index));
            let base = track_color(track.color);
            let fill = if is_selected {
                ACCENT
            } else if is_hover {
                mix(BG_3, base, 0.6)
            } else if phonemes {
                phoneme_color(&chip.hit.text)
            } else {
                // Low-confidence words are dimmer, a cue that the transcript may be wrong here.
                mix(BG_2, base, 0.1 + 0.3 * chip.hit.confidence)
            };
            painter.rect_filled(r, CornerRadius::same(3), fill);
            if r.width() > 12.0 {
                let color = if is_selected { BG_0 } else { TEXT };
                let font = if phonemes { mono(9.5) } else { ui_font(11.5) };
                let inner = painter.with_clip_rect(r.intersect(rect));
                // Centred when it fits. Otherwise the start of the word is what identifies it.
                let galley = inner.layout_no_wrap(chip.hit.text.clone(), font, color);
                let x = if galley.size().x < r.width() - 2.0 { r.center().x - galley.size().x * 0.5 } else { r.left() + 2.0 };
                inner.galley(egui::pos2(x, r.center().y - galley.size().y * 0.5), galley, color);
            }
        }
        if let Some(hit) = clicked {
            if !phonemes {
                self.editor.selected_word = Some((hit.source, hit.word_index));
            }
            self.audition_hit(&hit);
        }
        if let Some(hit) = double {
            self.place_hit_at_playhead(&hit);
        }
    }
}

fn phoneme_color(symbol: &str) -> Color32 {
    use mp_analysis::g2p::{class_of, PhonemeClass::*};
    match class_of(symbol) {
        Vowel => Color32::from_rgb(66, 74, 110),
        Glide => Color32::from_rgb(58, 88, 100),
        Nasal => Color32::from_rgb(70, 80, 80),
        VoicedFricative => Color32::from_rgb(96, 76, 64),
        UnvoicedFricative => Color32::from_rgb(112, 74, 60),
        VoicedStop => Color32::from_rgb(88, 62, 88),
        UnvoicedStop => Color32::from_rgb(102, 60, 84),
        Affricate => Color32::from_rgb(108, 68, 72),
    }
}
