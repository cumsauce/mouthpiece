//! Trying alternatives for a piece of audio, in place. Each one is swapped in live and heard in
//! context, with the words and music around it, so the comparison is the thing itself.

use crate::app::App;
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::timeline::format_time;
use eframe::egui::{self, Align2, Key, Modifiers, Rect, Sense, Ui};
use mp_analysis::search::{SoundCandidate, SoundWanted};
use mp_analysis::Word;
use mp_model::{ClipId, Project, SourceId, TrackId};

pub struct Trial {
    pub clip: ClipId,
    pub track: TrackId,
    /// The project before trying anything. Cancelling goes back to this.
    snapshot: Project,
    pub query: String,
    pub candidates: Vec<SoundCandidate>,
    /// The one playing, or `None` for what was there before.
    pub current: Option<usize>,
    /// Ones marked to come back to, which stay at the top of the list.
    pub pinned: Vec<(u64, u64)>,
    pub with_music: bool,
    pub keep_timing: bool,
    wanted: SoundWanted,
    in_word: Option<String>,
}

const ROW_H: f32 = 44.0;
const LIST_LIMIT: usize = 300;

fn pin_key(c: &SoundCandidate) -> (u64, u64) {
    (c.hit.source, c.hit.start.to_bits())
}

impl App {
    /// Starts trying alternatives for the selected clip, or a phoneme selected in the editor.
    pub fn open_trial(&mut self) {
        if let Some((clip, hit)) = self.editor.selected_sound.clone() {
            if self.project.clip(clip).is_some() {
                self.open_sound_in_clip(clip, &hit);
                return;
            }
        }
        // A whole word was selected. Trying it as a whole only makes sense if it has one label.
        let single = match self.selection.as_slice() {
            [one] => Some(*one),
            many if !many.is_empty() => {
                self.toast(crate::app::ToastKind::Info, "Click again to select one piece of the word, or click a phoneme in the editor");
                return;
            }
            _ => None,
        };
        let Some(clip) = single else {
            self.toast(crate::app::ToastKind::Info, "Select a clip, or click a phoneme in the editor, then press F");
            return;
        };
        let snapshot = self.project.clone();
        self.start_trial(clip, snapshot, None);
    }

    /// Cuts one phoneme out of a placed word, so that it can be replaced on its own.
    fn open_sound_in_clip(&mut self, clip: ClipId, hit: &mp_analysis::search::Hit) {
        let snapshot = self.project.clone();
        let Some(c) = self.project.clip(clip).cloned() else { return };
        let word: Option<Word> = self.index.transcript(hit.source).and_then(|t| t.words.get(hit.word_index).cloned());
        let (x, y) = (c.time_of_source(hit.start), c.time_of_source(hit.end));
        let (a, b) = (x.min(y).max(c.start), x.max(y).min(c.end()));
        let name = c.word_name.clone().or_else(|| word.as_ref().map(|w| w.text.clone())).or(c.label.clone());

        // Split on both sides. Whatever's left over either side keeps its own sounds as a label.
        let middle = self.project.split_clip(clip, a).unwrap_or(clip);
        let right = self.project.split_clip(middle, b);
        let left = (middle != clip).then_some(clip);
        if let Some(w) = &word {
            let at = w.phonemes.iter().position(|p| p.start == hit.start).unwrap_or(0);
            let count = hit.text.split_whitespace().count().max(1);
            let join = |range: std::ops::Range<usize>| w.phonemes.get(range).map_or(String::new(), |p| p.iter().map(|x| x.symbol.as_str()).collect::<Vec<_>>().join(" "));
            let (before, after) = (join(0..at), join(at + count..w.phonemes.len()));
            if let Some(c) = left.and_then(|id| self.project.clip_mut(id)) {
                c.label = Some(before);
            }
            if let Some(c) = right.and_then(|id| self.project.clip_mut(id)) {
                c.label = Some(after);
            }
        }
        if let Some(c) = self.project.clip_mut(middle) {
            c.label = Some(hit.text.clone());
        }
        let mut pieces = vec![middle];
        pieces.extend(left);
        pieces.extend(right);
        self.project.group_clips(&pieces, name.clone());
        self.editor.selected_sound = None;
        self.selection = vec![middle];
        self.start_trial(middle, snapshot, name);
    }

