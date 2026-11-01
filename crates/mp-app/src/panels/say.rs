//! The right panel. Say is where lines get made: one input that takes a word, a whole line, /a sound/
//! or ~a rhyme. Transcript is for reading what was sung. Sources holds the files and speech models.

use crate::app::{App, ToastAction, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use crate::panels::timeline::{format_bars, format_time};
use eframe::egui::{self, Align2, CursorIcon, Key, Modifiers, Rect, Sense, Stroke, Ui};
use mp_analysis::search::Hit;
use mp_analysis::sentence::{self, PartSource, SentencePart};
use mp_analysis::transcribe::MODELS;
use mp_analysis::{normalize_word, Transcript};
use mp_dsp::Stereo;
use mp_model::{SourceId, TrackKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SayTab {
    Say,
    Lyrics,
    Changes,
    Words,
    Sources,
}

pub struct SayState {
    pub tab: SayTab,
    pub text: String,
    pub plan: Vec<SentencePart>,
    /// The word whose takes are shown below the line.
    pub focus: usize,
    pub dirty: bool,
    pub focus_input: bool,
    pub gap_ms: f64,
    pub quantize_words: bool,
    pub tune_words: bool,
    pub pending_transcribe: Vec<SourceId>,
    /// Lyrics being rewritten, which makes the input change them rather than place a new line.
    pub rewrite: Option<crate::panels::changes::Rewrite>,
    /// Words selected in the lyrics or editor, as a source and a range of words.
    pub word_selection: Option<(SourceId, usize, usize)>,
    /// The last thing tried, for comparing with what was there.
    pub trial_last: Option<usize>,
    /// Lines placed this session, newest first.
    pub recent: Vec<String>,
    similar: (String, Vec<String>),
}

impl Default for SayState {
    fn default() -> Self {
        Self {
            tab: SayTab::Say,
            text: String::new(),
            plan: Vec::new(),
            focus: 0,
            dirty: false,
            focus_input: false,
            gap_ms: 40.0,
            quantize_words: false,
            tune_words: false,
            pending_transcribe: Vec::new(),
            rewrite: None,
            word_selection: None,
            trial_last: None,
            recent: Vec::new(),
            similar: (String::new(), Vec::new()),
        }
    }
}

const TYPING_HINTS: [(&str, &str); 4] = [
    ("hello", "every take of a word"),
    ("hello world", "a whole line"),
    ("/k ae t/", "a sound, even inside words"),
    ("~cat", "words that rhyme"),
];
const RECENT_LINES: usize = 8;

const ROW_H: f32 = 50.0;
const RESULT_LIMIT: usize = 300;
const FOOTER_H: f32 = 92.0;

pub fn input_id() -> egui::Id {
    egui::Id::unique("say-input")
}

impl App {
    pub fn say_panel(&mut self, ui: &mut Ui) {
        self.video_preview(ui);
        if self.trial.is_some() {
            self.trial_panel(ui);
            return;
        }
        tabs(ui, &mut self.say.tab, &[(SayTab::Say, "Say"), (SayTab::Lyrics, "Lyrics"), (SayTab::Changes, "Changes"), (SayTab::Words, "Words"), (SayTab::Sources, "Files")]);
        let rule = ui.cursor().top() + 1.0;
        ui.painter().hline(ui.max_rect().x_range(), rule, Stroke::new(1.0, CONTROL));
        ui.add_space(SP_3);
        if self.say.dirty {
            self.replan();
        }
        match self.say.tab {
            SayTab::Say => self.say_tab(ui),
            SayTab::Lyrics => self.transcript_tab(ui),
            SayTab::Changes => self.changes_tab(ui),
            SayTab::Words => self.words_tab(ui),
            SayTab::Sources => self.sources_tab(ui),
        }
    }

    /// Works out how the line will be made. Takes already chosen for unchanged words are kept, so
    /// typing more doesn't undo auditioning.
    pub fn replan(&mut self) {
        self.say.dirty = false;
        if self.say.rewrite.is_some() {
            self.plan_rewrite();
            return;
        }
        let query = self.say.text.trim().to_string();
        let old = std::mem::take(&mut self.say.plan);
        self.say.plan = if query.starts_with('/') || query.starts_with('~') {
            let options = self.index.search(&query, RESULT_LIMIT);
            let source = if options.is_empty() { PartSource::Missing } else { PartSource::Takes };
            vec![SentencePart { text: query, source, options, selected: 0, made: None, use_made: false }]
        } else {
            let mut plan = sentence::plan(&query, &self.index);
            self.mark_made_words(&mut plan);
            for part in &mut plan {
                if let Some(previous) = old.iter().find(|p| p.text == part.text && p.source == part.source) {
                    part.selected = previous.selected.min(part.options.len().saturating_sub(1));
                    if part.made.is_some() && previous.made.is_some() {
                        part.use_made = previous.use_made;
                    }
                }
            }
            plan
        };
        self.say.focus = self.say.focus.min(self.say.plan.len().saturating_sub(1));
    }

    /// A word you made takes priority over anything found or stitched.
    pub fn mark_made_words(&self, plan: &mut [SentencePart]) {
        for part in plan {
            if let Some(word) = self.project.word_named(&part.text) {
                part.made = Some(word.id.0);
                part.use_made = true;
            }
        }
    }

    fn say_tab(&mut self, ui: &mut Ui) {
        self.say_status(ui);
        self.rewrite_header(ui);
        self.say_input(ui);
        ui.add_space(SP_2);
        if self.say.rewrite.is_some() {
            if self.say.plan.is_empty() {
                ui.label(caption(if self.say.rewrite.as_ref().is_some_and(|r| r.runs.is_empty()) { "Edit the words. Only what you change gets new audio." } else { "These words will be removed." }));
            } else {
                self.plan_chips(ui);
                ui.add_space(SP_2);
            }
            let list_height = (ui.available_height() - 66.0).max(ROW_H);
            ui.allocate_ui(egui::vec2(ui.available_width(), list_height), |ui| {
                ui.set_min_height(list_height);
                self.focused_part(ui);
            });
            self.rewrite_footer(ui);
            return;
        }
        if self.say.text.trim().is_empty() {
            self.say_empty(ui);
            return;
        }
        if self.say.plan.is_empty() {
            return;
        }
        self.plan_chips(ui);
        ui.add_space(SP_2);
        let list_height = (ui.available_height() - FOOTER_H).max(ROW_H);
        ui.allocate_ui(egui::vec2(ui.available_width(), list_height), |ui| {
            ui.set_min_height(list_height);
            self.focused_part(ui);
        });
        self.say_footer(ui);
    }

    /// What's happening with the words, and the one thing to do about it if there's a problem.
    fn say_status(&mut self, ui: &mut Ui) {
        if let Some((label, fraction)) = self.jobs.active.iter().find(|j| j.label.starts_with("Transcribing") || j.label.starts_with("Downloading") || j.label.starts_with("Splitting")).map(|j| (j.label.clone(), j.fraction)) {
            ui.label(caption(format!("{label}…")));
            progress(ui, fraction, ui.available_width());
            ui.add_space(SP_2);
            return;
        }
        if let Some(source) = self.split_offer() {
            self.split_offer_card(ui, source);
            // Words wait for this decision, so the speech model can too.
            if self.stems.held.contains(&source) {
                return;
            }
        }
        let needs_model = !MODELS.iter().any(|m| m.is_installed());
        let has_vocal = self.project.sources.iter().any(|s| s.kind == TrackKind::Vocal);
        if needs_model && (has_vocal || !self.say.pending_transcribe.is_empty()) {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(strong("Add a speech model to find the words"));
                ui.label(caption("It runs on this computer. A one-time download from Hugging Face."));
                ui.add_space(SP_1);
                let recommended = MODELS.iter().find(|m| m.id == "base.en").unwrap_or(&MODELS[0]);
                if ui.add(Button::primary(&format!("Download {} model, {} MB", recommended.label, recommended.size_mb))).clicked() {
                    self.jobs.download_model(*recommended);
                }
                ui.label(caption("Singing? Choose Large Turbo in Sources."));
            });
            ui.add_space(SP_2);
        }
    }

    fn say_input(&mut self, ui: &mut Ui) {
        let id = input_id();
        // Line-editing keys mean something else here, so they're taken before the text box sees them.
        if ui.memory(|m| m.has_focus(id)) {
            let (preview, place, prev_word, next_word, up, down, escape) = ui.input_mut(|i| {
                let preview = i.consume_key(Modifiers::SHIFT, Key::Enter);
                let place = i.consume_key(Modifiers::NONE, Key::Enter);
                let prev_word = i.consume_key(Modifiers::SHIFT, Key::Tab);
                let next_word = i.consume_key(Modifiers::NONE, Key::Tab);
                let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                let escape = i.consume_key(Modifiers::NONE, Key::Escape);
                (preview, place, prev_word, next_word, up, down, escape)
            });
            if self.say.dirty {
                self.replan();
            }
            let count = self.say.plan.len();
            // In an empty input, Space means what it means everywhere else: listen.
            if self.say.text.is_empty() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) {
                self.hear_placed_line();
            }
            if count > 0 && (prev_word || next_word) {
                self.say.focus = if next_word { (self.say.focus + 1) % count } else { (self.say.focus + count - 1) % count };
                self.audition_focused();
            }
            if up || down {
                self.step_focused_take(if down { 1 } else { -1 });
            }
            let rewriting = self.say.rewrite.is_some();
            if preview {
                if rewriting { self.preview_rewrite() } else { self.preview_line() }
            }
            if place {
                if rewriting { self.commit_rewrite() } else { self.place_sentence() }
            }
            if escape {
                if rewriting {
                    self.cancel_rewrite();
                } else if self.say.text.is_empty() {
                    ui.memory_mut(|m| m.surrender_focus(id));
                } else {
                    self.say.text.clear();
                    self.say.dirty = true;
                }
            }
        }
        let focused = ui.memory(|m| m.has_focus(id));
        let stroke = Stroke::new(1.0, if focused { TEXT_3 } else { CONTROL });
        let frame = egui::Frame::NONE.fill(BASE).stroke(stroke).corner_radius(R_M).inner_margin(egui::Margin::same(SP_2 as i8));
        let edit = frame
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.say.text)
                        .id(id)
                        .frame(egui::Frame::NONE)
                        .font(font_input())
                        .return_key(None)
                        .lock_focus(true)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text(if self.say.rewrite.is_some() { "Edit the words" } else { "Type what they should say" }),
                )
            })
            .inner;
        if edit.changed() {
            self.say.focus = self.say.plan.len().saturating_sub(1);
            self.say.dirty = true;
        }
        if self.say.focus_input {
            edit.request_focus();
            self.say.focus_input = false;
        }
    }

    fn say_empty(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("say-empty").auto_shrink(false).show(ui, |ui| {
            if !self.project.words.is_empty() {
                ui.label(caption("Your words"));
                ui.add_space(SP_1);
                let names: Vec<String> = self.project.words.iter().map(|w| w.name.clone()).collect();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_HALF + 1.0, SP_HALF + 1.0);
                    for name in names {
                        if ui.add(Button::secondary(&name).small()).clicked() {
                            self.append_to_line(&name);
                        }
                    }
                });
                ui.add_space(SP_2);
            }
            if self.index.word_count() > 0 {
                if !self.selection.is_empty() && !self.engine.is_playing() {
                    hint_row(ui, "Space", "Hear that line with the music");
                    ui.add_space(SP_2);
                }
                let count = self.index.word_count();
                ui.label(caption(format!("{count} word{} ready. Type a line, or find one in the lyrics.", if count == 1 { "" } else { "s" })));
                ui.add_space(SP_1);
                if ui.add(Button::secondary("Read the lyrics").small()).clicked() {
                    self.say.tab = SayTab::Lyrics;
                }
                ui.add_space(SP_3);
                if self.say.recent.is_empty() {
                    section(ui, "What you can type");
                    for (query, what) in TYPING_HINTS {
                        hint_row(ui, query, what);
                    }
                } else {
                    section(ui, "Recent lines");
                    for line in self.say.recent.clone() {
                        if ui.add(Button::ghost(&truncate(&line, 44)).small().fill_width().left().tip("Use this line again", None)).clicked() {
                            self.say.text = line;
                            self.say.dirty = true;
                            self.say.focus_input = true;
                        }
                    }
                }
            } else {
                section(ui, "What you can type");
                for (query, what) in TYPING_HINTS {
                    hint_row(ui, query, what);
                }
            }
        });
    }

    /// Plays the selection, and stopping comes back to where the next line goes.
    fn hear_placed_line(&mut self) {
        if self.engine.is_playing() {
            self.stop();
            self.editor.needs_fit = true;
        } else if !self.selection.is_empty() {
            let resume = self.playhead;
            self.run(crate::actions::Action::PlayFromSelection);
            self.play_start = resume;
        } else {
            self.toggle_play();
        }
    }

    pub fn append_to_line(&mut self, word: &str) {
        if !self.say.text.is_empty() && !self.say.text.ends_with(' ') {
            self.say.text.push(' ');
        }
        self.say.text.push_str(word);
        self.say.tab = SayTab::Say;
        self.say.dirty = true;
        self.say.focus_input = true;
    }

    fn plan_chips(&mut self, ui: &mut Ui) {
        // A chip's width comes from measured text, so that rows can be worked out before drawing.
        let available = ui.available_width();
        let mut rows: Vec<Vec<(usize, f32)>> = vec![Vec::new()];
        let mut used = 0.0;
        for (i, part) in self.say.plan.iter().enumerate() {
            let width = ui.painter().layout_no_wrap(part.text.clone(), font_body(), TEXT).size().x + SP_4;
            if used + width > available && used > 0.0 {
                rows.push(Vec::new());
                used = 0.0;
            }
            rows.last_mut().expect("rows starts non-empty").push((i, width));
            used += width + SP_1;
        }
        for row in rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP_1;
                for (i, width) in row {
                    self.plan_chip(ui, i, width);
                }
            });
        }
    }

    fn plan_chip(&mut self, ui: &mut Ui, i: usize, width: f32) {
        let part = &self.say.plan[i];
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), Sense::click());
        let focused = i == self.say.focus;
        let (color, tip_text) = if part.use_made { (GOOD, "A word you made. ↑ and ↓ try other takes.") } else { match part.source {
            PartSource::Takes => (TEXT_3, "Click to hear. Tab moves between words."),
            PartSource::Stitched => (WARN, "Never said as a word, so it's stitched from sounds."),
            PartSource::Missing => (BAD, "Nothing recorded can make this."),
        } };
        let fill = if focused { CONTROL_HOVER } else if response.hovered() { CONTROL } else { RAISED };
        ui.painter().rect_filled(rect, R_S, fill);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, &part.text, font_body(), if part.source == PartSource::Missing && !part.use_made { BAD } else { TEXT });
        // A line under the chip shows how it'll be made, without taking up room.
        let line = Rect::from_min_max(egui::pos2(rect.left() + SP_1, rect.bottom() - SP_HALF), egui::pos2(rect.right() - SP_1, rect.bottom()));
        ui.painter().rect_filled(line, egui::CornerRadius::same(1), if focused && part.source == PartSource::Takes { TEXT } else { color });
        let clicked = response.clicked();
        tip(response, tip_text, None);
        if clicked {
            self.say.focus = i;
            self.audition_focused();
        }
    }

    fn audition_focused(&mut self) {
        let Some(part) = self.say.plan.get(self.say.focus) else { return };
        if part.use_made {
            if let Some(word) = part.made {
                self.audition_word(mp_model::WordId(word));
            }
            return;
        }
        let pieces: Vec<Hit> = part.pieces().into_iter().cloned().collect();
        self.audition_pieces(&pieces, 0.0);
    }

    fn step_focused_take(&mut self, delta: i32) {
        let Some(part) = self.say.plan.get_mut(self.say.focus) else { return };
        // A made word sits above the takes in the list, so the keys walk from one into the other.
        if part.made.is_some() {
            if part.use_made {
                if delta > 0 && part.source == PartSource::Takes && !part.options.is_empty() {
                    part.use_made = false;
                    part.selected = 0;
                }
                self.audition_focused();
                return;
            }
            if delta < 0 && part.selected == 0 {
                part.use_made = true;
                self.audition_focused();
                return;
            }
        }
        if part.source != PartSource::Takes || part.options.is_empty() {
            return;
        }
        let n = part.options.len() as i32;
        part.selected = (part.selected as i32 + delta).rem_euclid(n) as usize;
        self.audition_focused();
    }

    /// The takes of the focused word, or what can be done about a word that can't be made.
    fn focused_part(&mut self, ui: &mut Ui) {
        let Some(part) = self.say.plan.get(self.say.focus).cloned() else { return };
        if let Some(word) = part.made {
            self.made_word_row(ui, mp_model::WordId(word), part.use_made);
            ui.add_space(SP_1);
        }
        match part.source {
            PartSource::Takes => {
                let n = part.options.len();
                ui.label(caption(format!("{n} take{} of \"{}\"", if n == 1 { "" } else { "s" }, part.text)));
            }
            PartSource::Stitched => {
                ui.label(caption(format!("\"{}\" isn't in the recordings. Stitched from sounds:", part.text)));
            }
            PartSource::Missing => {
                if part.made.is_none() {
                    ui.label(caption(format!("Nothing here can make \"{}\"", part.text)));
                    self.similar_words(ui, &part.text);
                }
                return;
            }
        }
        ui.add_space(SP_1);
        let selected = part.selected;
        let stitched = part.source == PartSource::Stitched;
        egui::ScrollArea::vertical().id_salt("takes").auto_shrink(false).show_rows(ui, ROW_H, part.options.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                self.take_row(ui, i, &part.options[i], !stitched && !part.use_made && i == selected, stitched);
            }
        });
    }

    /// A saved word as a row in the list of takes.
    fn made_word_row(&mut self, ui: &mut Ui, word: mp_model::WordId, selected: bool) {
        let Some(saved) = self.project.word(word) else { return };
        let (name, parts, length) = (saved.name.clone(), saved.parts.len(), saved.length());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, R_S, CONTROL);
            painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(SP_HALF, rect.height())), 0.0, ACCENT);
        } else if response.hovered() {
            painter.rect_filled(rect, R_S, RAISED);
        }
        painter.text(rect.left_top() + egui::vec2(SP_2, 5.0), Align2::LEFT_TOP, &name, font_label(), TEXT);
        let info = format!("Your word · {parts} piece{} · {length:.2}s", if parts == 1 { "" } else { "s" });
        painter.text(rect.left_top() + egui::vec2(SP_2, 21.0), Align2::LEFT_TOP, info, font_caption(), TEXT_3);
        if response.clicked() {
            if let Some(part) = self.say.plan.get_mut(self.say.focus) {
                part.use_made = true;
            }
            self.audition_word(word);
        }
        if response.double_clicked() {
            self.place_saved_word(word);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.word_drag = Some(word);
        }
    }

    /// Puts a saved word at the playhead on the Sentence track.
    pub fn place_saved_word(&mut self, word: mp_model::WordId) {
        let at = self.playhead;
        let name = self.project.word(word).map_or(String::new(), |w| w.name.clone());
        let (placed, track) = self.edit(&format!("Place \"{name}\""), |p| {
            let track = App::sentence_track(p);
            (p.place_word(word, track, at), track)
        });
        let end = placed.iter().filter_map(|c| self.project.clip(*c)).map(|c| c.end()).fold(at, f64::max);
        self.selection = placed.clone();
        self.selected_track = Some(track);
        self.seek(end);
        self.after_place(&placed, track);
    }

    /// Everything you've made, to hear, use and tidy up.
    fn words_tab(&mut self, ui: &mut Ui) {
        if self.project.words.is_empty() {
            let rect = ui.available_rect_before_wrap();
            empty_state(ui, rect, "No words of your own yet", "Select clips and press ⇧W to keep a word you've made.", None);
            return;
        }
        let words: Vec<mp_model::WordId> = self.project.words.iter().map(|w| w.id).collect();
        egui::ScrollArea::vertical().id_salt("my-words").auto_shrink(false).show(ui, |ui| {
            for word in words {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SP_HALF;
                    // Three icon buttons and their spacing, so that the row can't push the panel wider.
                    let row_width = ui.available_width() - 3.0 * (CONTROL_H + SP_HALF);
                    ui.allocate_ui(egui::vec2(row_width, 38.0), |ui| self.made_word_row(ui, word, false));
                    if ui.add(IconButton::new(Icon::Plus, "Place at the playhead")).clicked() {
                        self.place_saved_word(word);
                    }
                    let name = self.project.word(word).map_or(String::new(), |w| w.name.clone());
                    if ui.add(IconButton::new(Icon::Note, "Rename")).clicked() {
                        self.word_dialog = Some(crate::app::WordDialog { name, clips: Vec::new(), renaming: Some(word), focused: false });
                    }
                    if ui.add(IconButton::new(Icon::Trash, "Delete. Copies you've placed stay.")).clicked() {
                        self.edit("Delete word", |p| p.words.retain(|w| w.id != word));
                        self.say.dirty = true;
                    }
                });
            }
        });
        self.hint = "Click to hear. Double-click to place. Drag to the timeline. Type a word's name in Say to use it in a line.".to_string();
    }

    fn similar_words(&mut self, ui: &mut Ui, word: &str) {
        if self.say.similar.0 != word {
            let mut found: Vec<String> = Vec::new();
            for hit in self.index.search(word, 40) {
                let text = normalize_word(&hit.text);
                if !found.contains(&text) {
                    found.push(text);
                }
            }
            found.truncate(8);
            self.say.similar = (word.to_string(), found);
        }
        let words = self.say.similar.1.clone();
        if words.is_empty() {
            return;
        }
        ui.add_space(SP_1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
            ui.label(caption("Try:"));
            for w in words {
                if ui.add(Button::secondary(&w).small()).clicked() {
                    self.say.text = self.say.text.replace(word, &w);
                    self.say.dirty = true;
                    self.say.focus_input = true;
                }
            }
        });
    }

    fn take_row(&mut self, ui: &mut Ui, i: usize, hit: &Hit, selected: bool, stitched: bool) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, R_S, CONTROL);
            painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(SP_HALF, rect.height())), 0.0, ACCENT);
        } else if response.hovered() {
            painter.rect_filled(rect, R_S, RAISED);
        }

        // First line: the take in the words around it, which is how you recognise one.
        let x = rect.left() + SP_2;
        let text_right = rect.right() - 72.0;
        let baseline = rect.top() + SP_2;
        let text_area = painter.with_clip_rect(Rect::from_min_max(rect.min, egui::pos2(text_right, rect.bottom())));
        let (before, after) = self.take_context(hit);
        let mut cursor = x;
        for (text, color) in [(before, TEXT_3), (hit.text.clone(), TEXT), (after, TEXT_3)] {
            if text.is_empty() {
                continue;
            }
            let galley = text_area.layout_no_wrap(text, font_body(), color);
            let width = galley.size().x;
            text_area.galley(egui::pos2(cursor, baseline), galley, color);
            cursor += width + SP_1;
        }

        // Second line: where it's from, and how it sounds.
        let source = self.project.source(SourceId(hit.source));
        let mut info = source.map_or(String::new(), |s| truncate(&s.name, 16));
        info.push_str(&format!(" · {}", format_time(hit.start)));
        if let Some(hz) = hit.pitch_hz {
            info.push_str(&format!(" · {}", mp_dsp::midi_name(mp_dsp::hz_to_midi(hz))));
        }
        info.push_str(&format!(" · {:.2}s", hit.end - hit.start));
        text_area.text(egui::pos2(x, rect.top() + 28.0), Align2::LEFT_TOP, info, font_caption(), TEXT_3);

        // A thumbnail of the waveform.
        let thumb = Rect::from_min_max(egui::pos2(text_right + SP_1, rect.top() + SP_2), egui::pos2(rect.right() - SP_2, rect.bottom() - SP_2));
        if let Some(runtime) = self.runtime.get(&SourceId(hit.source)) {
            let sr = self.sample_rate as f64;
            let columns = thumb.width() as i32;
            let half = thumb.height() * 0.5;
            for c in 0..columns {
                let f = |n: i32| (hit.start + (hit.end - hit.start) * n as f64 / columns as f64) * sr;
                let bin = runtime.peaks.query(f(c), f(c + 1));
                let px = thumb.left() + c as f32;
                let (top, bottom) = (thumb.center().y - bin.max * half, thumb.center().y - bin.min * half);
                painter.line_segment([egui::pos2(px, top), egui::pos2(px, bottom.max(top + 1.0))], Stroke::new(1.0, if selected { TEXT } else { TEXT_3 }));
            }
        }

        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
            self.hint = "Click to hear. Double-click to place only this take. Drag to the timeline.".to_string();
        }
        if response.clicked() {
            if !stitched {
                if let Some(part) = self.say.plan.get_mut(self.say.focus) {
                    part.selected = i;
                }
            }
            self.audition_hit(hit);
        }
        if response.double_clicked() {
            self.place_hit_at_playhead(hit);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.hit_drag = Some(hit.clone());
        }
    }

    /// A couple of words either side of a take.
    fn take_context(&self, hit: &Hit) -> (String, String) {
        let Some(transcript) = self.index.transcript(hit.source) else { return (String::new(), String::new()) };
        // A sound's context is the word it's inside, shown after it.
        if let Some(word) = &hit.context {
            return (String::new(), format!("in {word}"));
        }
        let length = hit.text.split_whitespace().count().max(1);
        let around = |from: usize, to: usize| -> String {
            transcript.words.get(from..to).map_or(String::new(), |w| w.iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join(" "))
        };
        let index = hit.word_index;
        (around(index.saturating_sub(2), index), around(index + length, (index + length + 2).min(transcript.words.len())))
    }

    fn say_footer(&mut self, ui: &mut Ui) {
        ui.add_space(SP_1);
        ui.add(ValueField::new("Gap", &mut self.say.gap_ms, 0.0..=400.0).unit(" ms").decimals(0).default(40.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_3;
            switch(ui, &mut self.say.quantize_words, "Snap to grid");
            switch(ui, &mut self.say.tune_words, "Tune to key");
        });
        ui.add_space(SP_1);
        let has_audio = self.say.plan.iter().any(|p| p.use_made || !p.pieces().is_empty());
        ui.horizontal(|ui| {
            if ui.add(Button::secondary("Preview").enabled(has_audio).tip("Hear the line before placing it", Some("⇧Return"))).clicked() {
                self.preview_line();
            }
            let place = format!("Place at {}", format_bars(&self.project, self.playhead));
            if ui.add(Button::primary(&place).enabled(has_audio).tip("Put the line on the timeline", Some("Return"))).clicked() {
                self.place_sentence();
            }
        });
    }

    pub fn audition_pieces(&mut self, pieces: &[Hit], gap_seconds: f64) {
        // A single piece can show its picture while it plays. Stitched audio has no one place in a source.
        let single = (pieces.len() == 1).then(|| (SourceId(pieces[0].source), pieces[0].start));
        let buffer = self.pieces_buffer(pieces, gap_seconds);
        self.audition_buffer(buffer);
        self.audition_source = single;
    }

    /// Stitches pieces into one buffer.
    fn pieces_buffer(&self, pieces: &[Hit], gap_seconds: f64) -> Stereo {
        let sr = self.sample_rate as f64;
        let gap = (gap_seconds * sr) as usize;
        let mut out = Stereo::default();
        for hit in pieces {
            let Some(buffer) = self.bank.get(SourceId(hit.source)) else { continue };
            let a = ((hit.start * sr) as usize).min(buffer.frames());
            let b = ((hit.end * sr) as usize).min(buffer.frames());
            if b <= a {
                continue;
            }
            // A short fade at each end, as a placed clip would have.
            let fade = ((0.004 * sr) as usize).max(1).min((b - a) / 2 + 1);
            let start = out.frames();
            out.l.extend_from_slice(&buffer.l[a..b]);
            out.r.extend_from_slice(&buffer.r[a..b]);
            let n = out.frames() - start;
            for k in 0..fade.min(n) {
                let g = k as f32 / fade as f32;
                out.l[start + k] *= g;
                out.r[start + k] *= g;
                out.l[start + n - 1 - k] *= g;
                out.r[start + n - 1 - k] *= g;
            }
            out.l.resize(out.l.len() + gap, 0.0);
            out.r.resize(out.r.len() + gap, 0.0);
        }
        out
    }

    pub fn preview_line(&mut self) {
        let gap = self.say.gap_ms * 0.001;
        let mut line = Stereo::default();
        for part in &self.say.plan {
            let piece = match part.made.filter(|_| part.use_made) {
                Some(word) => self.render_word(mp_model::WordId(word)),
                None => self.pieces_buffer(&part.pieces().into_iter().cloned().collect::<Vec<_>>(), 0.0),
            };
            line.l.extend_from_slice(&piece.l);
            line.r.extend_from_slice(&piece.r);
            let silence = line.l.len() + (gap * self.sample_rate as f64) as usize;
            line.l.resize(silence, 0.0);
            line.r.resize(silence, 0.0);
        }
        self.audition_buffer(line);
    }

    /// Places one part of a line, returning its clips and where it ends.
    pub fn lay_out_part(p: &mut mp_model::Project, part: &SentencePart, track: mp_model::TrackId, start: f64) -> (Vec<mp_model::ClipId>, f64) {
        if let Some(word) = part.made.filter(|_| part.use_made) {
            let placed = p.place_word(mp_model::WordId(word), track, start);
            let end = placed.iter().filter_map(|c| p.clip(*c)).map(|c| c.end()).fold(start, f64::max);
            return (placed, end);
        }
        let mut pieces = Vec::new();
        let mut at = start;
        for hit in part.pieces() {
            if let Some(clip) = App::place_hit_at(p, hit, track, at) {
                at = p.clip(clip).map_or(at, |c| c.end());
                pieces.push(clip);
            }
        }
        // Pieces of a stitched word blend together and stay together, ready to adjust.
        if pieces.len() > 1 {
            const BLEND: f64 = 0.012;
            p.group_clips(&pieces, Some(part.text.clone()));
            p.crossfade_clips(&pieces, BLEND);
        }
        let end = pieces.iter().filter_map(|c| p.clip(*c)).map(|c| c.end()).fold(start, f64::max);
        (pieces, end)
    }

    /// Lays out the line on the Sentence track, starting at the playhead.
    pub fn place_sentence(&mut self) {
        if self.say.dirty {
            self.replan();
        }
        let plan = self.say.plan.clone();
        let gap = self.say.gap_ms * 0.001;
        let quantize = self.say.quantize_words;
        let grid = self.grid_division;
        let at = self.playhead;
        let (clips, end, track) = self.edit("Place line", |p| {
            let track = App::sentence_track(p);
            let mut clips = Vec::new();
            let mut cursor = at;
            for part in &plan {
                let mut start = cursor;
                if quantize {
                    let snapped = p.snap_to_grid(start, grid);
                    start = if snapped < cursor { snapped + p.beat_seconds() * grid } else { snapped };
                }
                let (placed, end) = App::lay_out_part(p, part, track, start);
                clips.extend(placed);
                cursor = end + gap;
            }
            (clips, cursor, track)
        });
        let count = clips.len();
        if count == 0 {
            self.toast(ToastKind::Info, "Nothing to place yet. Type a word that's in the recordings.");
            return;
        }
        self.selection = clips.clone();
        self.selected_track = Some(track);
        self.seek(end);
        if self.say.tune_words {
            self.snap_selection_pitch();
        }
        self.after_place(&clips, track);
        self.toast_with(ToastKind::Good, format!("Placed {count} clip{}", if count == 1 { "" } else { "s" }), ToastAction::Undo);
        let line = self.say.text.trim().to_string();
        self.say.recent.retain(|l| *l != line);
        self.say.recent.insert(0, line);
        self.say.recent.truncate(RECENT_LINES);
        self.say.text.clear();
        self.say.dirty = true;
        self.say.focus_input = true;
        self.editor.needs_fit = true;
    }

    fn transcript_tab(&mut self, ui: &mut Ui) {
        let sources: Vec<(SourceId, String)> = self
            .project
            .sources
            .iter()
            .filter(|s| self.index.transcript(s.id.0).is_some())
            .map(|s| (s.id, s.name.clone()))
            .collect();
        if sources.is_empty() {
            let rect = ui.available_rect_before_wrap();
            empty_state(ui, rect, "Nothing transcribed yet", "Vocals appear here as readable text.", None);
            return;
        }
        self.word_selection_bar(ui);
        ui.add_space(SP_2);
        egui::ScrollArea::vertical().id_salt("transcript").auto_shrink(false).show(ui, |ui| {
            for (source, name) in sources {
                ui.label(strong(name));
                self.lyrics_row(ui, source);
                ui.add_space(SP_1);
                if let Some(transcript) = self.index.transcript(source.0).cloned() {
                    self.transcript_text(ui, source, &transcript);
                }
                ui.add_space(SP_4);
            }
        });
        self.hint = "Click to hear. Shift-click to select a phrase and change it. Double-click to add to your line. Drag to the timeline.".to_string();
    }

    /// Lyrics as flowing text, in phrases split at pauses. Painted directly, so that long ones stay fast.
    fn transcript_text(&mut self, ui: &mut Ui, source: SourceId, transcript: &Transcript) {
        const LINE: f32 = 19.0;
        const PAUSE: f64 = 0.2;
        let width = ui.available_width();
        let left = ui.cursor().left();
        let top = ui.cursor().top();
        let painter = ui.painter().clone();
        let space = 4.0;
        let mut layout: Vec<(usize, Rect)> = Vec::with_capacity(transcript.words.len());
        let (mut x, mut y) = (left, top);
        let mut stamps: Vec<(f32, f64)> = Vec::new();
        const STAMP_W: f32 = 34.0;
        for (i, word) in transcript.words.iter().enumerate() {
            let starts_phrase = i == 0 || word.start - transcript.words[i - 1].end > PAUSE;
            if starts_phrase {
                if i > 0 {
                    y += LINE + SP_1;
                }
                x = left + STAMP_W;
                stamps.push((y, word.start));
            }
            let w = painter.layout_no_wrap(word.text.clone(), font_body(), TEXT).size().x;
            if x + w > left + width && x > left + STAMP_W {
                x = left + STAMP_W;
                y += LINE;
            }
            layout.push((i, Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, LINE))));
            x += w + space;
        }
        let total = Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + width, y + LINE));
        let response = ui.allocate_rect(total, Sense::click_and_drag());
        let pointer = response.hover_pos().or(response.interact_pointer_pos());
        let hovered = pointer.and_then(|p| layout.iter().find(|(_, r)| r.expand2(egui::vec2(space * 0.5, 0.0)).contains(p)).map(|(i, _)| *i));
        let clip = ui.clip_rect();
        for (seconds_y, seconds) in &stamps {
            if *seconds_y + LINE >= clip.top() && *seconds_y <= clip.bottom() {
                let minutes = (*seconds / 60.0) as u64;
                let label = format!("{}:{:02}", minutes, (*seconds % 60.0) as u64);
                painter.text(egui::pos2(left, *seconds_y + 3.0), Align2::LEFT_TOP, label, font_num(9.5), TEXT_3);
            }
        }
        let selected = self.say.word_selection.filter(|(s, _, _)| *s == source).map(|(_, a, b)| a..=b);
        let changed: Vec<(usize, usize)> = self.project.patches.iter().filter(|p| p.source == source && p.active.is_some()).map(|p| (p.first_word, p.last_word)).collect();
        for (i, r) in &layout {
            if r.bottom() < clip.top() || r.top() > clip.bottom() {
                continue;
            }
            let word = &transcript.words[*i];
            let is_hover = hovered == Some(*i);
            let is_selected = selected.as_ref().is_some_and(|s| s.contains(i));
            if is_selected {
                painter.rect_filled(r.expand2(egui::vec2(2.0, 0.0)), 0.0, ACCENT_DIM);
            } else if is_hover {
                painter.rect_filled(r.expand2(egui::vec2(2.0, 0.0)), R_S, CONTROL_HOVER);
            }
            // Dimmer words are ones the transcription is less sure of.
            let color = if is_hover || is_selected { TEXT } else if word.confidence < 0.5 { TEXT_3 } else { TEXT_2 };
            painter.text(r.left_center(), Align2::LEFT_CENTER, &word.text, font_body(), color);
            // A line through words shows that they've been changed.
            if changed.iter().any(|(a, b)| a <= i && i <= b) {
                painter.hline(r.x_range(), r.center().y, Stroke::new(1.0, TEXT_3));
            }
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let hit_at = |i: usize| {
            let w = &transcript.words[i];
            Hit { source: source.0, start: w.start, end: w.end, text: w.text.clone(), context: None, kind: mp_analysis::search::HitKind::Exact, confidence: w.confidence, pitch_hz: w.pitch_hz, word_index: i }
        };
        if let Some(i) = hovered {
            if response.double_clicked() {
                self.say.word_selection = None;
                self.append_to_line(&normalize_word(&transcript.words[i].text));
            } else if response.clicked() {
                self.select_word(source, i, ui.input(|input| input.modifiers.shift));
                self.audition_hit(&hit_at(i));
            } else if response.drag_started_by(egui::PointerButton::Primary) {
                self.hit_drag = Some(hit_at(i));
            }
        }
    }

    /// Selects a word, or extends the selection to it if `extend`.
    pub fn select_word(&mut self, source: SourceId, word: usize, extend: bool) {
        self.say.word_selection = match self.say.word_selection {
            Some((s, a, b)) if extend && s == source => Some((s, a.min(word), b.max(word))),
            _ => Some((source, word, word)),
        };
    }

    fn sources_tab(&mut self, ui: &mut Ui) {
        if ui.add(Button::secondary("Add files…").icon(Icon::Plus).fill_width().tip("Add audio or video files", Some("⌘I"))).clicked() {
            self.import_dialog();
        }
        if ui.add(Button::secondary("Find a song online…").icon(Icon::Search).fill_width().tip("Search for a song or paste a link", Some("⇧⌘F"))).clicked() {
            self.run(crate::actions::Action::FindSong);
        }
        ui.add_space(SP_2);
        egui::ScrollArea::vertical().id_salt("sources").auto_shrink(false).show(ui, |ui| {
            let sources = self.project.sources.clone();
            for source in sources {
                let words = self.index.transcript(source.id.0).map(|t| t.words.len());
                let transcribing = self.jobs.active.iter().any(|j| j.label == format!("Transcribing {}", source.name));
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(strong(truncate(&source.name, 34)));
                    if !source.path.exists() {
                        ui.label(caption("This file can't be found.").color(BAD));
                        if ui.add(Button::secondary("Locate…").small().tip("Choose where this file is now", None)).clicked() {
                            self.locate_source(source.id);
                        }
                    }
                    let mut info = format!("{} · {}", source.kind.label(), format_time(source.duration));
                    if let Some(bpm) = source.bpm {
                        info.push_str(&format!(" · {bpm:.1} BPM"));
                    }
                    ui.label(caption(info));
                    ui.add_space(SP_1);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_1;
                        if transcribing {
                            status_dot(ui, Tone::Warn, "Transcribing");
                        } else if let Some(words) = words {
                            status_dot(ui, Tone::Good, &format!("{words} words"));
                            if ui.add(Button::ghost("Redo").small().tip("Transcribe again with the selected model", None)).clicked() {
                                self.request_transcribe(source.id, true);
                            }
                        } else if ui.add(Button::secondary("Transcribe").small()).clicked() {
                            self.request_transcribe(source.id, true);
                        }
                        if ui.add(Button::ghost("New track").small().tip("Put this file on a new track", None)).clicked() {
                            let (id, name, duration, kind) = (source.id, source.name.clone(), source.duration, source.kind);
                            self.edit("Add source to track", |p| {
                                let t = p.add_track(name, kind);
                                p.add_clip(t, id, 0.0, 0.0, duration);
                            });
                        }
                    });
                });
                ui.add_space(SP_1);
            }
            section(ui, "Speech model");
            let downloading = self.jobs.active.iter().any(|j| j.label.starts_with("Downloading"));
            for (i, model) in MODELS.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(body(format!("{} · {} MB", model.label, model.size_mb)));
                        ui.label(caption(model.note));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if model.is_installed() {
                            if self.selected_model == i {
                                status_dot(ui, Tone::Good, "In use");
                            } else if ui.add(Button::secondary("Use").small()).clicked() {
                                self.selected_model = i;
                            }
                        } else if ui.add(Button::secondary("Download").small().enabled(!downloading)).clicked() {
                            self.jobs.download_model(*model);
                        }
                    });
                });
                ui.add_space(SP_1);
            }
        });
    }
}
