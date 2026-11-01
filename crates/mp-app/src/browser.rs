//! The right panel: word search, the Sentence Builder, and project files with speech models.

use crate::app::{App, ToastKind};
use crate::arranger::truncate;
use crate::jobs::file_stem;
use crate::theme::*;
use crate::timeline::format_time;
use crate::widgets::{self, draw_icon, Icon};
use eframe::egui::{self, Align2, CornerRadius, CursorIcon, Key, Rect, Sense, Stroke, Ui};
use mp_analysis::search::Hit;
use mp_analysis::sentence::{self, PartSource, SentencePart};
use mp_analysis::transcribe::MODELS;
use mp_dsp::Stereo;
use mp_model::SourceId;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrowserTab {
    Words,
    Sentence,
    Files,
}

pub struct BrowserState {
    pub tab: BrowserTab,
    pub query: String,
    pub results: Vec<Hit>,
    pub selected: usize,
    pub search_dirty: bool,
    pub focus_search: bool,
    pub sentence: String,
    pub plan: Vec<SentencePart>,
    pub sentence_dirty: bool,
    pub focus_sentence: bool,
    pub gap_ms: f32,
    pub quantize_words: bool,
    pub tune_words: bool,
    pub pending_transcribe: Vec<SourceId>,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            tab: BrowserTab::Words,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            search_dirty: false,
            focus_search: false,
            sentence: String::new(),
            plan: Vec::new(),
            sentence_dirty: false,
            focus_sentence: false,
            gap_ms: 40.0,
            quantize_words: false,
            tune_words: false,
            pending_transcribe: Vec::new(),
        }
    }
}

const ROW_H: f32 = 46.0;
const RESULT_LIMIT: usize = 500;

