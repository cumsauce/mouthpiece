//! Correcting a transcript with the song's real lyrics, found online or pasted in.

use crate::app::{App, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Ui};
use mp_analysis::lyrics::{guess_query, LyricsMatch};
use mp_model::SourceId;

const LIST_H: f32 = 280.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Find,
    Paste,
}

pub struct LyricsDialog {
    source: SourceId,
    mode: Mode,
    query: String,
    pasted: String,
    results: Vec<LyricsMatch>,
    searching: bool,
    searched: bool,
    error: Option<String>,
}

impl App {
    pub fn open_lyrics(&mut self, source: SourceId) {
        let Some(info) = self.project.source(source) else { return };
        let folder = info.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let query = guess_query(&info.name, folder.as_deref());
        self.lyrics = Some(LyricsDialog { source, mode: Mode::Find, query, pasted: String::new(), results: Vec::new(), searching: false, searched: false, error: None });
        self.search_lyrics();
    }

    /// For the test script.
    pub fn open_lyrics_for_first(&mut self) {
        if let Some(source) = self.project.sources.first().map(|s| s.id) {
            self.open_lyrics(source);
        }
    }

    fn search_lyrics(&mut self) {
        let length = self.lyrics.as_ref().and_then(|d| self.project.source(d.source)).map_or(0.0, |s| s.duration);
        let Some(dialog) = &mut self.lyrics else { return };
        let query = dialog.query.trim().to_string();
        if query.is_empty() {
            return;
        }
        dialog.searching = true;
        dialog.error = None;
        self.jobs.search_lyrics(query, length);
    }

    pub fn on_lyrics_results(&mut self, results: Result<Vec<LyricsMatch>, String>) {
        let Some(dialog) = &mut self.lyrics else { return };
        dialog.searching = false;
        dialog.searched = true;
        match results {
            Ok(results) => dialog.results = results,
            Err(error) => dialog.error = Some(error),
        }
    }

    /// Corrects a source's words with lyrics, in the background.
    pub fn apply_lyrics(&mut self, source: SourceId, text: String) {
        let Some(transcript) = self.index.transcript(source.0).cloned() else { return };
        let (Some(runtime), Some(info)) = (self.runtime.get(&source), self.project.source(source)) else { return };
        self.jobs.apply_lyrics(source, info.path.clone(), transcript, text, runtime.mono.clone(), self.sample_rate as f32);
    }

    pub fn on_lyrics_applied(&mut self, source: SourceId, transcript: mp_analysis::Transcript, report: mp_analysis::lyrics::LyricsReport) {
        self.install_corrected(source, transcript);
        let mut text = format!("Lyrics matched {} words: changed {} and added {}", report.confirmed + report.replaced, report.replaced, report.added);
        if report.left_out > 0 {
            text.push_str(&format!(". {} {} left out, not seeming to be in this recording", report.left_out, if report.left_out == 1 { "was" } else { "were" }));
        }
        self.toast(ToastKind::Good, text);
    }

    /// Goes back to what was heard before lyrics were applied.
    fn undo_lyrics(&mut self, source: SourceId) {
        let Some(mut transcript) = self.index.transcript(source.0).cloned() else { return };
        let Some(words) = transcript.before_lyrics.take() else { return };
        transcript.words = words;
        if let Some(path) = self.project.source(source).map(|s| s.path.clone()) {
            mp_analysis::transcribe::save_cached(&path, &transcript.model.clone(), &transcript);
        }
        self.install_corrected(source, transcript);
        self.toast(ToastKind::Info, "Back to the words as they were heard");
    }

