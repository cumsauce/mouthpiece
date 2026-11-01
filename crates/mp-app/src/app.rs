//! Application state, and the operations that the panels and commands call.

use crate::panels::editor::EditorState;
use crate::panels::lane::ClipDrag;
use crate::panels::render::ExportDialog;
use crate::panels::say::{SayState, SayTab};
use crate::panels::timeline::TimeView;
use crate::jobs::{file_stem, JobEvent, Jobs};
use crate::script::Script;
use eframe::egui::{self, TextureHandle};
use mp_analysis::peaks::Peaks;
use mp_analysis::search::{Hit, WordIndex};
use mp_analysis::spectrogram::Spectrogram;
use mp_analysis::transcribe::{self, TranscribeRequest, MODELS};
use mp_engine::compile::audition_voice;
use mp_engine::{AudioEngine, GraphCompiler, RenderCache, SourceBank};
use mp_model::{ClipId, History, MusicalKey, Project, SourceId, TrackId, TrackKind};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub const FILE_EXTENSION: &str = "mouthpiece";
const AUTOSAVE_SECONDS: u64 = 30;
const RENDER_CACHE_LIMIT: usize = 1 << 31;

pub struct SourceRuntime {
    pub peaks: Arc<Peaks>,
    pub mono: Arc<Vec<f32>>,
}

