//! Things done once or rarely: choosing the audio output, and splitting a song into stems.

use crate::app::{App, ToastAction, ToastKind};
use crate::ds::*;
use crate::panels::arranger::truncate;
use eframe::egui::{self, Ui};
use mp_analysis::stems::{self, Setup, StemError, StemFormat, VOCAL_MODELS};
use mp_engine::{list_hosts, list_outputs, system_output_name, AudioConfig, OutputInfo};
use mp_model::{SourceId, TrackKind};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

const BLOCK_SIZES: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];
const LABEL_W: f32 = 104.0;

/// The device lists, read once when the window opens, because reading them can be slow.
pub struct AudioPanel {
    hosts: Vec<String>,
    outputs: Vec<OutputInfo>,
    system: Option<String>,
}

pub struct SplitDialog {
    source: SourceId,
    setup: Setup,
    error: Option<StemError>,
}

#[derive(Default)]
pub struct StemsState {
    pub dialog: Option<SplitDialog>,
    /// The source being split, which is at most one at a time from here.
    pub running: Option<SourceId>,
    /// What to split once the install finishes.
    pub after_install: Option<SourceId>,
    pub dismissed: HashSet<SourceId>,
    /// Sources not transcribed yet because they may be about to be split.
    pub held: Vec<SourceId>,
    /// Stems about to arrive that should show a video's picture.
    pub pending_picture: HashMap<PathBuf, PathBuf>,
}

fn field(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(LABEL_W, CONTROL_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(LABEL_W);
            ui.label(caption(label));
        });
        add(ui);
    });
    ui.add_space(SP_1);
}

impl App {
    pub fn open_audio_settings(&mut self) {
        let host = self.engine.config.host.clone();
        self.audio_panel = Some(AudioPanel { hosts: list_hosts(), outputs: list_outputs(host.as_deref()), system: system_output_name(host.as_deref()) });
    }

    /// Reopens the output with new settings. Playback carries on from the same place.
    pub fn apply_audio(&mut self, config: AudioConfig) {
        let playing = self.engine.is_playing();
        let was_rate = self.sample_rate;
        self.engine.close();
        self.engine = mp_engine::AudioEngine::start(config.clone(), self.wake_ui());
        self.settings.audio = config;
        self.settings.save();
        // Devices belong to the engine that's gone, so the next graph builds new ones.
        self.compiler = mp_engine::GraphCompiler::default();
        self.needs_compile = true;
        self.sample_rate = self.engine.sample_rate;
        if self.sample_rate != was_rate {
            // Audio is held at the engine's rate, so everything is decoded and rendered again.
            self.cache = Arc::new(mp_engine::RenderCache::new());
            self.bank.buffers.clear();
            self.runtime.clear();
            self.spectrograms.clear();
            // The tempo already found isn't to be redetected, and a project with no audio hasn't found one.
            self.tempo_user_set |= !self.project.sources.is_empty();
            for source in &self.project.sources {
                if source.path.exists() {
                    self.jobs.import(source.path.clone(), self.sample_rate, Some(source.id));
                }
            }
        }
        let frame = self.seconds_to_frames(self.playhead);
        self.engine.seek(frame);
        if playing {
            self.engine.play();
        }
        if let Some(problem) = self.engine.problem.clone() {
            self.toast(ToastKind::Bad, problem);
        }
    }

    pub fn wake_ui(&self) -> Arc<dyn Fn() + Send + Sync> {
        let ctx = self.ctx.clone();
        Arc::new(move || ctx.request_repaint())
    }

    /// Reacts to the output going away, or to the system changing to another one.
    pub fn watch_audio_device(&mut self) {
        let lost = self.engine.shared.device_lost.swap(false, std::sync::atomic::Ordering::Relaxed);
        let follows_system = self.engine.config.device.is_none() && self.engine.config.host.is_none();
        let system = self.system_output.lock().clone();
        let moved = follows_system && system.as_ref().is_some_and(|name| *name != self.engine.device_name);
        if lost || moved {
            let config = self.engine.config.clone();
            self.apply_audio(config);
            if self.engine.problem.is_none() {
                self.toast(ToastKind::Info, format!("Now playing through {}", self.engine.device_name));
            }
        }
    }