    fn install_corrected(&mut self, source: SourceId, transcript: mp_analysis::Transcript) {
        // Changes remember which words they replace by position, and those positions have moved.
        if let Some(old) = self.index.transcript(source.0) {
            let moved = |index: usize| -> usize {
                let at = old.words.get(index).map_or(0.0, |w| w.start);
                transcript
                    .words
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| (a.start - at).abs().total_cmp(&(b.start - at).abs()))
                    .map_or(0, |(i, _)| i)
            };
            let remapped: Vec<(usize, usize, usize)> = self.project.patches.iter().enumerate().filter(|(_, p)| p.source == source).map(|(i, p)| (i, moved(p.first_word), moved(p.last_word))).collect();
            for (patch, first, last) in remapped {
                self.project.patches[patch].first_word = first;
                self.project.patches[patch].last_word = last.max(first);
            }
        }
        self.index.set_transcript(source.0, transcript);
        self.save_packaged_transcript(source);
        self.say.word_selection = None;
        self.say.dirty = true;
        self.takes_memo = None;
    }

    /// The row under a transcript's name in the Lyrics tab.
    pub fn lyrics_row(&mut self, ui: &mut Ui, source: SourceId) {
        let corrected = self.index.transcript(source.0).is_some_and(|t| t.before_lyrics.is_some());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP_1;
            if corrected {
                status_dot(ui, Tone::Good, "Corrected with lyrics");
                if ui.add(Button::ghost("Undo").small().tip("Go back to the words as they were heard", None)).clicked() {
                    self.undo_lyrics(source);
                }
                if ui.add(Button::ghost("Other lyrics").small()).clicked() {
                    self.open_lyrics(source);
                }
            } else if ui.add(Button::ghost("Improve with lyrics").small().icon(Icon::Wand).tip("Use the song's real lyrics to correct what was heard", None)).clicked() {
                self.open_lyrics(source);
            }
        });
    }

    pub fn lyrics_modal(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.lyrics.take() else { return };
        let mut search = false;
        let mut use_text = None;
        let mut close = false;
        let applying = self.jobs.active.iter().any(|j| j.label == "Applying lyrics");
        let dismissed = modal(ctx, "lyrics", 560.0, |ui: &mut Ui| {
            ui.label(heading("Improve with lyrics"));
            ui.add_space(SP_1);
            ui.label(caption("The words come from the lyrics and the timing from the recording. Anything sung that isn't in the lyrics stays, and you can undo this."));
            ui.add_space(SP_2);
            segmented(ui, &mut dialog.mode, &[(Mode::Find, "Find online"), (Mode::Paste, "Paste")]);
            ui.add_space(SP_2);
            match dialog.mode {
                Mode::Find => {
                    ui.horizontal(|ui| {
                        let edit = text_field(ui, &mut dialog.query, "Song and artist", ui.available_width() - 76.0);
                        search |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        search |= ui.add(Button::secondary("Search").enabled(!dialog.searching)).clicked();
                    });
                    ui.add_space(SP_2);
                    if dialog.searching {
                        progress(ui, None, ui.available_width());
                    } else if let Some(error) = &dialog.error {
                        ui.label(caption(error).color(BAD));
                    } else if dialog.results.is_empty() && dialog.searched {
                        ui.label(caption("Nothing found. Try other words, or paste them in."));
                    }
                    if !dialog.results.is_empty() {
                        egui::ScrollArea::vertical().id_salt("lyric-results").max_height(LIST_H).show(ui, |ui| {
                            for found in &dialog.results {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(ui.available_width() - 70.0);
                                        ui.label(strong(truncate(&found.track, 52)));
                                        let length = found.duration.map_or(String::new(), |d| format!(" · {}:{:02}", d as u64 / 60, d as u64 % 60));
                                        let timed = if found.timed { " · Timed" } else { "" };
                                        ui.label(caption(format!("{}{length}{timed}", truncate(&found.artist, 36))));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add(Button::secondary("Use").small().enabled(!applying)).clicked() {
                                            use_text = Some(found.text.clone());
                                        }
                                    });
                                });
                                ui.add_space(SP_1);
                            }
                        });
                    }
                }
                Mode::Paste => {
                    let frame = egui::Frame::NONE.fill(BASE).corner_radius(R_S).inner_margin(egui::Margin::same(SP_2 as i8));
                    frame.show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("lyric-paste").max_height(LIST_H).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut dialog.pasted).frame(egui::Frame::NONE).font(font_input()).desired_rows(10).desired_width(f32::INFINITY).hint_text("Paste the lyrics here"));
                        });
                    });
                    ui.add_space(SP_2);
                    let ready = !dialog.pasted.trim().is_empty() && !applying;
                    if ui.add(Button::primary("Use these lyrics").enabled(ready)).clicked() {
                        use_text = Some(dialog.pasted.clone());
                    }
                }
            }
            ui.add_space(SP_3);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(Button::ghost("Close")).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(caption(if dialog.mode == Mode::Find { "Searches LRCLIB. Only the search words are sent." } else { "Stays on this computer." }));
                });
            });
        });
        let source = dialog.source;
        if !dismissed && !close && use_text.is_none() {
            self.lyrics = Some(dialog);
        }
        if search {
            self.search_lyrics();
        }
        if let Some(text) = use_text {
            self.apply_lyrics(source, text);
        }
    }
}