pub enum SpectrogramState {
    Pending,
    Ready { spectrogram: Arc<Spectrogram>, tiles: Vec<TextureHandle> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BottomTab {
    Editor,
    Devices,
    Mixer,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub action: Option<ToastAction>,
    pub born: Instant,
}

/// A button on a toast.
#[derive(Clone, Debug)]
pub enum ToastAction {
    Undo,
    Reveal(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Good,
    Bad,
}

/// What to do once the user has decided about unsaved changes.
#[derive(Clone, Debug, PartialEq)]
pub enum AfterDiscard {
    New,
    Open,
    OpenPath(PathBuf),
    Quit,
}

/// The dialog that names a word.
pub struct WordDialog {
    pub name: String,
    pub clips: Vec<ClipId>,
    /// Set when renaming a saved word, rather than saving clips as one.
    pub renaming: Option<mp_model::WordId>,
    pub focused: bool,
}

pub struct App {
    pub ctx: egui::Context,
    pub project: Project,
    pub history: History,
    pub project_path: Option<PathBuf>,
    pub saved_state: Project,

    pub engine: AudioEngine,
    pub compiler: GraphCompiler,
    pub bank: SourceBank,
    pub cache: Arc<RenderCache>,
    pub sample_rate: u32,
    pub runtime: HashMap<SourceId, SourceRuntime>,
    pub index: WordIndex,
    pub spectrograms: HashMap<SourceId, SpectrogramState>,
    pub needs_compile: bool,
    last_cache_revision: u64,

    pub selection: Vec<ClipId>,
    pub selected_track: Option<TrackId>,
    pub master_selected: bool,
    pub arr_view: TimeView,
    pub arr_scroll_y: f32,
    pub editor: EditorState,
    pub say: SayState,
    pub grid_division: f64,
    pub snap: bool,
    pub snap_line: Option<f64>,
    pub clip_drag: Option<ClipDrag>,
    pub marquee: Option<egui::Rect>,
    pub gesture: Option<(String, Project)>,
    pub hit_drag: Option<Hit>,
    pub clipboard: Vec<(mp_model::Clip, TrackId)>,
    pub word_drag: Option<mp_model::WordId>,
    pub selected_patch: Option<mp_model::PatchId>,
    pub trial: Option<crate::panels::trial::Trial>,
    pub word_dialog: Option<WordDialog>,
    pub video: HashMap<SourceId, Arc<mp_analysis::video::VideoProxy>>,
    pub video_texture: Option<TextureHandle>,
    pub video_frame: Option<(SourceId, usize)>,
    pub show_video: bool,
    pub picture: crate::panels::picture::PictureState,
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Whether Delete means the selected track: true after clicking its header, until anything else is clicked.
    pub track_focus: bool,
    /// A track being renamed, and its name so far.
    pub rename_track: Option<(TrackId, String, bool)>,
    /// What's being auditioned, and from where, so the picture can follow it.
    pub audition_source: Option<(SourceId, f64)>,

    pub show_inspector: bool,
    pub show_say: bool,
    pub show_bottom: bool,
    pub bottom_tab: BottomTab,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_index: usize,
    pub show_shortcuts: bool,
    pub export_dialog: Option<ExportDialog>,
    pub pending_discard: Option<AfterDiscard>,
    pub allow_close: bool,
    last_title: String,
    /// The last measured clip range and its pitch, as measuring is too slow to repeat each frame.
    pitch_memo: Option<((SourceId, u64, u64), Option<f32>)>,
    pub toasts: Vec<Toast>,
    pub jobs: Jobs,
    pub hint: String,
    /// Where the arranger lanes are on screen, for things that float over them.
    pub arr_lanes_rect: egui::Rect,
    /// Clips that were just placed, and when, to draw attention to them.
    pub flash: HashMap<ClipId, Instant>,
    /// Takes of the last word looked up, as this is needed every frame while a clip is selected.
    takes_memo: Option<(String, Vec<Hit>)>,

    pub playhead: f64,
    pub play_start: f64,
    /// Track index to level, decayed slowly so meters read smoothly.
    pub meter_hold: HashMap<usize, [f32; 2]>,
    pub master_hold: [f32; 2],
    tempo_priority: u8,
    /// Shown once a batch of imports is done, so a later stem's better tempo isn't announced twice.
    tempo_toast: Option<String>,
    pub tempo_user_set: bool,
    tap_times: Vec<Instant>,
    pub selected_model: usize,
    pub script: Option<Script>,
    pub script_dropped: Vec<egui::DroppedFileHandle>,
    pub inject_frames: VecDeque<(Vec<egui::Event>, egui::Modifiers)>,
    last_autosave: Instant,
}

impl App {
    pub fn new(ctx: egui::Context, args: Vec<PathBuf>) -> Self {
        let engine = AudioEngine::start();
        let sample_rate = engine.sample_rate;
        let project = Project::default();
        let installed = MODELS.iter().position(|m| m.is_installed());
        let mut app = Self {
            jobs: Jobs::new(ctx.clone()),
            ctx,
            saved_state: project.clone(),
            project,
            history: History::default(),
            project_path: None,
            engine,
            compiler: GraphCompiler::default(),
            bank: SourceBank::default(),
            cache: Arc::new(RenderCache::new()),
            sample_rate,
            runtime: HashMap::new(),
            index: WordIndex::default(),
            spectrograms: HashMap::new(),
            needs_compile: true,
            last_cache_revision: 0,
            selection: Vec::new(),
            selected_track: None,
            master_selected: false,
            arr_view: TimeView::new(24.0),
            arr_scroll_y: 0.0,
            editor: EditorState::default(),
            say: SayState::default(),
            grid_division: 1.0,
            snap: true,
            snap_line: None,
            clip_drag: None,
            marquee: None,
            gesture: None,
            hit_drag: None,
            clipboard: Vec::new(),
            word_drag: None,
            selected_patch: None,
            trial: None,
            word_dialog: None,
            video: HashMap::new(),
            video_texture: None,
            video_frame: None,
            show_video: true,
            picture: Default::default(),
            render_state: None,
            track_focus: false,
            rename_track: None,
            audition_source: None,
            show_inspector: true,
            show_say: true,
            show_bottom: true,
            bottom_tab: BottomTab::Editor,
            palette_open: false,
            palette_query: String::new(),
            palette_index: 0,
            show_shortcuts: false,
            export_dialog: None,
            pending_discard: None,
            allow_close: false,
            last_title: String::new(),
            pitch_memo: None,
            toasts: Vec::new(),
            hint: String::new(),
            arr_lanes_rect: egui::Rect::ZERO,
            flash: HashMap::new(),
            takes_memo: None,
            playhead: 0.0,
            play_start: 0.0,
            meter_hold: HashMap::new(),
            master_hold: [0.0; 2],
            tempo_priority: 0,
            tempo_toast: None,
            tempo_user_set: false,
            tap_times: Vec::new(),
            selected_model: installed.unwrap_or(1),
            script: Script::from_env(),
            script_dropped: Vec::new(),
            inject_frames: VecDeque::new(),
            last_autosave: Instant::now(),
        };
        for path in args {
            if path.extension().is_some_and(|e| e == FILE_EXTENSION) {
                app.open_project_at(&path);
            } else {
                app.import_path(path);
            }
        }
        app
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), kind, action: None, born: Instant::now() });
    }

    pub fn toast_with(&mut self, kind: ToastKind, text: impl Into<String>, action: ToastAction) {
        self.toasts.push(Toast { text: text.into(), kind, action: Some(action), born: Instant::now() });
    }

    // ----- Editing -----