    fn start_trial(&mut self, clip: ClipId, snapshot: Project, in_word: Option<String>) {
        let Some(c) = self.project.clip(clip).cloned() else { return };
        let Some(track) = self.project.track_of_clip(clip) else { return };
        let label = c.label.clone().unwrap_or_default();
        let tokens: Vec<String> = label.split_whitespace().map(str::to_lowercase).collect();
        let is_sound = !tokens.is_empty() && tokens.iter().all(|t| mp_analysis::g2p::ALL_PHONEMES.contains(&t.as_str()));
        let query = if is_sound { format!("/{label}/") } else { label };

        // What's around this piece tells us which sounds will join to it naturally.
        let sound_at = |id: ClipId, last: bool| -> Option<String> {
            let label = self.project.clip(id)?.label.clone()?;
            let mut tokens = label.split_whitespace().map(str::to_lowercase).filter(|t| mp_analysis::g2p::ALL_PHONEMES.contains(&t.as_str()));
            if last { tokens.last() } else { tokens.next() }
        };
        let members = self.project.group_members(clip);
        let position = members.iter().position(|m| *m == clip).unwrap_or(0);
        let before = position.checked_sub(1).and_then(|i| sound_at(members[i], true));
        let after = members.get(position + 1).and_then(|m| sound_at(*m, false));
        let neighbour = position.checked_sub(1).map(|i| members[i]).or(members.get(position + 1).copied());
        let pitch_clip = neighbour.and_then(|m| self.project.clip(m).cloned()).unwrap_or_else(|| c.clone());
        let wanted = SoundWanted { symbols: tokens, before, after, pitch_hz: self.clip_pitch(pitch_clip.source, pitch_clip.src_in, pitch_clip.src_out), length: Some(c.src_len()) };

        self.trial = Some(Trial {
            clip,
            track,
            snapshot,
            query,
            candidates: Vec::new(),
            current: None,
            pinned: Vec::new(),
            with_music: true,
            keep_timing: false,
            wanted,
            in_word,
        });
        self.update_trial_candidates();
        self.show_say = true;
    }

    fn update_trial_candidates(&mut self) {
        let Some(trial) = &self.trial else { return };
        let query = trial.query.trim();
        let candidates: Vec<SoundCandidate> = if let Some(inner) = query.strip_prefix('/') {
            let symbols: Vec<String> = inner.trim_end_matches('/').split_whitespace().map(str::to_lowercase).collect();
            let wanted = SoundWanted { symbols, ..trial.wanted.clone() };
            self.index.sound_candidates(&wanted)
        } else if query.is_empty() {
            Vec::new()
        } else {
            self.index.search(query, LIST_LIMIT).into_iter().map(|hit| SoundCandidate { hit, same_before: false, same_after: false }).collect()
        };
        // What's already in place has its own row at the top, so it isn't offered again.
        const SAME: f64 = 0.03;
        let original = self.trial.as_ref().and_then(|t| t.snapshot.clip(t.clip).or(self.project.clip(t.clip))).map(|c| (c.source.0, c.src_in));
        let mut candidates = candidates;
        if let Some((source, at)) = original {
            candidates.retain(|c| c.hit.source != source || (c.hit.start - at).abs() > SAME);
        }
        if let Some(trial) = &mut self.trial {
            trial.candidates = candidates;
            trial.candidates.truncate(LIST_LIMIT);
            trial.current = None;
            sort_pinned(trial);
        }
    }

    /// Swaps a candidate in on the timeline, then plays it in context.
    fn try_candidate(&mut self, index: Option<usize>) {
        let Some(trial) = &mut self.trial else { return };
        trial.current = index;
        let (clip, keep_timing) = (trial.clip, trial.keep_timing);
        let original = trial.snapshot.clip(clip).cloned();
        let hit = index.and_then(|i| trial.candidates.get(i)).map(|c| c.hit.clone());
        match (hit, original) {
            (Some(hit), original) => {
                // A piece made by splitting isn't in the snapshot, so its own length is the slot.
                let slot = self.project.clip(clip).map_or(0.0, |c| c.length());
                if let Some((a, b)) = App::padded_range(&self.project, &hit) {
                    self.project.swap_clip_audio(clip, SourceId(hit.source), a, b);
                    let stretch = if keep_timing { (slot / (b - a).max(1e-3)).clamp(0.5, 2.0) } else { original.map_or(1.0, |o| o.stretch) };
                    self.project.set_stretch(clip, stretch);
                }
            }
            (None, Some(original)) => {
                if let Some(c) = self.project.clip_mut(clip) {
                    *c = original;
                }
            }
            (None, None) => {}
        }
        self.project.update_auto_crossfades();
        self.needs_compile = true;
        self.hear_trial();
    }

