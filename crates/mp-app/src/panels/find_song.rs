//! Finding a song online, for when you don't have the file. It's where a project starts, so it leads
//! straight on to splitting.

use crate::app::{App, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Ui};
use mp_analysis::online::{default_download_dir, DownloadKind, SongResult};
use mp_model::SourceId;
use std::collections::HashSet;
use std::path::PathBuf;

const LIST_H: f32 = 320.0;

#[derive(Default)]
pub struct FindSong {
    pub open: bool,
    pub query: String,
    pub results: Vec<SongResult>,
    /// The search in progress, whose job also says what's happening.
    pub searching: Option<u64>,
    pub error: Option<String>,
    /// Whether anything has been searched yet, to tell "no results" from not having tried.
    pub searched: bool,
    focused: bool,
    /// Downloads to split as soon as they've been imported.
    pub split_after: HashSet<PathBuf>,
}

impl App {
    pub fn open_find_song(&mut self) {
        self.find.open = true;
        self.find.focused = false;
        self.find.error = None;
    }

    pub fn download_dir(&self) -> PathBuf {
        self.settings.download_dir.clone().unwrap_or_else(default_download_dir)
    }

    fn close_find_song(&mut self) {
        self.find.open = false;
        if let Some(job) = self.find.searching.take() {
            if let Some(job) = self.jobs.active.iter().find(|j| j.id == job) {
                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn start_search(&mut self) {
        let query = self.find.query.trim().to_string();
        if query.is_empty() || self.find.searching.is_some() {
            return;
        }
        self.find.error = None;
        self.find.results.clear();
        self.find.searching = Some(self.jobs.search_songs(query));
    }

    /// For the test script.
    pub fn search_now(&mut self) {
        self.start_search();
    }

    fn get_song(&mut self, result: SongResult, split: bool) {
        self.jobs.download_song(result, self.settings.download_kind, self.download_dir(), split);
        self.close_find_song();
    }

    pub fn on_song_downloaded(&mut self, path: PathBuf, split: bool) {
        if split {
            self.find.split_after.insert(path.clone());
        }
        self.import_path(path);
    }

    /// Goes on to split a download. Straight away if everything's ready, or by asking if it has to install.
    pub fn split_if_asked(&mut self, path: &std::path::Path, source: SourceId) {
        if self.find.split_after.remove(path) {
            self.open_split_ready(source);
        }
    }

    pub fn find_song_modal(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let searching = self.find.searching.and_then(|id| self.jobs.active.iter().find(|j| j.id == id)).map(|j| (j.label.clone(), j.fraction));
        let mut search = false;
        let mut get = None;
        let mut close = false;
        let mut kind = self.settings.download_kind;
        let mut choose_folder = false;
        let folder = self.download_dir().display().to_string();
        let mut query = std::mem::take(&mut self.find.query);
        let mut focused = self.find.focused;
        let results = std::mem::take(&mut self.find.results);
        let error = self.find.error.clone();
        let searched = self.find.searched;
        let dismissed = modal(ctx, "find-song", 560.0, |ui: &mut Ui| {
            ui.label(heading("Find a song"));
            ui.add_space(SP_2);
            ui.horizontal(|ui| {
                let edit = text_field(ui, &mut query, "Song and artist, or paste a link", ui.available_width() - 76.0);
                if !focused {
                    edit.request_focus();
                    focused = true;
                }
                search |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                search |= ui.add(Button::secondary("Search").enabled(searching.is_none())).clicked();
            });
            ui.add_space(SP_2);
            ui.horizontal(|ui| {
                segmented(ui, &mut kind, &[(DownloadKind::Audio, "Audio"), (DownloadKind::Video, "Video")]);
                ui.add_space(SP_2);
                if ui.add(Button::ghost(&format!("Saves to {}", truncate(&folder, 40))).small().tip("Choose where downloads go. Projects keep using these files.", None)).clicked() {
                    choose_folder = true;
                }
            });
            ui.add_space(SP_3);
            if let Some((label, fraction)) = &searching {
                ui.label(caption(format!("{label}…")));
                progress(ui, *fraction, ui.available_width());
            } else if let Some(error) = &error {
                ui.label(caption(error).color(BAD));
            } else if results.is_empty() {
                ui.label(caption(if searched { "Nothing found. Try other words." } else { "Choose Video to cut the picture along with the words." }));
            }
            if !results.is_empty() {
                egui::ScrollArea::vertical().id_salt("song-results").max_height(LIST_H).show(ui, |ui| {
                    for result in &results {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() - 190.0);
                                ui.label(strong(truncate(&result.title, 48)));
                                let length = result.duration.map_or(String::new(), |d| format!(" · {}:{:02}", d as u64 / 60, d as u64 % 60));
                                ui.label(caption(format!("{}{length}", truncate(&result.channel, 36))));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = SP_1;
                                if ui.add(Button::secondary("Add and split").small().tip("Download it, then split it into stems", None)).clicked() {
                                    get = Some((result.clone(), true));
                                }
                                if ui.add(Button::ghost("Add").small().tip("Download it and add it as it is", None)).clicked() {
                                    get = Some((result.clone(), false));
                                }
                            });
                        });
                        ui.add_space(SP_1);
                    }
                });
            }
            ui.add_space(SP_3);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(Button::ghost("Close")).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(caption("Uses yt-dlp. Only download what you have the right to use."));
                });
            });
        });
        self.find.query = query;
        self.find.focused = focused;
        self.find.results = results;
        if kind != self.settings.download_kind {
            self.settings.download_kind = kind;
            self.settings.save();
        }
        if choose_folder {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                self.settings.download_dir = Some(folder);
                self.settings.save();
            }
        }
        if search {
            self.start_search();
        }
        if let Some((result, split)) = get {
            self.get_song(result, split);
        } else if dismissed || close {
            self.close_find_song();
        }
    }

    pub fn on_song_results(&mut self, job: u64, results: Result<Vec<SongResult>, String>) {
        if self.find.searching != Some(job) {
            return;
        }
        self.find.searching = None;
        self.find.searched = true;
        match results {
            Ok(results) => self.find.results = results,
            Err(error) if error == "cancelled" => {}
            Err(error) => self.find.error = Some(error),
        }
    }

    pub fn on_song_failed(&mut self, error: String) {
        if error != "cancelled" {
            self.toast(ToastKind::Bad, format!("Couldn't download that: {error}"));
        }
    }
}
