//! Changing part of a song. Select some lyrics, edit the words, and only the words that changed get
//! new audio, as a change on the same track. No extra tracks, and the recording stays as it was.

use crate::app::{App, ToastAction, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::say::SayTab;
use crate::panels::timeline::{format_time, TimeView};
use eframe::egui::{self, Align2, Rect, Stroke, Ui};
use mp_analysis::normalize_word;
use mp_analysis::sentence::{self, SentencePart};
use mp_model::{ClipId, PatchId, Project, SourceId, TrackId};

/// A part of the rewritten line that differs, and the stretch of the song it replaces.
pub struct RewriteRun {
    pub parts: std::ops::Range<usize>,
    pub start: f64,
    pub end: f64,
    pub old: std::ops::Range<usize>,
}

/// The lyrics being rewritten.
pub struct Rewrite {
    pub track: TrackId,
    pub source: SourceId,
    pub first_word: usize,
    pub last_word: usize,
    pub before_text: String,
    pub before_words: Vec<String>,
    /// Each original word's place on the arrangement, and in the source.
    pub times: Vec<(f64, f64)>,
    pub source_times: Vec<(f64, f64)>,
    /// The change this adds a version to, if it already exists.
    pub patch: Option<PatchId>,
    pub runs: Vec<RewriteRun>,
    pub fit: bool,
}

/// Speeds a run of clips up or down this far at most to fit the words they replace. Past this, it
/// sounds worse than running long.
const FIT_RANGE: (f64, f64) = (0.6, 1.5);

impl App {
    /// Starts rewriting: the selected change if there is one, otherwise the selected words.
    pub fn open_rewrite(&mut self) {
        let patch = self.selection.first().and_then(|c| self.project.clip(*c)).and_then(|c| c.patch).or(self.selected_patch);
        let range = match patch.and_then(|id| self.project.patch(id)) {
            Some(p) => Some((p.source, p.first_word, p.last_word)),
            None => self.selected_word_range(),
        };
        let Some((source, a, b)) = range else {
            self.show_say = true;
            self.say.tab = SayTab::Lyrics;
            self.toast(ToastKind::Info, "Select the words to change: click the first, then Shift-click the last");
            return;
        };
        let Some(mut rewrite) = self.locate_words(source, a, b) else {
            self.toast(ToastKind::Info, "Those words aren't on the timeline");
            return;
        };
        rewrite.patch = patch;
        let text = patch.and_then(|id| self.project.patch(id)).map_or(rewrite.before_text.clone(), |p| p.current_text().to_string());
        self.say.text = if text.is_empty() { rewrite.before_text.clone() } else { text };
        self.say.rewrite = Some(rewrite);
        self.say.tab = SayTab::Say;
        self.show_say = true;
        self.say.dirty = true;
        self.say.focus_input = true;
    }

    /// The transcript words selected in the lyrics or editor, or under the editor's range selection.
    fn selected_word_range(&self) -> Option<(SourceId, usize, usize)> {
        if let Some(words) = self.say.word_selection {
            return Some(words);
        }
        let (a, b) = self.editor.range?;
        let track = self.project.track(self.selected_track?)?;
        let mid = (a + b) * 0.5;
        let clip = track.clips.iter().rev().find(|c| c.start <= mid && mid < c.end())?;
        let (x, y) = (clip.source_time_at(a.max(clip.start)), clip.source_time_at(b.min(clip.end())));
        let (x, y) = (x.min(y), x.max(y));
        let transcript = self.index.transcript(clip.source.0)?;
        let first = transcript.words.iter().position(|w| w.end > x)?;
        let last = transcript.words.iter().rposition(|w| w.start < y)?;
        (first <= last).then_some((clip.source, first, last))
    }

    /// Finds where words are on the arrangement, preferring the selected track.
    fn locate_words(&self, source: SourceId, a: usize, b: usize) -> Option<Rewrite> {
        let transcript = self.index.transcript(source.0)?;
        let words = transcript.words.get(a..=b)?;
        const SLACK: f64 = 0.05;
        let (from, to) = (words.first()?.start, words.last()?.end);
        let holds = |c: &&mp_model::Clip| c.patch.is_none() && c.source == source && c.src_in - SLACK <= from && to <= c.src_out + SLACK;
        let selected = self.selected_track.and_then(|t| self.project.track(t)).filter(|t| t.clips.iter().any(|c| holds(&c)));
        let track = selected.or_else(|| self.project.tracks.iter().find(|t| t.clips.iter().any(|c| holds(&c))))?;
        let clip = track.clips.iter().find(holds)?;
        let place = |s: f64, e: f64| {
            let (x, y) = (clip.time_of_source(s), clip.time_of_source(e));
            (x.min(y), x.max(y))
        };
        Some(Rewrite {
            track: track.id,
            source,
            first_word: a,
            last_word: b,
            before_text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            before_words: words.iter().map(|w| normalize_word(&w.text)).collect(),
            times: words.iter().map(|w| place(w.start, w.end)).collect(),
            source_times: words.iter().map(|w| (w.start, w.end)).collect(),
            patch: None,
            runs: Vec::new(),
            fit: true,
        })
    }

    /// Plans only the words that differ from the recording.
    pub fn plan_rewrite(&mut self) {
        let Some(mut rewrite) = self.say.rewrite.take() else { return };
        let tokens: Vec<String> = self.say.text.split_whitespace().map(normalize_word).filter(|w| !w.is_empty()).collect();
        let mut plan: Vec<SentencePart> = Vec::new();
        rewrite.runs.clear();
        for change in sentence::diff_words(&rewrite.before_words, &tokens) {
            let mut parts = sentence::plan(&tokens[change.new.clone()].join(" "), &self.index);
            self.mark_made_words(&mut parts);
            let (start, end) = if change.old.is_empty() {
                let at = change.old.start.checked_sub(1).map_or_else(|| rewrite.times.first().map_or(0.0, |t| t.0), |i| rewrite.times[i].1);
                (at, at)
            } else {
                (rewrite.times[change.old.start].0, rewrite.times[change.old.end - 1].1)
            };
            let from = plan.len();
            plan.extend(parts);
            rewrite.runs.push(RewriteRun { parts: from..plan.len(), start, end, old: change.old });
        }
        self.say.plan = plan;
        self.say.rewrite = Some(rewrite);
        self.say.focus = self.say.focus.min(self.say.plan.len().saturating_sub(1));
    }

    /// Builds the change in a project. Used on the real one, and on a copy to preview it.
    fn apply_rewrite(p: &mut Project, rewrite: &Rewrite, plan: &[SentencePart], text: &str) -> Option<(PatchId, Vec<ClipId>)> {
        let first = rewrite.runs.first()?;
        let start = rewrite.runs.iter().map(|r| r.start).fold(first.start, f64::min);
        let end = rewrite.runs.iter().map(|r| r.end).fold(first.end, f64::max);
        let id = rewrite.patch.unwrap_or_else(|| {
            p.create_patch(rewrite.track, (start, end), rewrite.before_text.clone(), (rewrite.source, rewrite.first_word, rewrite.last_word))
        });
        if let Some(patch) = p.patches.iter_mut().find(|x| x.id == id) {
            patch.start = patch.start.min(start);
            patch.end = patch.end.max(end);
        }
        let mut all = Vec::new();
        for (i, run) in rewrite.runs.iter().enumerate() {
            let mut cursor = run.start;
            let mut clips = Vec::new();
            for part in &plan[run.parts.clone()] {
                let (placed, after) = App::lay_out_part(p, part, rewrite.track, cursor);
                cursor = after;
                clips.extend(placed);
            }
            let (slot, span) = (run.end - run.start, cursor - run.start);
            if rewrite.fit && slot > 0.02 && span > 0.0 {
                let ratio = (slot / span).clamp(FIT_RANGE.0, FIT_RANGE.1);
                for clip in &clips {
                    let Some((at, stretch)) = p.clip(*clip).map(|c| (c.start, c.stretch)) else { continue };
                    p.set_stretch(*clip, stretch * ratio);
                    if let Some(c) = p.clip_mut(*clip) {
                        c.start = run.start + (at - run.start) * ratio;
                    }
                }
            }
            all.extend(clips);
            // Words kept between two changes are hidden with the rest of the range, so they play from a clip.
            if let Some(next) = rewrite.runs.get(i + 1) {
                if next.old.start > run.old.end {
                    let (a, b) = (rewrite.source_times[run.old.end].0, rewrite.source_times[next.old.start - 1].1);
                    let label = rewrite.before_words[run.old.end..next.old.start].join(" ");
                    let at = rewrite.times[run.old.end].0;
                    all.extend(p.add_slice(rewrite.track, rewrite.source, at, a, b, Some(label)));
                }
            }
        }
        p.add_patch_version(id, text.to_string(), &all);
        p.update_auto_crossfades();
        Some((id, all))
    }

    /// Hears the change with the music around it, without making it.
    pub fn preview_rewrite(&mut self) {
        if self.say.dirty {
            self.replan();
        }
        let Some(rewrite) = &self.say.rewrite else { return };
        let mut trial = self.project.clone();
        let Some((id, _)) = App::apply_rewrite(&mut trial, rewrite, &self.say.plan, self.say.text.trim()) else {
            self.toast(ToastKind::Info, "These are the words already there. Change some first.");
            return;
        };
        let Some((a, b)) = trial.patch(id).map(|p| trial.patch_range(p)) else { return };
        let buffer = self.render_context(&trial, (a - 1.2).max(0.0), b + 0.8);
        self.audition_buffer(buffer);
    }

    pub fn commit_rewrite(&mut self) {
        if self.say.dirty {
            self.replan();
        }
        let Some(rewrite) = self.say.rewrite.take() else { return };
        let plan = self.say.plan.clone();
        let text = self.say.text.trim().to_string();
        let made = self.edit("Change words", |p| App::apply_rewrite(p, &rewrite, &plan, &text));
        let Some((id, clips)) = made else {
            self.say.rewrite = Some(rewrite);
            self.toast(ToastKind::Info, "These are the words already there. Change some first.");
            return;
        };
        self.selection = clips.clone();
        self.selected_track = Some(rewrite.track);
        self.selected_patch = Some(id);
        self.after_place(&clips, rewrite.track);
        if let Some(start) = self.project.patch(id).map(|p| p.start) {
            self.seek((start - PRE_ROLL).max(0.0));
        }
        self.say.text.clear();
        self.say.word_selection = None;
        self.say.dirty = true;
        // Next comes listening, so the keys should go to the transport and A, not to the text box.
        self.say.focus_input = false;
        self.ctx.memory_mut(|m| m.surrender_focus(crate::panels::say::input_id()));
        self.editor.range = None;
        self.editor.needs_fit = true;
        self.toast_with(ToastKind::Good, "Changed. Space to hear it, A to compare with the original.", ToastAction::Undo);
    }

    pub fn cancel_rewrite(&mut self) {
        self.say.rewrite = None;
        self.say.text.clear();
        self.say.dirty = true;
    }

    pub fn rewrite_header(&mut self, ui: &mut Ui) {
        let Some(rewrite) = &self.say.rewrite else { return };
        let before = rewrite.before_text.clone();
        let adding = rewrite.patch.is_some();
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(caption(if adding { "A new version of" } else { "Changing" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(IconButton::new(Icon::Close, "Cancel").shortcut("Esc").small()).clicked() {
                        self.cancel_rewrite();
                    }
                });
            });
            ui.label(strong(truncate(&before, 60)));
        });
        ui.add_space(SP_2);
    }

    pub fn rewrite_footer(&mut self, ui: &mut Ui) {
        let Some(mut fit) = self.say.rewrite.as_ref().map(|r| r.fit) else { return };
        ui.add_space(SP_1);
        if switch(ui, &mut fit, "Fit to the original timing").changed() {
            if let Some(r) = &mut self.say.rewrite {
                r.fit = fit;
            }
        }
        ui.add_space(SP_1);
        let changed = self.say.rewrite.as_ref().is_some_and(|r| !r.runs.is_empty());
        ui.horizontal(|ui| {
            if ui.add(Button::secondary("Hear in place").enabled(changed).tip("Hear it with the music, without changing anything", Some("⇧Return"))).clicked() {
                self.preview_rewrite();
            }
            if ui.add(Button::primary("Make change").enabled(changed).tip("Keeps the original underneath", Some("Return"))).clicked() {
                self.commit_rewrite();
            }
        });
    }

    /// The selected words in the lyrics, and what can be done with them.
    pub fn word_selection_bar(&mut self, ui: &mut Ui) {
        let Some((source, a, b)) = self.say.word_selection else {
            ui.label(caption("Click a word to hear it. Shift-click to select a phrase and change it."));
            return;
        };
        let Some(words) = self.index.transcript(source.0).and_then(|t| t.words.get(a..=b)) else { return };
        let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        let (from, to) = (words[0].start, words[words.len() - 1].end);
        ui.label(strong(truncate(&text, 60)));
        ui.horizontal(|ui| {
            if ui.add(Button::primary("Change these words").tip("Rewrite them, keeping the original underneath", Some("C"))).clicked() {
                self.open_rewrite();
            }
            if ui.add(Button::secondary("Hear")).clicked() {
                self.audition(source, from, to, false);
            }
        });
    }

    /// Every change in the project, to compare and tidy up.
    pub fn changes_tab(&mut self, ui: &mut Ui) {
        if self.project.patches.is_empty() {
            let rect = ui.available_rect_before_wrap();
            empty_state(ui, rect, "No changes yet", "In Lyrics, select some words and press C.", None);
            return;
        }
        let mut original = self.project.bypass_patches;
        if switch(ui, &mut original, "Hear the original song").changed() {
            self.run(crate::actions::Action::HearOriginal);
        }
        ui.add_space(SP_2);
        let mut ids: Vec<(f64, PatchId)> = self.project.patches.iter().map(|p| (p.start, p.id)).collect();
        ids.sort_by(|a, b| a.0.total_cmp(&b.0));
        egui::ScrollArea::vertical().id_salt("changes").auto_shrink(false).show(ui, |ui| {
            for (_, id) in ids {
                self.change_card(ui, id);
                ui.add_space(SP_1);
            }
        });
    }

    fn change_card(&mut self, ui: &mut Ui, id: PatchId) {
        let Some(patch) = self.project.patch(id).cloned() else { return };
        let frame = if self.selected_patch == Some(id) { card_frame().fill(CONTROL) } else { card_frame() };
        let response = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(num(format_time(patch.start)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = SP_HALF;
                    if ui.add(IconButton::new(Icon::Trash, "Remove, putting the recording back").small()).clicked() {
                        self.edit("Remove change", |p| p.delete_patch(id));
                        self.selection.retain(|c| self.project.clip(*c).is_some());
                        self.selected_patch = None;
                    }
                    if ui.add(IconButton::new(Icon::Plus, "Try another version").shortcut("C").small()).clicked() {
                        self.selected_patch = Some(id);
                        self.selection.clear();
                        self.open_rewrite();
                    }
                    if ui.add(IconButton::new(Icon::Loop, "Loop this with the music around it").small()).clicked() {
                        self.hear_change(id);
                    }
                });
            });
            ui.label(caption(format!("was: {}", truncate(&patch.before, 44))));
            ui.label(strong(truncate(patch.current_text(), 44)));
            ui.add_space(SP_1);
            let labels: Vec<String> = (0..patch.versions.len()).map(|i| (i + 1).to_string()).collect();
            let mut options: Vec<(Option<usize>, &str)> = vec![(None, "Original")];
            options.extend(labels.iter().enumerate().map(|(i, l)| (Some(i), l.as_str())));
            let mut active = patch.active;
            if segmented(ui, &mut active, &options) {
                self.selected_patch = Some(id);
                self.switch_patch(id, active);
            }
        });
        if response.response.interact(egui::Sense::click()).clicked() {
            self.selected_patch = Some(id);
        }
    }

    pub fn switch_patch(&mut self, id: PatchId, version: Option<usize>) {
        self.edit("Switch version", |p| p.set_patch_active(id, version));
        self.selection.retain(|c| self.project.clip(*c).is_some());
    }

    /// Loops a change with the music around it. Switching versions while this plays is the comparison.
    pub fn hear_change(&mut self, id: PatchId) {
        const AFTER: f64 = 1.0;
        let Some(patch) = self.project.patch(id) else { return };
        let (a, b) = self.project.patch_range(patch);
        let start = (a - PRE_ROLL).max(0.0);
        self.selected_patch = Some(id);
        self.project.loop_region = mp_model::LoopRegion { start, end: b + AFTER, enabled: true };
        self.needs_compile = true;
        self.seek(start);
        self.play_start = start;
        self.engine.audition(None);
        self.engine.play();
    }

    /// Dims what a change hides, so that it reads as replaced rather than deleted.
    pub fn paint_change_shade(&self, painter: &egui::Painter, view: &TimeView, lane: Rect, track: TrackId) {
        for (a, b) in self.project.hidden_ranges(track) {
            let r = Rect::from_min_max(egui::pos2(view.x(a), lane.top()), egui::pos2(view.x(b), lane.bottom()));
            painter.rect_filled(r, 0.0, alpha(BASE, 190));
        }
    }

    /// Each change's marker on its lane: what it's playing, and a click to compare.
    pub fn change_pills(&self, view: &TimeView, lane: Rect, track: TrackId, painter: &egui::Painter) -> Vec<(PatchId, Rect, String, bool)> {
        self.project
            .patches
            .iter()
            .filter(|p| p.track == track)
            .map(|p| {
                let on = p.active.is_some() && !self.project.bypass_patches;
                let (a, _) = self.project.patch_range(p);
                let label = if on {
                    if p.versions.len() > 1 { format!("{} · {}", p.active.unwrap_or(0) + 1, truncate(p.current_text(), 12)) } else { truncate(p.current_text(), 14) }
                } else {
                    "Original".to_string()
                };
                let width = painter.layout_no_wrap(label.clone(), font_micro(), TEXT).size().x + SP_2;
                let at = egui::pos2(view.x(a), lane.bottom() - 16.0);
                (p.id, Rect::from_min_size(at, egui::vec2(width, 14.0)), label, on)
            })
            .collect()
    }

    pub fn paint_change_pills(&self, painter: &egui::Painter, view: &TimeView, lane: Rect, track: TrackId) {
        for (id, rect, label, on) in self.change_pills(view, lane, track, painter) {
            let selected = self.selected_patch == Some(id);
            painter.rect_filled(rect, R_S, if on { TEXT_2 } else { CONTROL });
            if selected {
                painter.rect_stroke(rect, R_S, Stroke::new(1.0, TEXT), egui::StrokeKind::Outside);
            }
            painter.text(rect.left_center() + egui::vec2(SP_1, 0.0), Align2::LEFT_CENTER, label, font_micro(), if on { BASE } else { TEXT_2 });
        }
    }
}

/// How much of the song plays before a change, so that it's heard coming in.
pub const PRE_ROLL: f64 = 1.5;