    /// Plays from a little before the clip to a little after it. While the transport runs, the swap
    /// is heard by itself, so this only plays when stopped.
    pub fn hear_trial(&mut self) {
        const BEFORE: f64 = 1.0;
        const AFTER: f64 = 0.6;
        if self.engine.is_playing() {
            return;
        }
        let Some(trial) = &self.trial else { return };
        let Some((a, b)) = self.project.clip(trial.clip).map(|c| (c.start, c.end())) else { return };
        let mut context = self.project.clone();
        if !trial.with_music {
            let track = trial.track;
            context.tracks.iter_mut().for_each(|t| {
                t.solo = false;
                t.mute = t.id != track;
            });
        }
        let buffer = self.render_context(&context, (a - BEFORE).max(0.0), b + AFTER);
        self.audition_buffer(buffer);
    }

    pub fn keep_trial(&mut self) {
        if let Some(trial) = self.trial.take() {
            if trial.snapshot != self.project {
                self.history.push("Try a sound", trial.snapshot);
            }
            self.flash_clips(&[trial.clip]);
        }
    }

    pub fn cancel_trial(&mut self) {
        if let Some(trial) = self.trial.take() {
            self.project = trial.snapshot;
            self.needs_compile = true;
            self.selection.retain(|c| self.project.clip(*c).is_some());
            self.engine.audition(None);
        }
    }