    fn play_test_tone(&mut self) {
        const SECONDS: f32 = 0.5;
        let rate = self.sample_rate as f32;
        let tone = |hz: f32, i: usize| {
            let t = i as f32 / rate;
            (t * hz * std::f32::consts::TAU).sin() * 0.2 * (1.0 - t / SECONDS).max(0.0)
        };
        let frames = (SECONDS * rate) as usize;
        // Different notes on each side, so that swapped or missing channels can be heard.
        let left = (0..frames).map(|i| tone(440.0, i)).collect();
        let right = (0..frames).map(|i| tone(660.0, i)).collect();
        self.audition_buffer(mp_dsp::Stereo { l: left, r: right });
    }

    pub fn audio_modal(&mut self, ctx: &egui::Context) {
        let Some(mut panel) = self.audio_panel.take() else { return };
        let mut config = self.engine.config.clone();
        let mut close = false;
        let mut test = false;
        let device_name = self.engine.device_name.clone();
        let rate = self.sample_rate;
        let block = self.engine.shared.block.load(std::sync::atomic::Ordering::Relaxed);
        let problem = self.engine.problem.clone();
        let dismissed = modal(ctx, "audio", 440.0, |ui: &mut Ui| {
            ui.label(heading("Audio"));
            ui.add_space(SP_3);
            if panel.hosts.len() > 1 {
                field(ui, "Driver", |ui| {
                    let name = config.host.clone().unwrap_or_else(|| "Automatic".to_string());
                    dropdown(ui, "audio-host", name, 280.0, |ui| {
                        if menu_item(ui, "Automatic", None).clicked() {
                            config.host = None;
                            config.device = None;
                            ui.close();
                        }
                        for host in &panel.hosts {
                            if menu_item(ui, host, None).clicked() {
                                config.host = Some(host.clone());
                                config.device = None;
                                ui.close();
                            }
                        }
                    });
                });
            }
            let system = format!("System output ({})", panel.system.as_deref().unwrap_or("none"));
            field(ui, "Output", |ui| {
                let shown = config.device.clone().unwrap_or_else(|| system.clone());
                dropdown(ui, "audio-output", truncate(&shown, 40), 280.0, |ui| {
                    if menu_item(ui, &system, None).clicked() {
                        config.device = None;
                        config.first_channel = 0;
                        ui.close();
                    }
                    menu_separator(ui);
                    for output in &panel.outputs {
                        if menu_item(ui, &output.name, None).clicked() {
                            config.device = Some(output.name.clone());
                            config.first_channel = 0;
                            ui.close();
                        }
                    }
                });
            });
            let current = panel.outputs.iter().find(|o| o.name == device_name).cloned();
            field(ui, "Sample rate", |ui| {
                let shown = config.sample_rate.map_or(format!("Automatic ({:.1} kHz)", rate as f32 / 1000.0), |r| format!("{:.1} kHz", r as f32 / 1000.0));
                dropdown(ui, "audio-rate", shown, 280.0, |ui| {
                    if menu_item(ui, "Automatic", None).clicked() {
                        config.sample_rate = None;
                        ui.close();
                    }
                    for r in current.iter().flat_map(|o| &o.sample_rates) {
                        if menu_item(ui, &format!("{:.1} kHz", *r as f32 / 1000.0), None).clicked() {
                            config.sample_rate = Some(*r);
                            ui.close();
                        }
                    }
                });
            });
            field(ui, "Block size", |ui| {
                let shown = config.block_size.map_or(if block > 0 { format!("Automatic ({block})") } else { "Automatic".to_string() }, |b| b.to_string());
                dropdown(ui, "audio-block", shown, 280.0, |ui| {
                    if menu_item(ui, "Automatic", None).clicked() {
                        config.block_size = None;
                        ui.close();
                    }
                    let range = current.as_ref().and_then(|o| o.block_range);
                    for size in BLOCK_SIZES.iter().filter(|b| range.is_none_or(|(min, max)| min <= **b && **b <= max)) {
                        if menu_item(ui, &size.to_string(), None).clicked() {
                            config.block_size = Some(*size);
                            ui.close();
                        }
                    }
                });
            });
            let channels = current.as_ref().map_or(2, |o| o.channels);
            if channels > 2 {
                field(ui, "Channels", |ui| {
                    let label = |first: u16| format!("{} and {}", first + 1, first + 2);
                    dropdown(ui, "audio-channels", label(config.first_channel), 280.0, |ui| {
                        for first in (0..channels.saturating_sub(1)).step_by(2) {
                            if menu_item(ui, &label(first), None).clicked() {
                                config.first_channel = first;
                                ui.close();
                            }
                        }
                    });
                });
            }
            ui.add_space(SP_2);
            if let Some(problem) = &problem {
                ui.label(caption(problem).color(BAD));
            } else if block > 0 {
                ui.label(caption(format!("Playing through {device_name}. About {:.1} ms between blocks: lower is quicker to respond, higher is safer against dropouts.", block as f32 * 1000.0 / rate as f32)));
            }
            ui.label(caption("Changing the sample rate reloads the project's audio."));
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(Button::primary("Done")).clicked();
                test = ui.add(Button::secondary("Play a test sound").tip("A lower note on the left, a higher one on the right", None)).clicked();
            });
        });
        if config != self.engine.config {
            let host_changed = config.host != self.engine.config.host;
            self.apply_audio(config);
            if host_changed {
                let host = self.engine.config.host.clone();
                panel.outputs = list_outputs(host.as_deref());
                panel.system = system_output_name(host.as_deref());
            }
        }
        if test {
            self.play_test_tone();
        }
        if !dismissed && !close {
            self.audio_panel = Some(panel);
        }
    }

    // ----- Stems -----

    /// A project that's one song, not yet split, is the moment to offer it.
    pub fn split_offer(&self) -> Option<SourceId> {
        let [source] = self.project.sources.as_slice() else { return None };
        let whole_song = source.kind == TrackKind::Music || source.has_video;
        let on_a_track = self.project.tracks.iter().flat_map(|t| &t.clips).any(|c| c.source == source.id);
        (whole_song && on_a_track && self.stems.running != Some(source.id) && !self.stems.dismissed.contains(&source.id)).then_some(source.id)
    }

    /// Once a song has been decided on, and wasn't split, its words are found after all.
    pub fn release_held_transcriptions(&mut self) {
        if self.stems.held.is_empty() || self.stems.running.is_some() || self.stems.after_install.is_some() || self.stems.dialog.is_some() {
            return;
        }
        let offered = self.split_offer();
        let ready: Vec<SourceId> = self.stems.held.iter().copied().filter(|s| Some(*s) != offered).collect();
        self.stems.held.retain(|s| Some(*s) == offered);
        for source in ready {
            let on_a_track = self.project.tracks.iter().flat_map(|t| &t.clips).any(|c| c.source == source);
            if on_a_track {
                self.request_transcribe(source, false);
            }
        }
    }

    pub fn split_offer_card(&mut self, ui: &mut Ui, source: SourceId) {
        let is_music = self.project.source(source).is_some_and(|s| s.kind == TrackKind::Music);
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Is this the whole song?"));
            ui.label(caption("Split it into vocals, drums, bass and other, so that you're cutting the voice alone and the words are found more reliably."));
            ui.add_space(SP_1);
            ui.horizontal_wrapped(|ui| {
                if ui.add(Button::primary("Split into stems")).clicked() {
                    self.open_split(source);
                }
                let label = if is_music { "It's only a voice" } else { "No need" };
                if ui.add(Button::ghost(label).small()).clicked() {
                    self.stems.dismissed.insert(source);
                    if is_music {
                        self.treat_as_voice(source);
                    }
                }
            });
        });
        ui.add_space(SP_2);
    }

    fn treat_as_voice(&mut self, source: SourceId) {
        self.edit("Treat as a voice", |p| {
            if let Some(s) = p.sources.iter_mut().find(|s| s.id == source) {
                s.kind = TrackKind::Vocal;
            }
            for track in p.tracks.iter_mut().filter(|t| t.clips.iter().any(|c| c.source == source)) {
                track.kind = TrackKind::Vocal;
            }
        });
        self.request_transcribe(source, false);
    }

    /// The source of the selected clip or track.
    pub fn split_selected(&mut self) {
        let from_clip = self.selection.first().and_then(|c| self.project.clip(*c)).map(|c| c.source);
        let from_track = self.selected_track.and_then(|t| self.project.track(t)).and_then(|t| t.clips.first()).map(|c| c.source);
        match from_clip.or(from_track) {
            Some(source) => self.open_split(source),
            None => self.toast(ToastKind::Info, "Select a track or clip with a song on it"),
        }
    }

    pub fn open_split(&mut self, source: SourceId) {
        let Some(path) = self.project.source(source).map(|s| s.path.clone()) else { return };
        if self.stems.running.is_some() || self.stems.after_install.is_some() {
            self.toast(ToastKind::Info, "Already splitting a song. Wait for it to finish, or cancel it in the status bar.");
            return;
        }
        // Already done for this file, so there's nothing to wait for.
        if let Some(files) = stems::existing(&path, &self.settings.stems) {
            self.finish_stems(source, files);
            return;
        }
        self.stems.dialog = Some(SplitDialog { source, setup: stems::setup(), error: None });
    }

    /// For when splitting was already asked for: only stops to ask if there's a large download first.
    pub fn open_split_ready(&mut self, source: SourceId) {
        self.open_split(source);
        if self.stems.dialog.as_ref().is_some_and(|d| d.setup == Setup::Ready) {
            self.stems.dialog = None;
            self.start_split(source, false);
        }
    }

    fn start_split(&mut self, source: SourceId, low_memory: bool) {
        let Some((name, path)) = self.project.source(source).map(|s| (s.name.clone(), s.path.clone())) else { return };
        self.stems.running = Some(source);
        // Words found from the whole mix would only be thrown away.
        let transcribing = format!("Transcribing {name}");
        if let Some(job) = self.jobs.active.iter().find(|j| j.label == transcribing) {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.jobs.split_stems(source, &name, path, self.settings.stems.clone(), low_memory);
    }

    pub fn on_stems_installed(&mut self) {
        if let Some(source) = self.stems.after_install.take() {
            self.start_split(source, false);
        }
    }

    pub fn on_stems_failed(&mut self, source: Option<SourceId>, error: StemError) {
        self.stems.running = None;
        let source = source.or(self.stems.after_install.take());
        if error == StemError::Cancelled {
            return;
        }
        if matches!(error, StemError::BrokenInstall(_)) {
            stems::forget_install();
        }
        match source {
            Some(source) => {
                self.stems.dialog = Some(SplitDialog { source, setup: stems::setup(), error: Some(error) });
            }
            None => self.toast(ToastKind::Bad, error.message()),
        }
    }

    /// Puts the stems on tracks. A track with only the untouched song goes, since the stems replace it.
    /// One with cuts in it is someone's work, so it stays, muted.
    pub fn finish_stems(&mut self, source: SourceId, files: BTreeMap<String, PathBuf>) {
        self.stems.running = None;
        self.stems.held.retain(|s| *s != source);
        let Some(original) = self.project.source(source).cloned() else { return };
        let duration = original.duration;
        self.edit("Split into stems", |p| {
            let whole = |t: &mp_model::Track| {
                let [clip] = t.clips.as_slice() else { return false };
                clip.source == source && clip.start == 0.0 && (clip.length() - duration).abs() < 0.01
            };
            let (remove, mute): (Vec<_>, Vec<_>) = p.tracks.iter().filter(|t| t.clips.iter().any(|c| c.source == source)).map(|t| (t.id, whole(t))).partition(|(_, whole)| *whole);
            for (id, _) in remove {
                p.remove_track(id);
            }
            for (id, _) in mute {
                if let Some(t) = p.track_mut(id) {
                    t.mute = true;
                }
            }
        });
        self.selection.retain(|c| self.project.clip(*c).is_some());
        if self.selected_track.is_some_and(|t| self.project.track(t).is_none()) {
            self.selected_track = None;
        }
        // The vocal is what gets cut, so its clips carry the picture of a video.
        if original.has_video {
            if let Some(vocals) = files.get("Vocals") {
                let picture = original.picture_path.clone().unwrap_or(original.path.clone());
                self.stems.pending_picture.insert(vocals.clone(), picture);
            }
        }
        for name in stems::TRACK_STEMS {
            if let Some(path) = files.get(name) {
                self.jobs.import(path.clone(), self.sample_rate, None);
            }
        }
        self.tempo_priority = 0;
        let folder = files.get("Vocals").and_then(|p| p.parent()).map(PathBuf::from);
        let text = "Split into vocals, drums, bass and other";
        match folder {
            Some(folder) => self.toast_with(ToastKind::Good, text, ToastAction::Reveal(folder)),
            None => self.toast(ToastKind::Good, text),
        }
    }

    pub fn split_modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.stems.dialog.take() else { return };
        let name = self.project.source(dialog.source).map_or(String::new(), |s| s.name.clone());
        let mut close = false;
        let mut go = false;
        let mut less_memory = false;
        let mut settings = self.settings.stems.clone();
        let dismissed = modal(ctx, "split", 460.0, |ui: &mut Ui| {
            ui.label(heading(&format!("Split {} into stems", truncate(&name, 30))));
            ui.add_space(SP_1);
            ui.label(caption("Makes vocals, drums, bass and other, and puts them on tracks in its place. It runs on this computer, and you can keep working while it does. The file itself isn't changed."));
            ui.add_space(SP_3);
            field(ui, "Vocal model", |ui| {
                let shown = VOCAL_MODELS.iter().find(|(id, _)| *id == settings.vocal_model).map_or(settings.vocal_model.clone(), |(_, label)| label.to_string());
                dropdown(ui, "stem-model", shown, 200.0, |ui| {
                    for (id, label) in VOCAL_MODELS {
                        if menu_item(ui, label, None).clicked() {
                            settings.vocal_model = id.to_string();
                            ui.close();
                        }
                    }
                });
            });
            field(ui, "Files", |ui| {
                segmented(ui, &mut settings.format, &[(StemFormat::Wav, "WAV"), (StemFormat::Flac, "FLAC"), (StemFormat::Mp3, "MP3")]);
            });
            field(ui, "Models kept in", |ui| {
                let shown = settings.model_dir().display().to_string();
                if ui.add(Button::ghost(&truncate(&shown, 40)).small().tip("Models are large. Choose another folder or drive.", None)).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        settings.model_dir = Some(folder);
                    }
                }
            });
            ui.add_space(SP_2);
            if let Some(error) = &dialog.error {
                ui.label(caption(error.message()).color(BAD));
                ui.add_space(SP_2);
            }
            ui.label(caption(match dialog.setup {
                Setup::Ready => "Takes minutes with a GPU, and much longer without one. The first split also downloads the models.",
                Setup::NeedsInstall => "The first time, this gets everything it needs by itself: the separation software, its models and ffmpeg. That's several gigabytes. After that it works offline.",
                Setup::NeedsRepair => "The separation software needs a quick repair first. This only fetches what's missing.",
            }));
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), CONTROL_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match dialog.setup {
                    Setup::Ready => "Split",
                    Setup::NeedsInstall => "Download and split",
                    Setup::NeedsRepair => "Repair and split",
                };
                go = ui.add(Button::primary(label)).clicked();
                if matches!(dialog.error, Some(StemError::OutOfMemory(_))) {
                    less_memory = ui.add(Button::secondary("Split using less memory")).clicked();
                }
                close = ui.add(Button::ghost("Cancel")).clicked();
            });
        });
        if settings != self.settings.stems {
            self.settings.stems = settings;
            self.settings.save();
        }
        if go || less_memory {
            match dialog.setup {
                Setup::NeedsInstall | Setup::NeedsRepair => {
                    self.stems.after_install = Some(dialog.source);
                    self.jobs.install_stems();
                }
                Setup::Ready => self.start_split(dialog.source, less_memory),
            }
        } else if !dismissed && !close {
            self.stems.dialog = Some(dialog);
        }
    }
}