    /// Runs a single undoable edit.
    pub fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Project) -> R) -> R {
        let before = self.project.clone();
        let result = f(&mut self.project);
        if self.project != before {
            self.history.push(label, before);
            self.needs_compile = true;
        }
        result
    }

    /// Starts a continuous edit, such as a drag: all changes until `end_gesture` are one undo step.
    pub fn begin_gesture(&mut self, label: &str) {
        if self.gesture.is_none() {
            self.gesture = Some((label.to_string(), self.project.clone()));
        }
    }

    pub fn end_gesture(&mut self) {
        if let Some((label, before)) = self.gesture.take() {
            if before != self.project {
                self.history.push(label, before);
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.project != self.saved_state
    }

    pub fn undo(&mut self) {
        if self.trial.is_some() {
            self.cancel_trial();
            return;
        }
        match self.history.undo(&mut self.project) {
            Some(label) => {
                self.after_history_change();
                self.toast(ToastKind::Info, format!("Undid {label}"));
            }
            None => self.toast(ToastKind::Info, "Nothing to undo"),
        }
    }

    pub fn redo(&mut self) {
        match self.history.redo(&mut self.project) {
            Some(label) => {
                self.after_history_change();
                self.toast(ToastKind::Info, format!("Redid {label}"));
            }
            None => self.toast(ToastKind::Info, "Nothing to redo"),
        }
    }

    fn after_history_change(&mut self) {
        self.needs_compile = true;
        self.selection.retain(|c| self.project.clip(*c).is_some());
        if self.selected_track.is_some_and(|t| self.project.track(t).is_none()) {
            self.selected_track = None;
        }
    }

    // ----- Transport -----

    pub fn seconds_to_frames(&self, t: f64) -> u64 {
        (t.max(0.0) * self.sample_rate as f64).round() as u64
    }

    pub fn toggle_play(&mut self) {
        if self.engine.is_playing() {
            self.engine.stop();
        } else {
            self.play_start = self.playhead;
            self.engine.audition(None);
            self.engine.play();
        }
    }

    /// Stops and returns to where playback started, or to the beginning if already stopped.
    pub fn stop(&mut self) {
        if self.engine.is_playing() {
            self.engine.stop();
            self.seek(self.play_start);
        } else {
            self.seek(0.0);
        }
        self.engine.audition(None);
    }

    pub fn seek(&mut self, t: f64) {
        self.playhead = t.max(0.0);
        let frame = self.seconds_to_frames(self.playhead);
        self.engine.seek(frame);
    }

    pub fn tap_tempo(&mut self) {
        let now = Instant::now();
        if self.tap_times.last().is_some_and(|t| now.duration_since(*t).as_secs_f64() > 2.0) {
            self.tap_times.clear();
        }
        self.tap_times.push(now);
        if self.tap_times.len() >= 3 {
            let span = now.duration_since(self.tap_times[0]).as_secs_f64();
            let bpm = 60.0 * (self.tap_times.len() - 1) as f64 / span;
            self.set_bpm(bpm.round());
        }
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        self.tempo_user_set = true;
        self.edit("Change tempo", |p| p.bpm = bpm.clamp(20.0, 400.0));
    }

    /// Plays part of a source right away through the preview path.
    pub fn audition(&mut self, source: SourceId, from: f64, to: f64, reversed: bool) {
        if let Some(buffer) = self.bank.get(source) {
            let voice = audition_voice(buffer.clone(), from, to, reversed, self.sample_rate);
            self.engine.audition(voice);
            self.audition_source = Some((source, if reversed { to } else { from }));
        }
    }

    pub fn audition_hit(&mut self, hit: &Hit) {
        self.audition(SourceId(hit.source), hit.start, hit.end, false);
    }

    // ----- Sentence mixing -----

    /// The track that spliced words land on, created on first use. Creating it mutes the original
    /// vocals, since hearing both at once is never what a sentence mix wants.
    pub fn sentence_track(project: &mut Project) -> TrackId {
        if let Some(t) = project.tracks.iter().find(|t| t.name == "Sentence") {
            return t.id;
        }
        for t in &mut project.tracks {
            if t.kind == TrackKind::Vocal {
                t.mute = true;
            }
        }
        project.add_track("Sentence", TrackKind::Vocal)
    }

    /// A hit's range with slight padding, so that a word's attack is never clipped.
    pub fn padded_range(project: &Project, hit: &Hit) -> Option<(f64, f64)> {
        const PAD_BEFORE: f64 = 0.005;
        const PAD_AFTER: f64 = 0.01;
        let duration = project.source(SourceId(hit.source))?.duration;
        Some(((hit.start - PAD_BEFORE).max(0.0), (hit.end + PAD_AFTER).min(duration)))
    }

    /// Places a word at a position.
    pub fn place_hit_at(project: &mut Project, hit: &Hit, track: TrackId, at: f64) -> Option<ClipId> {
        let (a, b) = Self::padded_range(project, hit)?;
        project.add_slice(track, SourceId(hit.source), at, a, b, Some(hit.text.clone()))
    }

    /// Puts a search hit at the playhead on the Sentence track, then moves the playhead after it,
    /// so that the next word lands right behind this one.
    pub fn place_hit_at_playhead(&mut self, hit: &Hit) {
        let at = self.playhead;
        let placed = self.edit(&format!("Place \"{}\"", hit.text), |p| {
            let track = Self::sentence_track(p);
            Self::place_hit_at(p, hit, track, at).map(|c| (c, track))
        });
        if let Some((clip, track)) = placed {
            self.selection = vec![clip];
            self.selected_track = Some(track);
            if let Some(end) = self.project.clip(clip).map(|c| c.end()) {
                self.seek(end);
            }
            self.after_place(&[clip], track);
        }
    }

    /// Which clips a click on one should select: its whole group, or if that's already selected,
    /// just the one, so that a piece of a word can be adjusted by itself.
    pub fn click_selection(&self, clip: ClipId) -> Vec<ClipId> {
        let members = self.project.group_members(clip);
        let whole_group_selected = members.len() > 1 && members.len() == self.selection.len() && members.iter().all(|m| self.selection.contains(m));
        if whole_group_selected { vec![clip] } else { members }
    }

    /// Mixes a saved word down to audio, for hearing it without placing it.
    pub fn render_word(&self, id: mp_model::WordId) -> mp_dsp::Stereo {
        let mut out = mp_dsp::Stereo::default();
        let Some(word) = self.project.word(id) else { return out };
        let mut project = Project::default();
        project.master.devices.clear();
        let track = project.add_track("Word", TrackKind::Vocal);
        if let Some(t) = project.track_mut(track) {
            t.clips = word.parts.clone();
        }
        let graph = GraphCompiler::default().compile(&project, &self.bank, &self.cache, self.sample_rate);
        let end = (word.length() * self.sample_rate as f64) as u64;
        mp_engine::render_offline(
            graph,
            self.sample_rate as f32,
            0,
            end,
            0,
            |l, r| {
                out.l.extend_from_slice(l);
                out.r.extend_from_slice(r);
            },
            |_| true,
        );
        out
    }

    pub fn audition_buffer(&mut self, buffer: mp_dsp::Stereo) {
        let seconds = buffer.frames() as f64 / self.sample_rate as f64;
        if seconds > 0.0 {
            let voice = audition_voice(Arc::new(buffer), 0.0, seconds, false, self.sample_rate);
            self.engine.audition(voice);
            self.audition_source = None;
        }
    }

    pub fn audition_word(&mut self, id: mp_model::WordId) {
        let buffer = self.render_word(id);
        self.audition_buffer(buffer);
    }

    pub fn flash_clips(&mut self, clips: &[ClipId]) {
        let now = Instant::now();
        self.flash.extend(clips.iter().map(|c| (*c, now)));
    }

    /// Makes sure that clips just placed are on screen and noticed.
    pub fn after_place(&mut self, clips: &[ClipId], track: TrackId) {
        self.flash_clips(clips);
        if let Some(start) = clips.first().and_then(|c| self.project.clip(*c)).map(|c| c.start) {
            self.arr_view.follow(start);
        }
        if let Some(end) = clips.last().and_then(|c| self.project.clip(*c)).map(|c| c.end()) {
            self.arr_view.follow(end);
        }
        let mut top = 0.0;
        for t in &self.project.tracks {
            if t.id == track {
                let (height, visible) = (t.height, self.arr_lanes_rect.height());
                if top < self.arr_scroll_y {
                    self.arr_scroll_y = top;
                } else if top + height > self.arr_scroll_y + visible {
                    self.arr_scroll_y = top + height - visible;
                }
                break;
            }
            top += t.height + 1.0;
        }
    }

    /// Every take that could replace a placed clip: the word or phrase it says, or the sound it is.
    pub fn takes_of_label(&mut self, label: &str) -> Vec<Hit> {
        if let Some((memo, takes)) = &self.takes_memo {
            if memo == label {
                return takes.clone();
            }
        }
        let tokens: Vec<String> = label.split_whitespace().map(str::to_lowercase).collect();
        let is_sound = !tokens.is_empty() && tokens.iter().all(|t| mp_analysis::g2p::ALL_PHONEMES.contains(&t.as_str()));
        let mut takes = if tokens.len() > 1 { self.index.find_phrase(label) } else { self.index.takes_of(label) };
        if takes.is_empty() && is_sound {
            takes = self.index.find_phonemes(&tokens);
        }
        self.takes_memo = Some((label.to_string(), takes.clone()));
        takes
    }

    pub fn takes_of_clip(&mut self, clip: &mp_model::Clip) -> Vec<Hit> {
        match &clip.label {
            Some(label) => {
                let label = label.clone();
                self.takes_of_label(&label)
            }
            None => Vec::new(),
        }
    }

    /// Which of `takes` a clip is currently playing.
    pub fn current_take(&self, clip: &mp_model::Clip, takes: &[Hit]) -> usize {
        let distance = |h: &Hit| {
            if h.source == clip.source.0 { (h.start - clip.src_in).abs() } else { f64::MAX }
        };
        takes.iter().enumerate().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1))).map_or(0, |(i, _)| i)
    }

    /// Uses an earlier transcript of this file if there is one, preferring larger models.
    fn load_cached_transcript(&mut self, source: SourceId) -> bool {
        let Some(path) = self.project.source(source).map(|s| s.path.clone()) else { return false };
        let cached = MODELS.iter().rev().find_map(|m| transcribe::load_cached(&path, m.id));
        match cached {
            Some(transcript) => {
                self.index.set_transcript(source.0, transcript);
                true
            }
            None => false,
        }
    }

    pub fn request_transcribe(&mut self, source: SourceId, force: bool) {
        let Some(info) = self.project.source(source).cloned() else { return };
        if !force && self.load_cached_transcript(source) {
            return;
        }
        let Some(runtime) = self.runtime.get(&source) else { return };
        let model = MODELS
            .get(self.selected_model)
            .filter(|m| m.is_installed())
            .or_else(|| MODELS.iter().rev().find(|m| m.is_installed()));
        let Some(model) = model.copied() else {
            self.say.pending_transcribe.push(source);
            self.say.tab = SayTab::Say;
            self.show_say = true;
            return;
        };
        let request = TranscribeRequest {
            mono: runtime.mono.clone(),
            sample_rate: self.sample_rate as f32,
            model,
            language: model.id.ends_with(".en").then(|| "en".to_string()),
        };
        self.jobs.transcribe(source, &info.name, info.path, request);
    }

    // ----- Files -----

    pub fn import_path(&mut self, path: PathBuf) {
        if !mp_analysis::decode::is_supported(&path) {
            self.toast(ToastKind::Bad, format!("Can't read {}", path.display()));
            return;
        }
        self.jobs.import(path, self.sample_rate, None);
    }

    pub fn import_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", mp_analysis::decode::SUPPORTED_EXTENSIONS);
        for path in dialog.pick_files().unwrap_or_default() {
            self.import_path(path);
        }
    }

    /// Runs `next` now if nothing would be lost, and otherwise asks first.
    pub fn after_discard(&mut self, next: AfterDiscard) {
        if self.is_dirty() {
            self.pending_discard = Some(next);
        } else {
            self.perform(next);
        }
    }

    pub fn perform(&mut self, next: AfterDiscard) {
        match next {
            AfterDiscard::New => self.new_project(),
            AfterDiscard::Open => self.open_dialog(),
            AfterDiscard::OpenPath(path) => self.open_project_at(&path),
            AfterDiscard::Quit => {
                self.allow_close = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn new_project(&mut self) {
        self.engine.stop();
        // Ids restart with each project, so nothing keyed by them may carry over: a running device
        // or a clip render with a matching id would otherwise be used for something else entirely.
        self.compiler = GraphCompiler::default();
        self.cache = Arc::new(RenderCache::new());
        self.jobs.switch_project();
        self.clipboard.clear();
        self.word_drag = None;
        self.selected_patch = None;
        self.trial = None;
        self.say.rewrite = None;
        self.say.word_selection = None;
        self.editor.selected_sound = None;
        self.word_dialog = None;
        self.video.clear();
        self.video_frame = None;
        self.audition_source = None;
        self.gesture = None;
        self.clip_drag = None;
        self.marquee = None;
        self.hit_drag = None;
        self.snap_line = None;
        self.flash.clear();
        self.takes_memo = None;
        self.tempo_toast = None;
        self.pitch_memo = None;
        self.say.plan.clear();
        self.say.dirty = true;
        self.editor.range = None;
        self.editor.last_track = None;
        self.editor.needs_fit = true;
        self.project = Project::default();
        self.saved_state = self.project.clone();
        self.history = History::default();
        self.project_path = None;
        self.bank = SourceBank::default();
        self.runtime.clear();
        self.index = WordIndex::default();
        self.spectrograms.clear();
        self.selection.clear();
        self.selected_track = None;
        self.tempo_priority = 0;
        self.tempo_user_set = false;
        self.needs_compile = true;
        self.seek(0.0);
    }

    pub fn open_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Mouthpiece project", &[FILE_EXTENSION]);
        if let Some(path) = dialog.pick_file() {
            self.open_project_at(&path);
        }
    }

    pub fn open_project_at(&mut self, path: &Path) {
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| Project::from_json(&s).map_err(|e| e.to_string()));
        match loaded {
            Ok(mut project) => {
                self.new_project();
                // If a file moved, look for it next to the project before giving up on it.
                let dir = path.parent().map(Path::to_path_buf);
                for source in &mut project.sources {
                    if !source.path.exists() {
                        if let (Some(dir), Some(name)) = (&dir, source.path.file_name()) {
                            let beside = dir.join(name);
                            if beside.exists() {
                                source.path = beside;
                            }
                        }
                    }
                }
                let missing = project.sources.iter().filter(|s| !s.path.exists()).count();
                if missing > 0 {
                    self.toast(ToastKind::Bad, format!("{missing} source file(s) can't be found"));
                }
                for source in &project.sources {
                    if source.path.exists() {
                        self.jobs.import(source.path.clone(), self.sample_rate, Some(source.id));
                    }
                }
                self.project = project;
                self.saved_state = self.project.clone();
                self.project_path = Some(path.to_path_buf());
                self.tempo_user_set = true;
                self.needs_compile = true;
            }
            Err(e) => self.toast(ToastKind::Bad, format!("Can't open project: {e}")),
        }
    }

    pub fn save(&mut self) {
        match self.project_path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Mouthpiece project", &[FILE_EXTENSION])
            .set_file_name(format!("{}.{FILE_EXTENSION}", self.project.name));
        if let Some(path) = dialog.save_file() {
            self.save_to(&path);
        }
    }

    pub fn save_to(&mut self, path: &Path) {
        self.project.name = file_stem(path);
        match write_project(&self.project, path) {
            Ok(()) => {
                self.saved_state = self.project.clone();
                self.project_path = Some(path.to_path_buf());
                // The saved file is now the newest copy, so an older autosave would only mislead.
                let _ = std::fs::remove_file(self.autosave_path());
                self.toast(ToastKind::Good, "Saved");
            }
            Err(e) => self.toast(ToastKind::Bad, format!("Save failed: {e}")),
        }
    }

    fn autosave_path(&self) -> PathBuf {
        match &self.project_path {
            Some(p) => p.with_extension(format!("{FILE_EXTENSION}.autosave")),
            None => transcribe::app_data_dir().join(format!("untitled.{FILE_EXTENSION}.autosave")),
        }
    }

    pub fn has_autosave(&self) -> bool {
        self.autosave_path().exists()
    }

    /// Opens the autosave of this project. It stays unsaved, so it can't overwrite anything silently.
    pub fn recover_autosave(&mut self) {
        let (path, saved_as) = (self.autosave_path(), self.project_path.clone());
        self.open_project_at(&path);
        if self.project_path.is_some() {
            self.project_path = saved_as;
            self.saved_state = Project::default();
            self.toast(ToastKind::Good, "Recovered from autosave. Save to keep it.");
        }
    }

    fn autosave(&mut self) {
        if self.last_autosave.elapsed().as_secs() < AUTOSAVE_SECONDS || !self.is_dirty() {
            return;
        }
        self.last_autosave = Instant::now();
        let path = self.autosave_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // A failed autosave must not interrupt the session, and the next attempt may succeed.
        let _ = write_project(&self.project, &path);
    }

    // ----- Frame housekeeping -----

    pub fn frame_start(&mut self) {
        self.process_jobs();
        if !self.selection.is_empty() {
            self.track_focus = false;
        }
        if self.jobs.active.is_empty() {
            if let Some(text) = self.tempo_toast.take() {
                self.toast(ToastKind::Good, text);
            }
        }
        self.sync_engine();
        self.autosave();
        // Continuous edits finish when the pointer lifts and no text is being edited. Ending them
        // here covers every widget, including a slider that was clicked but never dragged.
        if self.gesture.is_some() && !self.ctx.input(|i| i.pointer.any_down()) && !self.ctx.egui_wants_keyboard_input() {
            self.end_gesture();
        }
        self.toasts.retain(|t| t.born.elapsed().as_secs_f32() < crate::ds::TOAST_SECONDS);
        self.flash.retain(|_, born| born.elapsed().as_secs_f32() < crate::ds::MOTION_FLASH);
        if self.engine.is_playing() {
            self.playhead = self.engine.position() as f64 / self.sample_rate as f64;
        }
    }

    /// Sends a fresh graph to the audio thread if anything changed.
    fn sync_engine(&mut self) {
        let revision = self.cache.revision();
        if (self.needs_compile || revision != self.last_cache_revision) && self.engine.can_send() {
            self.last_cache_revision = revision;
            self.needs_compile = false;
            let graph = self.compiler.compile(&self.project, &self.bank, &self.cache, self.sample_rate);
            self.engine.set_graph(graph);
            if self.cache.memory_bytes() > RENDER_CACHE_LIMIT {
                self.prune_render_cache();
            }
        }
        self.engine.collect_garbage();
    }

    fn prune_render_cache(&mut self) {
        use mp_engine::render_cache::RenderKey;
        let sr = self.sample_rate as f64;
        let keep = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| RenderKey::of(c, (c.src_in * sr).round() as usize, (c.src_out * sr).round() as usize))
            .collect();
        self.cache.retain(&keep);
    }

    fn process_jobs(&mut self) {
        while let Some(event) = self.jobs.next_event() {
            match event {
                JobEvent::Progress { job, fraction } => self.jobs.set_progress(job, fraction),
                JobEvent::Done { job } => self.jobs.finish(job),
                JobEvent::Imported(imported) => self.on_imported(*imported),
                JobEvent::ImportFailed { path, error } => {
                    self.toast(ToastKind::Bad, format!("{}: {error}", file_stem(&path)));
                }
                JobEvent::Transcribed { source, transcript } => {
                    let n = transcript.words.len();
                    self.index.set_transcript(source.0, transcript);
                    self.say.dirty = true;
                    self.takes_memo = None;
                    self.toast(ToastKind::Good, format!("Found {n} word{}", if n == 1 { "" } else { "s" }));
                }
                JobEvent::TranscribeFailed { error } => {
                    if error != "cancelled" {
                        self.toast(ToastKind::Bad, error);
                    }
                }
                JobEvent::Spectrogram { source, spectrogram } => self.on_spectrogram(source, spectrogram),
                JobEvent::VideoReady { source, proxy } => {
                    self.video.insert(source, proxy);
                    self.video_frame = None;
                }
                JobEvent::NoVideo { source } => {
                    if let Some(s) = self.project.sources.iter_mut().find(|s| s.id == source) {
                        s.has_video = false;
                    }
                    self.saved_state.sources.iter_mut().filter(|s| s.id == source).for_each(|s| s.has_video = false);
                }
                JobEvent::ModelReady(model) => {
                    self.toast(ToastKind::Good, format!("{} model ready", model.label));
                    self.selected_model = MODELS.iter().position(|m| m.id == model.id).unwrap_or(0);
                    for source in std::mem::take(&mut self.say.pending_transcribe) {
                        self.request_transcribe(source, false);
                    }
                }
                JobEvent::DownloadFailed { error } => {
                    if error != "cancelled" {
                        self.toast(ToastKind::Bad, error);
                    }
                }
                JobEvent::Exported { path, seconds } => {
                    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
                    let text = format!("Rendered {} ({:.0} s, {:.1} MB)", file_stem(&path), seconds, size as f64 / 1e6);
                    self.toast_with(ToastKind::Good, text, ToastAction::Reveal(path));
                }
                JobEvent::ExportFailed { error } => self.toast(ToastKind::Bad, format!("Render failed: {error}")),
            }
        }
    }

    fn on_imported(&mut self, imported: crate::jobs::Imported) {
        let crate::jobs::Imported { path, audio, mono, peaks, tempo, key, existing } = imported;
        let duration = audio.frames() as f64 / self.sample_rate as f64;
        let name = file_stem(&path);
        let is_video = mp_analysis::video::is_video_path(&path);
        // Unless its name says otherwise, a video is people talking, so it should get transcribed.
        let kind = match TrackKind::from_filename(&name) {
            TrackKind::Music if is_video => TrackKind::Vocal,
            kind => kind,
        };
        let was_empty = self.project.tracks.is_empty();

        let source = match existing {
            Some(id) => id,
            None => self.edit(&format!("Import {name}"), |p| {
                let source = p.add_source(path.clone(), name.clone(), duration, kind);
                if let Some(s) = p.sources.iter_mut().find(|s| s.id == source) {
                    s.has_video = is_video;
                }
                let track = p.add_track(name.clone(), kind);
                p.add_clip(track, source, 0.0, 0.0, duration);
                source
            }),
        };
        if is_video && mp_model::video::ffmpeg_path().is_some() {
            self.jobs.video_proxy(source, path.clone());
        }
        self.bank.buffers.insert(source, audio);
        self.runtime.insert(source, SourceRuntime { peaks, mono });
        self.needs_compile = true;

        if let Some(tempo) = tempo {
            if let Some(s) = self.project.sources.iter_mut().find(|s| s.id == source) {
                s.bpm = Some(tempo.bpm);
            }
            self.saved_state.sources.iter_mut().filter(|s| s.id == source).for_each(|s| s.bpm = Some(tempo.bpm));
            // Drums say the most about tempo, and vocals the least.
            let priority = match kind {
                TrackKind::Drums => 4,
                TrackKind::Bass => 3,
                TrackKind::Music => 2,
                TrackKind::Vocal => 1,
            };
            if !self.tempo_user_set && priority > self.tempo_priority {
                self.tempo_priority = priority;
                self.project.bpm = tempo.bpm;
                self.project.grid_offset = tempo.offset;
                self.tempo_toast = Some(format!("Detected {:.1} BPM from {name}", tempo.bpm));
            }
        }
        if let Some((root, minor)) = key {
            if existing.is_none() && (self.project.key.is_none() || kind == TrackKind::Music) && kind != TrackKind::Vocal && kind != TrackKind::Drums {
                self.project.key = Some(MusicalKey { root, minor });
            }
        }
        if was_empty && existing.is_none() {
            self.arr_view.fit(0.0, duration);
            if let Some(track) = self.project.tracks.first() {
                self.selected_track = Some(track.id);
            }
        }
        // Vocals transcribe themselves. Any other source only picks up a transcript made earlier.
        if kind == TrackKind::Vocal {
            self.request_transcribe(source, false);
        } else if existing.is_some() {
            self.load_cached_transcript(source);
        }
        self.say.dirty = true;
    }

    /// Pitch of the audio a clip plays, in Hz, remembered for the most recent clip.
    pub fn clip_pitch(&mut self, source: SourceId, src_in: f64, src_out: f64) -> Option<f32> {
        let key = (source, src_in.to_bits(), src_out.to_bits());
        if let Some((memo_key, pitch)) = self.pitch_memo {
            if memo_key == key {
                return pitch;
            }
        }
        let sr = self.sample_rate as f32;
        let mono = &self.runtime.get(&source)?.mono;
        let a = ((src_in * sr as f64) as usize).min(mono.len());
        let b = ((src_out * sr as f64) as usize).min(mono.len());
        let pitch = mp_dsp::pitch::median_pitch(&mono[a..b.max(a)], sr);
        self.pitch_memo = Some((key, pitch));
        pitch
    }

    /// Sets the window title. Only when it changes, as every viewport command forces a redraw.
    pub fn set_title(&mut self, title: String) {
        if title != self.last_title {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    pub fn ensure_spectrogram(&mut self, source: SourceId) {
        if self.spectrograms.contains_key(&source) {
            return;
        }
        if let Some(runtime) = self.runtime.get(&source) {
            self.spectrograms.insert(source, SpectrogramState::Pending);
            self.jobs.spectrogram(source, runtime.mono.clone(), self.sample_rate as f32);
        }
    }

    fn on_spectrogram(&mut self, source: SourceId, spectrogram: Arc<Spectrogram>) {
        const TILE: usize = 2048;
        let mut tiles = Vec::new();
        let rows = spectrogram.rows;
        for (i, start) in (0..spectrogram.columns).step_by(TILE).enumerate() {
            let w = TILE.min(spectrogram.columns - start);
            let mut pixels = Vec::with_capacity(w * rows);
            for row in 0..rows {
                let line = &spectrogram.data[row * spectrogram.columns + start..row * spectrogram.columns + start + w];
                pixels.extend(line.iter().map(|v| crate::ds::spectrogram_color(*v as f32 / 255.0)));
            }
            let image = egui::ColorImage { size: [w, rows], source_size: egui::vec2(w as f32, rows as f32), pixels };
            tiles.push(self.ctx.load_texture(format!("spec-{}-{i}", source.0), image, egui::TextureOptions::LINEAR));
        }
        self.spectrograms.insert(source, SpectrogramState::Ready { spectrogram, tiles });
    }
}

fn write_project(project: &Project, path: &Path) -> Result<(), String> {
    let json = project.to_json().map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp-save");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