    /// Handles trying with the keyboard. These keys mean something else elsewhere, so they go first.
    pub fn trial_keys(&mut self, ctx: &egui::Context) {
        let Some(trial) = &self.trial else { return };
        let count = trial.candidates.len();
        let current = trial.current;
        if self.project.clip(trial.clip).is_none() {
            self.trial = None;
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down && count > 0 {
            self.try_candidate(Some(current.map_or(0, |c| (c + 1).min(count - 1))));
        }
        if up {
            // The top of the list goes back to what was there before.
            self.try_candidate(current.and_then(|c| c.checked_sub(1)));
        }
        if enter {
            self.keep_trial();
            return;
        }
        if escape {
            self.cancel_trial();
            return;
        }
        if typing {
            return;
        }
        let (space, compare, pin) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::Space), i.consume_key(Modifiers::NONE, Key::A), i.consume_key(Modifiers::NONE, Key::P))
        });
        if space {
            self.hear_trial();
        }
        if compare {
            // Between what's being tried and what was there, then back again.
            let next = if current.is_some() { self.say.trial_last = current; None } else { self.say.trial_last };
            self.try_candidate(next);
        }
        if pin {
            self.toggle_pin(current);
        }
    }

    fn toggle_pin(&mut self, index: Option<usize>) {
        let Some(trial) = &mut self.trial else { return };
        let Some(key) = index.and_then(|i| trial.candidates.get(i)).map(pin_key) else { return };
        if let Some(at) = trial.pinned.iter().position(|p| *p == key) {
            trial.pinned.remove(at);
        } else {
            trial.pinned.push(key);
        }
        sort_pinned(trial);
        trial.current = trial.candidates.iter().position(|c| pin_key(c) == key);
    }

    pub fn trial_panel(&mut self, ui: &mut Ui) {
        let Some(trial) = &self.trial else { return };
        let what = trial.query.clone();
        let in_word = trial.in_word.clone();
        ui.label(title(format!("Try {}", truncate(&what, 18))));
        ui.label(caption(match in_word {
            Some(word) => format!("in \"{word}\". Each one plays in place."),
            None => "Each one plays in place.".to_string(),
        }));
        ui.add_space(SP_2);

        let Some(trial) = &mut self.trial else { return };
        let mut query = trial.query.clone();
        let changed = text_field(ui, &mut query, "A sound like /k ae/, or a word", ui.available_width()).changed();
        trial.query = query;
        ui.add_space(SP_1);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_3;
            switch(ui, &mut trial.with_music, "With music");
            switch(ui, &mut trial.keep_timing, "Keep timing");
        });
        ui.add_space(SP_1);
        if changed {
            self.update_trial_candidates();
        }

        const FOOTER: f32 = 84.0;
        let list_height = (ui.available_height() - FOOTER).max(ROW_H);
        ui.allocate_ui(egui::vec2(ui.available_width(), list_height), |ui| {
            ui.set_min_height(list_height);
            self.trial_list(ui);
        });

        ui.add_space(SP_1);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_2;
            hint_row(ui, "↑↓", "try");
            hint_row(ui, "A", "compare");
            hint_row(ui, "P", "pin");
        });
        ui.add_space(SP_1);
        ui.horizontal(|ui| {
            if ui.add(Button::secondary("Cancel").tip("Put it all back", Some("Esc"))).clicked() {
                self.cancel_trial();
            }
            if ui.add(Button::primary("Keep this").tip("Keep what's playing", Some("Return"))).clicked() {
                self.keep_trial();
            }
        });
    }

    fn trial_list(&mut self, ui: &mut Ui) {
        let Some(trial) = &self.trial else { return };
        let count = trial.candidates.len();
        let current = trial.current;
        if count == 0 {
            ui.add_space(SP_2);
            ui.label(caption("Nothing matches. Try a shorter sound or a different word."));
            return;
        }
        ui.label(caption(format!("{count} found, best fit first")));
        ui.add_space(SP_1);
        egui::ScrollArea::vertical().id_salt("trial").auto_shrink(false).show_rows(ui, ROW_H, count + 1, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                if row == 0 {
                    self.trial_original_row(ui, current.is_none());
                } else {
                    self.trial_row(ui, row - 1, current == Some(row - 1));
                }
            }
        });
    }

    fn trial_original_row(&mut self, ui: &mut Ui, selected: bool) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
        paint_row_background(ui, rect, selected, response.hovered());
        ui.painter().text(rect.left_center() + egui::vec2(SP_2, 0.0), Align2::LEFT_CENTER, "What was there", font_label(), TEXT);
        if response.clicked() {
            self.try_candidate(None);
        }
    }

    fn trial_row(&mut self, ui: &mut Ui, index: usize, selected: bool) {
        let Some(trial) = &self.trial else { return };
        let Some(candidate) = trial.candidates.get(index).cloned() else { return };
        let pinned = trial.pinned.contains(&pin_key(&candidate));
        let hit = &candidate.hit;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
        paint_row_background(ui, rect, selected, response.hovered());
        let painter = ui.painter_at(rect);
        let x = rect.left() + SP_2;
        let word = hit.context.clone().unwrap_or_else(|| hit.text.clone());
        painter.text(egui::pos2(x, rect.top() + 6.0), Align2::LEFT_TOP, truncate(&word, 22), font_body(), TEXT);
        let mut info = format_time(hit.start);
        if let Some(hz) = hit.pitch_hz {
            info.push_str(&format!(" · {}", mp_dsp::midi_name(mp_dsp::hz_to_midi(hz))));
        }
        info.push_str(&format!(" · {:.2}s", hit.end - hit.start));
        if candidate.same_before || candidate.same_after {
            info.push_str(" · joins well");
        }
        painter.text(egui::pos2(x, rect.top() + 25.0), Align2::LEFT_TOP, info, font_caption(), if candidate.same_before || candidate.same_after { TEXT_2 } else { TEXT_3 });
        if pinned {
            draw_icon(&painter, Rect::from_center_size(rect.right_center() - egui::vec2(SP_3, 0.0), egui::vec2(10.0, 10.0)), Icon::Bookmark, TEXT);
        }
        if response.clicked() {
            self.try_candidate(Some(index));
        }
        if response.double_clicked() {
            self.keep_trial();
        }
        if selected && ui.input(|i| i.key_down(Key::ArrowDown) || i.key_down(Key::ArrowUp)) {
            ui.scroll_to_rect(rect, None);
        }
    }

    /// Renders a stretch of a project through the real engine, with any stretched clips ready.
    pub fn render_context(&self, project: &Project, from: f64, to: f64) -> mp_dsp::Stereo {
        use mp_engine::GraphCompiler;
        let sr = self.sample_rate;
        drop(GraphCompiler::default().compile(project, &self.bank, &self.cache, sr));
        self.cache.wait_idle();
        let graph = GraphCompiler::default().compile(project, &self.bank, &self.cache, sr);
        let mut out = mp_dsp::Stereo::default();
        mp_engine::render_offline(
            graph,
            sr as f32,
            (from * sr as f64) as u64,
            (to * sr as f64) as u64,
            0,
            |l, r| {
                out.l.extend_from_slice(l);
                out.r.extend_from_slice(r);
            },
            |_| true,
        );
        out
    }
}

fn sort_pinned(trial: &mut Trial) {
    let pinned = trial.pinned.clone();
    trial.candidates.sort_by_key(|c| !pinned.contains(&pin_key(c)));
}

fn paint_row_background(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    if selected {
        ui.painter().rect_filled(rect, R_S, CONTROL);
        ui.painter().rect_filled(Rect::from_min_size(rect.min, egui::vec2(SP_HALF, rect.height())), 0.0, ACCENT);
    } else if hovered {
        ui.painter().rect_filled(rect, R_S, RAISED);
    }
}