impl App {
    pub fn browser_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            for (name, tab) in [("Words", BrowserTab::Words), ("Sentence", BrowserTab::Sentence), ("Files", BrowserTab::Files)] {
                if widgets::tab(ui, name, self.browser.tab == tab) {
                    self.browser.tab = tab;
                }
            }
        });
        ui.add_space(4.0);
        match self.browser.tab {
            BrowserTab::Words => self.words_tab(ui),
            BrowserTab::Sentence => self.sentence_tab(ui),
            BrowserTab::Files => self.files_tab(ui),
        }
    }

    pub fn run_search(&mut self) {
        self.browser.results = self.index.search(&self.browser.query, RESULT_LIMIT);
        self.browser.selected = 0;
        self.browser.search_dirty = false;
    }

    fn words_tab(&mut self, ui: &mut Ui) {
        if self.browser.search_dirty {
            self.run_search();
        }
        // Search box.
        let frame = egui::Frame::NONE.fill(BG_0).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 5));
        let edit = frame
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                    draw_icon(ui.painter(), rect, Icon::Search, TEXT_DIM);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.browser.query)
                            .id(egui::Id::unique("word-search"))
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .hint_text("Search words and sounds"),
                    )
                })
                .inner
            })
            .inner;
        if edit.changed() {
            self.run_search();
        }
        if self.browser.focus_search {
            edit.request_focus();
            self.browser.focus_search = false;
        }
        // Keyboard flow: arrows audition results, Return places one and clears for the next word.
        let enter_pressed = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if edit.has_focus() {
            let (down, up) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                )
            });
            let count = self.browser.results.len();
            if count > 0 && (down || up) {
                self.browser.selected = if down {
                    (self.browser.selected + 1).min(count - 1)
                } else {
                    self.browser.selected.saturating_sub(1)
                };
                let hit = self.browser.results[self.browser.selected].clone();
                self.audition_hit(&hit);
            }
        }
        if enter_pressed {
            if let Some(hit) = self.browser.results.get(self.browser.selected).cloned() {
                self.place_hit_at_playhead(&hit);
                self.browser.query.clear();
                self.run_search();
            }
            edit.request_focus();
        }

        let summary = if self.browser.query.trim().is_empty() {
            if self.index.word_count() > 0 { format!("{} words transcribed", self.index.word_count()) } else { String::new() }
        } else {
            let n = self.browser.results.len();
            format!("{n} result{}", if n == 1 { "" } else { "s" })
        };
        ui.add_space(2.0);
        ui.label(egui::RichText::new(summary).color(TEXT_FAINT).size(10.5));

        if self.browser.query.trim().is_empty() {
            self.vocabulary(ui);
            return;
        }
        if self.browser.results.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No matches").color(TEXT_DIM));
                ui.label(egui::RichText::new("Try /sounds/ like /k ae t/, or ~rhymes").color(TEXT_FAINT).size(11.0));
            });
            return;
        }
        let count = self.browser.results.len();
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_H, count, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                self.result_row(ui, i);
            }
        });
    }

    fn vocabulary(&mut self, ui: &mut Ui) {
        if self.index.word_count() == 0 {
            egui::ScrollArea::vertical().id_salt("search-help").auto_shrink(false).show(ui, |ui| {
                if !MODELS.iter().any(|m| m.is_installed()) {
                    ui.add_space(4.0);
                    self.model_banner(ui);
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new("How search works").color(TEXT_DIM).strong());
                ui.add_space(2.0);
                for (query, what) in [
                    ("hello", "every take of a word"),
                    ("hello world", "a phrase, as it was said"),
                    ("/k ae t/", "a sound, even inside other words"),
                    ("~cat", "words that rhyme"),
                ] {
                    ui.horizontal(|ui| {
                        widgets::keycap(ui, query);
                        ui.label(egui::RichText::new(what).color(TEXT_FAINT).size(11.0));
                    });
                }
            });
            return;
        }
        ui.label(egui::RichText::new("All words").color(TEXT_DIM).strong());
        ui.add_space(2.0);
        let vocabulary = self.index.vocabulary();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
                for (word, count) in vocabulary {
                    let label = if count > 1 { format!("{word} {count}") } else { word.clone() };
                    let response = ui.add(egui::Button::new(egui::RichText::new(label).size(11.0)).min_size(egui::vec2(0.0, 18.0)));
                    if response.clicked() {
                        self.browser.query = word;
                        self.run_search();
                    }
                }
            });
        });
    }

    fn result_row(&mut self, ui: &mut Ui, i: usize) {
        let hit = self.browser.results[i].clone();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        let selected = i == self.browser.selected;
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, CornerRadius::same(4), BG_3);
        } else if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(4), BG_2);
        }
        if selected {
            painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())), 0.0, ACCENT);
        }
        let text_x = rect.left() + 8.0;
        painter.text(egui::pos2(text_x, rect.top() + 6.0), Align2::LEFT_TOP, truncate(&hit.text, 22), ui_font(13.5), TEXT);

        // Second line: where it's from, and how it sounds.
        let source = self.project.source(SourceId(hit.source));
        let mut info = source.map_or(String::new(), |s| truncate(&s.name, 14));
        if let Some(context) = &hit.context {
            info = format!("in \"{}\"", truncate(context, 12));
        }
        info.push_str(&format!(" · {}", format_time(hit.start)));
        if let Some(hz) = hit.pitch_hz {
            info.push_str(&format!(" · {}", mp_dsp::midi_name(mp_dsp::hz_to_midi(hz))));
        }
        painter.text(egui::pos2(text_x, rect.top() + 26.0), Align2::LEFT_TOP, info, ui_font(10.5), TEXT_FAINT);

        // Waveform thumbnail, duration and match type on the right.
        let thumb = Rect::from_min_max(egui::pos2(rect.right() - 72.0, rect.top() + 7.0), egui::pos2(rect.right() - 6.0, rect.top() + 25.0));
        if let Some(runtime) = self.runtime.get(&SourceId(hit.source)) {
            let sr = self.sample_rate as f64;
            let columns = thumb.width() as i32;
            for x in 0..columns {
                let f = |n: i32| (hit.start + (hit.end - hit.start) * n as f64 / columns as f64) * sr;
                let bin = runtime.peaks.query(f(x), f(x + 1));
                let px = thumb.left() + x as f32;
                let (top, bottom) = (thumb.center().y - bin.max * 9.0, thumb.center().y - bin.min * 9.0);
                painter.line_segment([egui::pos2(px, top), egui::pos2(px, bottom.max(top + 1.0))], Stroke::new(1.0, if selected { ACCENT } else { TEXT_DIM }));
            }
        }
        let label = format!("{} · {:.2}s", hit.kind.label(), hit.end - hit.start);
        painter.text(egui::pos2(rect.right() - 6.0, rect.top() + 28.0), Align2::RIGHT_TOP, label, ui_font(9.5), TEXT_FAINT);

        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
            self.hint = "Click to hear. Double-click or Return to place at the playhead. Drag to the timeline.".to_string();
        }
        if response.clicked() {
            self.browser.selected = i;
            self.audition_hit(&hit);
        }
        if response.double_clicked() {
            self.place_hit_at_playhead(&hit);
        }
        if response.drag_started() {
            self.browser.selected = i;
            self.hit_drag = Some(hit);
        }
    }

    /// Offers the speech models. Nothing is downloaded until one is clicked.
    fn model_banner(&mut self, ui: &mut Ui) {
        egui::Frame::NONE
            .fill(ACCENT_SOFT)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Add a speech model").strong().color(TEXT));
                ui.label(
                    egui::RichText::new("Transcription runs on this computer and needs one model, downloaded once from Hugging Face.")
                        .size(11.0)
                        .color(TEXT_DIM),
                );
                ui.add_space(3.0);
                self.model_rows(ui);
            });
        ui.add_space(6.0);
    }

    fn model_rows(&mut self, ui: &mut Ui) {
        let downloading = self.jobs.active.iter().any(|j| j.label.starts_with("Downloading"));
        for (i, model) in MODELS.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(egui::RichText::new(format!("{} · {} MB", model.label, model.size_mb)).size(12.0).color(TEXT));
                    ui.label(egui::RichText::new(model.note).size(10.0).color(TEXT_DIM));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if model.is_installed() {
                        if self.selected_model == i {
                            ui.label(egui::RichText::new("In use").size(11.0).color(GOOD));
                        } else if ui.button("Use").clicked() {
                            self.selected_model = i;
                        }
                    } else if ui.add_enabled(!downloading, egui::Button::new("Download")).clicked() {
                        self.jobs.download_model(*model);
                    }
                });
            });
        }
    }

    fn replan_sentence(&mut self) {
        // Keep take choices for words that are unchanged, so typing doesn't undo auditioning.
        let old = std::mem::take(&mut self.browser.plan);
        let mut plan = sentence::plan(&self.browser.sentence, &self.index);
        for part in &mut plan {
            if let Some(previous) = old.iter().find(|p| p.text == part.text) {
                part.selected = previous.selected.min(part.options.len().saturating_sub(1));
            }
        }
        self.browser.plan = plan;
        self.browser.sentence_dirty = false;
    }

    fn sentence_tab(&mut self, ui: &mut Ui) {
        if self.browser.sentence_dirty {
            self.replan_sentence();
        }
        ui.label(egui::RichText::new("Type what you want them to say").color(TEXT_DIM).size(11.5));
        let edit = ui.add(
            egui::TextEdit::multiline(&mut self.browser.sentence)
                .id(egui::Id::unique("sentence-text"))
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("never gonna say this"),
        );
        if self.browser.focus_sentence {
            edit.request_focus();
            self.browser.focus_sentence = false;
        }
        if edit.changed() {
            self.replan_sentence();
        }
        ui.add_space(4.0);

        if self.index.word_count() == 0 {
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Transcribe a vocal first, and this will build sentences from its words.").color(TEXT_FAINT).size(11.5));
            return;
        }
        if self.browser.plan.is_empty() {
            return;
        }

        // Each part of the sentence: colour shows how it'll be made, arrows choose takes.
        egui::ScrollArea::vertical().id_salt("plan").max_height(ui.available_height() - 124.0).auto_shrink([false, true]).show(ui, |ui| {
            // A framed chip only knows its size after it's drawn, so wrapped layout can't place it.
            // Rows are worked out from measured text instead.
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
            let available = ui.available_width();
            let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (i, part) in self.browser.plan.iter().enumerate() {
                let text = ui.painter().layout_no_wrap(part.text.clone(), ui_font(12.0), TEXT).size().x;
                let takes = part.source == PartSource::Takes && part.options.len() > 1;
                let width = text + 16.0 + if takes { 62.0 } else { 0.0 };
                if used + width > available && used > 0.0 {
                    rows.push(Vec::new());
                    used = 0.0;
                }
                rows.last_mut().expect("rows starts non-empty").push(i);
                used += width;
            }
            for row in rows {
                ui.horizontal(|ui| {
                    for i in row {
                        self.plan_chip(ui, i);
                    }
                });
            }
        });
        ui.add_space(6.0);
        let (takes, stitched, missing) = self.browser.plan.iter().fold((0, 0, 0), |(a, b, c), p| match p.source {
            PartSource::Takes => (a + 1, b, c),
            PartSource::Stitched => (a, b + 1, c),
            PartSource::Missing => (a, b, c + 1),
        });
        ui.horizontal_wrapped(|ui| {
            let legend = |ui: &mut Ui, color, text: String| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, color);
                ui.label(egui::RichText::new(text).size(10.5).color(TEXT_FAINT));
            };
            legend(ui, GOOD, format!("{takes} found"));
            if stitched > 0 {
                legend(ui, WARN, format!("{stitched} stitched from sounds"));
            }
            if missing > 0 {
                legend(ui, BAD, format!("{missing} missing"));
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Gap").color(TEXT_DIM).size(11.0));
            ui.add(egui::Slider::new(&mut self.browser.gap_ms, 0.0..=400.0).suffix(" ms").show_value(true).clamping(egui::SliderClamping::Always));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.browser.quantize_words, "Snap words to grid");
            ui.checkbox(&mut self.browser.tune_words, "Tune to key");
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Preview").on_hover_text("Hear the sentence before placing it").clicked() {
                self.preview_sentence();
            }
            let has_audio = self.browser.plan.iter().any(|p| !p.pieces().is_empty());
            ui.add_enabled_ui(has_audio, |ui| {
                if widgets::accent_button(ui, "Place at playhead").clicked() {
                    self.place_sentence();
                }
            });
        });
    }

    fn plan_chip(&mut self, ui: &mut Ui, i: usize) {
        let part = self.browser.plan[i].clone();
        let color = match part.source {
            PartSource::Takes => GOOD,
            PartSource::Stitched => WARN,
            PartSource::Missing => BAD,
        };
        let takes = part.source == PartSource::Takes && part.options.len() > 1;
        let frame = egui::Frame::NONE
            .fill(mix(BG_3, color, 0.14))
            .stroke(Stroke::new(1.0, mix(BG_3, color, 0.5)))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::symmetric(5, 2));
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                if takes && ui.small_button("‹").on_hover_text("Previous take").clicked() {
                    let p = &mut self.browser.plan[i];
                    p.selected = (p.selected + p.options.len() - 1) % p.options.len();
                    self.audition_part(i);
                }
                let text = ui.add(egui::Label::new(egui::RichText::new(&part.text).size(12.0).color(TEXT)).sense(Sense::click()));
                if text.clicked() {
                    self.audition_part(i);
                }
                let tip = match part.source {
                    PartSource::Takes => "Click to hear this take",
                    PartSource::Stitched => "Never said as a word, so it's made from sounds. Click to hear.",
                    PartSource::Missing => "Nothing recorded can make this word",
                };
                text.on_hover_text(tip);
                if takes {
                    ui.label(egui::RichText::new(format!("{}/{}", part.selected + 1, part.options.len())).size(9.5).color(TEXT_FAINT));
                    if ui.small_button("›").on_hover_text("Next take").clicked() {
                        let p = &mut self.browser.plan[i];
                        p.selected = (p.selected + 1) % p.options.len();
                        self.audition_part(i);
                    }
                }
            });
        });
    }

    fn audition_part(&mut self, i: usize) {
        let pieces: Vec<Hit> = self.browser.plan[i].pieces().into_iter().cloned().collect();
        self.audition_pieces(&pieces, 0.0);
    }

    /// Stitches pieces into one buffer and plays it through the preview path.
    fn audition_pieces(&mut self, pieces: &[Hit], gap_seconds: f64) {
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
            out.l.resize(out.frames() + gap, 0.0);
            out.r.resize(out.r.len() + gap, 0.0);
        }
        let seconds = out.frames() as f64 / sr;
        if seconds > 0.0 {
            let voice = mp_engine::compile::audition_voice(Arc::new(out), 0.0, seconds, false, self.sample_rate);
            self.engine.audition(voice);
        }
    }

    fn preview_sentence(&mut self) {
        let pieces: Vec<Hit> = self.browser.plan.iter().flat_map(|p| p.pieces()).cloned().collect();
        self.audition_pieces(&pieces, self.browser.gap_ms as f64 * 0.001);
    }

    /// Lays out the sentence on the Sentence track, starting at the playhead.
    pub fn place_sentence(&mut self) {
        let plan = self.browser.plan.clone();
        let gap = self.browser.gap_ms as f64 * 0.001;
        let quantize = self.browser.quantize_words;
        let grid = self.grid_division;
        let at = self.playhead;
        let (clips, end, track) = self.edit("Place sentence", |p| {
            let track = App::sentence_track(p);
            let mut clips = Vec::new();
            let mut cursor = at;
            for part in &plan {
                for (n, hit) in part.pieces().into_iter().enumerate() {
                    let mut start = cursor;
                    // Stitched pieces butt together, so that they read as a single word.
                    if quantize && n == 0 {
                        let snapped = p.snap_to_grid(start, grid);
                        start = if snapped < cursor { snapped + p.beat_seconds() * grid } else { snapped };
                    }
                    if let Some(clip) = App::place_hit_at(p, hit, track, start) {
                        cursor = p.clip(clip).map_or(start, |c| c.end());
                        clips.push(clip);
                    }
                }
                cursor += gap;
            }
            (clips, cursor, track)
        });
        let count = clips.len();
        if count == 0 {
            self.toast(ToastKind::Info, "Nothing to place. Transcribe a vocal first.");
            return;
        }
        self.selection = clips;
        self.selected_track = Some(track);
        self.seek(end);
        if self.browser.tune_words {
            self.snap_selection_pitch();
        }
        self.toast(ToastKind::Good, format!("Placed {count} clip{}", if count == 1 { "" } else { "s" }));
        self.editor.needs_fit = true;
    }

    fn files_tab(&mut self, ui: &mut Ui) {
        if ui.button("Add audio files…").clicked() {
            self.import_dialog();
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let sources = self.project.sources.clone();
            for source in sources {
                let transcript = self.index.transcript(source.id.0).map(|t| t.words.len());
                let transcribing = self.jobs.active.iter().any(|j| j.label == format!("Transcribing {}", source.name));
                egui::Frame::NONE.fill(BG_2).corner_radius(CornerRadius::same(5)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(truncate(&source.name, 34)).color(TEXT));
                    let mut info = format!("{} · {}", source.kind.label(), format_time(source.duration));
                    if let Some(bpm) = source.bpm {
                        info.push_str(&format!(" · {bpm:.1} BPM"));
                    }
                    ui.label(egui::RichText::new(info).size(10.5).color(TEXT_FAINT));
                    ui.horizontal(|ui| {
                        if ui.small_button("To new track").clicked() {
                            let (id, name, duration, kind) = (source.id, file_stem(&source.path), source.duration, source.kind);
                            self.edit("Add source to track", |p| {
                                let t = p.add_track(name, kind);
                                p.add_clip(t, id, 0.0, 0.0, duration);
                            });
                        }
                        if transcribing {
                            ui.label(egui::RichText::new("Transcribing…").size(10.5).color(WARN));
                        } else if let Some(words) = transcript {
                            ui.label(egui::RichText::new(format!("{words} words")).size(10.5).color(GOOD));
                            if ui.small_button("Redo").on_hover_text("Transcribe again with the selected model").clicked() {
                                self.request_transcribe(source.id, true);
                            }
                        } else if ui.small_button("Transcribe").clicked() {
                            self.request_transcribe(source.id, true);
                        }
                    });
                });
                ui.add_space(3.0);
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Speech model").strong().color(TEXT_DIM));
            ui.add_space(2.0);
            self.model_rows(ui);
        });
    }
}
