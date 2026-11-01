//! Background work. Nothing here runs on the UI or audio thread.

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use mp_analysis::peaks::Peaks;
use mp_analysis::spectrogram::Spectrogram;
use mp_analysis::tempo::TempoResult;
use mp_analysis::transcribe::{self, TranscribeRequest, WhisperModel};
use mp_analysis::video::{self, VideoProxy};
use mp_analysis::{decode, key, tempo, Transcript};
use mp_dsp::Stereo;
use mp_engine::{GraphCompiler, RenderCache, SourceBank};
use mp_export::video::{LevelSource, VideoSettings};
use mp_export::{ExportSettings, Format};
use mp_analysis::online::{self, DownloadKind, SongResult};
use mp_analysis::stems::{self, StemError, StemJob, StemSettings, StemWorker};
use mp_analysis::tools::{self, Tool};
use mp_model::{Project, SourceId};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Imported {
    pub path: PathBuf,
    pub audio: Arc<Stereo>,
    pub mono: Arc<Vec<f32>>,
    pub peaks: Arc<Peaks>,
    pub tempo: Option<TempoResult>,
    pub key: Option<(u8, bool)>,
    /// Set when reloading a source of an opened project, rather than importing a new one.
    pub existing: Option<SourceId>,
}

pub enum JobEvent {
    Progress { job: u64, fraction: f32 },
    Done { job: u64 },
    Imported(Box<Imported>),
    ImportFailed { path: PathBuf, error: String },
    Transcribed { source: SourceId, transcript: Transcript },
    TranscribeFailed { error: String },
    Spectrogram { source: SourceId, spectrogram: Arc<Spectrogram> },
    VideoReady { source: SourceId, proxy: Arc<VideoProxy> },
    NoVideo { source: SourceId },
    ModelReady(WhisperModel),
    DownloadFailed { error: String },
    Exported { path: PathBuf, seconds: f64 },
    ExportFailed { error: String },
    Label { job: u64, label: String },
    LyricsResults { results: Result<Vec<mp_analysis::lyrics::LyricsMatch>, String> },
    LyricsApplied { source: SourceId, transcript: Transcript, report: mp_analysis::lyrics::LyricsReport },
    LyricsFailed { error: String },
    SongResults { job: u64, results: Result<Vec<mp_analysis::online::SongResult>, String> },
    SongDownloaded { path: PathBuf, split: bool },
    SongFailed { error: String },
    ToolsFailed { error: String },
    FfmpegReady,
    MediaCollected { package: PathBuf, moves: Vec<(PathBuf, PathBuf)>, error: Option<String> },
    StemsInstalled,
    StemsReady { source: SourceId, files: BTreeMap<String, PathBuf> },
    /// `source` is none if it was the install that failed.
    StemsFailed { source: Option<SourceId>, error: StemError },
}

pub struct ActiveJob {
    pub id: u64,
    pub label: String,
    pub fraction: Option<f32>,
    pub cancel: Arc<AtomicBool>,
    /// Renders and model downloads carry on when a different project is opened.
    outlives_project: bool,
}

/// Marks an event as relevant whichever project is open.
const ANY_PROJECT: u64 = u64::MAX;

/// Lets a background thread report back. Ids restart with each project, so each event is stamped
/// with the project it belongs to, and the UI drops those from a project that's since been closed.
#[derive(Clone)]
struct Reporter {
    tx: Sender<(u64, JobEvent)>,
    project: u64,
    ctx: egui::Context,
}

impl Reporter {
    fn send(&self, event: JobEvent) {
        let _ = self.tx.send((self.project, event));
        self.ctx.request_repaint();
    }
}

struct TranscribeJob {
    job: u64,
    source: SourceId,
    request: TranscribeRequest,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    reporter: Reporter,
}

pub struct Jobs {
    tx: Sender<(u64, JobEvent)>,
    rx: Receiver<(u64, JobEvent)>,
    ctx: egui::Context,
    next_id: u64,
    project: u64,
    pub active: Vec<ActiveJob>,
    transcribe_queue: Sender<TranscribeJob>,
    stem_queue: Sender<StemRequest>,
    online_queue: Sender<Box<dyn FnOnce() + Send>>,
}

impl Jobs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded();
        let (transcribe_queue, queue) = unbounded::<TranscribeJob>();
        // One at a time: Whisper already uses every core.
        let _ = std::thread::Builder::new().name("transcribe".into()).spawn(move || {
            for job in queue.iter() {
                run_transcribe(job);
            }
        });
        let (stem_queue, stem_jobs) = unbounded::<StemRequest>();
        let _ = std::thread::Builder::new().name("stems".into()).spawn(move || run_stems(stem_jobs));
        // One at a time, because yt-dlp replaces itself when it updates.
        let (online_queue, online_jobs) = unbounded::<Box<dyn FnOnce() + Send>>();
        let _ = std::thread::Builder::new().name("online".into()).spawn(move || online_jobs.iter().for_each(|task| task()));
        Self { tx, rx, ctx, next_id: 1, project: 0, active: Vec::new(), transcribe_queue, stem_queue, online_queue }
    }

    /// Next event that still applies.
    pub fn next_event(&self) -> Option<JobEvent> {
        while let Ok((project, event)) = self.rx.try_recv() {
            if project == self.project || project == ANY_PROJECT {
                return Some(event);
            }
        }
        None
    }

    /// Call when a different project opens: stops its work and ignores anything still on the way.
    pub fn switch_project(&mut self) {
        self.project += 1;
        self.active.retain(|job| {
            if !job.outlives_project {
                job.cancel.store(true, Ordering::Relaxed);
            }
            job.outlives_project
        });
    }

    fn reporter(&self, project: u64) -> Reporter {
        Reporter { tx: self.tx.clone(), project, ctx: self.ctx.clone() }
    }

    fn register(&mut self, label: String, outlives_project: bool) -> (u64, Arc<AtomicBool>) {
        let id = self.next_id;
        self.next_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.push(ActiveJob { id, label, fraction: None, cancel: cancel.clone(), outlives_project });
        (id, cancel)
    }

    /// Sets up the separation software, in the background.
    pub fn install_stems(&mut self) {
        let (job, cancel) = self.register("Downloading stem splitter".to_string(), true);
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || {
            let result = stems::install(&cancel, &mut |label, fraction| {
                reporter.send(JobEvent::Label { job, label: format!("Downloading: {label}") });
                reporter.send(JobEvent::Progress { job, fraction });
            });
            reporter.send(match result {
                Ok(()) => JobEvent::StemsInstalled,
                Err(error) => JobEvent::StemsFailed { source: None, error },
            });
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn update_yt_dlp(&mut self) {
        let (job, cancel) = self.register("Updating yt-dlp".to_string(), true);
        let reporter = self.reporter(ANY_PROJECT);
        let _ = self.online_queue.send(Box::new(move || {
            let result = tools::ensure(&[Tool::YtDlp], &cancel, &mut |_, _| {}).and_then(|()| online::update(true, &cancel));
            if let Err(error) = result {
                reporter.send(JobEvent::SongFailed { error });
            }
            reporter.send(JobEvent::Done { job });
        }));
    }

    pub fn search_lyrics(&mut self, query: String, length: f64) {
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || reporter.send(JobEvent::LyricsResults { results: mp_analysis::lyrics::search(&query, length) }));
    }

    /// Fits lyrics to the audio, which takes a while for a whole song.
    pub fn apply_lyrics(&mut self, source: SourceId, path: PathBuf, transcript: Transcript, text: String, mono: Arc<Vec<f32>>, sample_rate: f32) {
        let (job, _) = self.register("Applying lyrics".to_string(), false);
        let reporter = self.reporter(self.project);
        std::thread::spawn(move || {
            let lines = mp_analysis::lyrics::parse(&text);
            reporter.send(match mp_analysis::lyrics::apply(&transcript, &lines, &mono, sample_rate) {
                Ok((corrected, report)) => {
                    transcribe::save_cached(&path, &corrected.model.clone(), &corrected);
                    JobEvent::LyricsApplied { source, transcript: corrected, report }
                }
                Err(error) => JobEvent::LyricsFailed { error },
            });
            reporter.send(JobEvent::Done { job });
        });
    }

    /// Fetches ffmpeg on its own, for showing and rendering video.
    pub fn collect_media(&mut self, paths: Vec<PathBuf>, package: PathBuf) {
        let (job, _) = self.register("Copying samples into the project".to_string(), false);
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || {
            let (moves, error) = mp_model::file::collect_media(&paths, &package, &mut |fraction| reporter.send(JobEvent::Progress { job, fraction }));
            reporter.send(JobEvent::MediaCollected { package, moves, error });
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn get_ffmpeg(&mut self) {
        let (job, cancel) = self.register("Downloading ffmpeg".to_string(), true);
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || {
            let result = tools::ensure(&[Tool::Ffmpeg], &cancel, &mut |_, fraction| reporter.send(JobEvent::Progress { job, fraction }));
            reporter.send(match result {
                Ok(()) => JobEvent::FfmpegReady,
                Err(error) => JobEvent::ToolsFailed { error },
            });
            reporter.send(JobEvent::Done { job });
        });
    }

    /// Starts a search, and returns its job, which reports what's happening in its label.
    pub fn search_songs(&mut self, query: String) -> u64 {
        let (job, cancel) = self.register("Searching".to_string(), true);
        let reporter = self.reporter(ANY_PROJECT);
        let _ = self.online_queue.send(Box::new(move || {
            let prepared = online::prepare(&cancel, &mut |label, fraction| {
                reporter.send(JobEvent::Label { job, label: label.to_string() });
                reporter.send(JobEvent::Progress { job, fraction });
            });
            reporter.send(JobEvent::Label { job, label: "Searching".to_string() });
            let results = prepared.and_then(|()| online::search(&query, &cancel));
            reporter.send(JobEvent::SongResults { job, results });
            reporter.send(JobEvent::Done { job });
        }));
        job
    }

    pub fn download_song(&mut self, song: SongResult, kind: DownloadKind, dir: PathBuf, split: bool) {
        let (job, cancel) = self.register(format!("Downloading {}", song.title), true);
        let reporter = self.reporter(self.project);
        let _ = self.online_queue.send(Box::new(move || {
            let result = online::prepare(&cancel, &mut |_, _| {}).and_then(|()| online::download(&song, kind, &dir, &cancel, &mut |fraction| reporter.send(JobEvent::Progress { job, fraction })));
            reporter.send(match result {
                Ok(path) => JobEvent::SongDownloaded { path, split },
                Err(error) => JobEvent::SongFailed { error },
            });
            reporter.send(JobEvent::Done { job });
        }));
    }

    pub fn split_stems(&mut self, source: SourceId, name: &str, input: PathBuf, settings: StemSettings, low_memory: bool) {
        let (job, cancel) = self.register(format!("Splitting {name}"), false);
        let reporter = self.reporter(self.project);
        let _ = self.stem_queue.send(StemRequest { job, source, name: name.to_string(), cancel, reporter, stem_job: StemJob { input, settings, low_memory, ffmpeg: mp_model::video::find_ffmpeg() } });
    }

    pub fn import(&mut self, path: PathBuf, sample_rate: u32, existing: Option<SourceId>) {
        let name = file_stem(&path);
        let (job, _) = self.register(format!("Analyzing {name}"), false);
        let reporter = self.reporter(self.project);
        std::thread::spawn(move || {
            let event = match decode::decode_file(&path, sample_rate) {
                Ok(audio) => {
                    let mono = audio.mono();
                    let sr = sample_rate as f32;
                    JobEvent::Imported(Box::new(Imported {
                        peaks: Arc::new(Peaks::build(&mono)),
                        tempo: tempo::detect(&mono, sr),
                        key: key::detect(&mono, sr),
                        mono: Arc::new(mono),
                        audio: Arc::new(audio),
                        path,
                        existing,
                    }))
                }
                Err(error) => JobEvent::ImportFailed { path, error },
            };
            reporter.send(event);
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn transcribe(&mut self, source: SourceId, name: &str, path: PathBuf, request: TranscribeRequest) {
        let (job, cancel) = self.register(format!("Transcribing {name}"), false);
        let reporter = self.reporter(self.project);
        let _ = self.transcribe_queue.send(TranscribeJob { job, source, request, path, cancel, reporter });
        self.ctx.request_repaint();
    }

    pub fn spectrogram(&mut self, source: SourceId, mono: Arc<Vec<f32>>, sample_rate: f32) {
        let reporter = self.reporter(self.project);
        std::thread::spawn(move || {
            let spectrogram = Arc::new(mp_analysis::spectrogram::compute(&mono, sample_rate));
            reporter.send(JobEvent::Spectrogram { source, spectrogram });
        });
    }

    pub fn video_proxy(&mut self, source: SourceId, path: PathBuf) {
        let (job, _) = self.register(format!("Preparing video for {}", file_stem(&path)), false);
        let reporter = self.reporter(self.project);
        std::thread::spawn(move || {
            reporter.send(match video::extract_proxy(&path) {
                Ok(proxy) => JobEvent::VideoReady { source, proxy: Arc::new(proxy) },
                Err(_) => JobEvent::NoVideo { source },
            });
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn download_model(&mut self, model: WhisperModel) {
        let (job, cancel) = self.register(format!("Downloading {} model", model.label), true);
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || {
            let progress = reporter.clone();
            let result = transcribe::download_model(
                &model,
                move |fraction| progress.send(JobEvent::Progress { job, fraction }),
                &cancel,
            );
            reporter.send(match result {
                Ok(()) => JobEvent::ModelReady(model),
                Err(error) => JobEvent::DownloadFailed { error },
            });
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn export(
        &mut self,
        project: Project,
        bank: SourceBank,
        cache: Arc<RenderCache>,
        sample_rate: u32,
        settings: ExportSettings,
        peaks: HashMap<SourceId, Arc<Peaks>>,
        path: PathBuf,
    ) {
        let (job, cancel) = self.register(format!("Rendering {}", file_stem(&path)), true);
        let reporter = self.reporter(ANY_PROJECT);
        std::thread::spawn(move || {
            // The first compile queues any clip renders that aren't ready yet. The second, once
            // they've landed, gets the final audio, with fresh devices that have no leftover tails.
            drop(GraphCompiler::default().compile(&project, &bank, &cache, sample_rate));
            cache.wait_idle();
            let graph = GraphCompiler::default().compile(&project, &bank, &cache, sample_rate);
            let seconds = settings.end.saturating_sub(settings.start) as f64 / sample_rate as f64;
            let progress = reporter.clone();
            let mut last = 0.0;
            let report = |p: f32| {
                if p - last >= 0.01 {
                    last = p;
                    progress.send(JobEvent::Progress { job, fraction: p });
                }
                !cancel.load(Ordering::Relaxed)
            };
            let result = match settings.format {
                Format::Mp4 { height } => export_video(graph, &project, sample_rate, &settings, height, peaks, &path, report),
                _ => mp_export::export(graph, sample_rate, &settings, &path, report),
            };
            match result {
                Ok(true) => reporter.send(JobEvent::Exported { path, seconds }),
                Ok(false) => {}
                Err(error) => reporter.send(JobEvent::ExportFailed { error }),
            }
            reporter.send(JobEvent::Done { job });
        });
    }

    pub fn set_label(&mut self, job: u64, label: String) {
        if let Some(j) = self.active.iter_mut().find(|j| j.id == job) {
            j.label = label;
        }
    }

    pub fn set_progress(&mut self, job: u64, fraction: f32) {
        if let Some(j) = self.active.iter_mut().find(|j| j.id == job) {
            j.fraction = Some(fraction);
        }
    }

    pub fn finish(&mut self, job: u64) {
        self.active.retain(|j| j.id != job);
    }
}

/// Renders the mix first, so that the video gets exactly what playback sounds like, then the picture.
fn export_video(
    graph: Box<mp_engine::EngineGraph>,
    project: &Project,
    sample_rate: u32,
    settings: &ExportSettings,
    height: u32,
    peaks: HashMap<SourceId, Arc<Peaks>>,
    path: &std::path::Path,
    mut report: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let audio = std::env::temp_dir().join(format!("mouthpiece-mix-{}.wav", std::process::id()));
    let mix = ExportSettings { format: Format::Wav24, ..settings.clone() };
    let result = (|| {
        if !mp_export::export(graph, sample_rate, &mix, &audio, |p| report(p * 0.2))? {
            return Ok(false);
        }
        const FPS: u32 = 30;
        let (a, b) = (settings.start as f64 / sample_rate as f64, settings.end as f64 / sample_rate as f64);
        let width = height * 16 / 9;
        let video = VideoSettings { width: width + width % 2, height, fps: FPS };
        let levels = LevelSource { peaks, sample_rate: sample_rate as f64 };
        mp_export::video::export_video(project, (a, b), &levels, &audio, &video, path, |p| report(0.2 + p * 0.8))
    })();
    let _ = std::fs::remove_file(&audio);
    result
}

struct StemRequest {
    job: u64,
    source: SourceId,
    name: String,
    cancel: Arc<AtomicBool>,
    reporter: Reporter,
    stem_job: StemJob,
}

/// Runs splits one at a time. The worker holds the models in memory, which saves loading them for the
/// next song, but a DAW needs that memory more than an idle splitter does, so it goes after a while.
fn run_stems(queue: Receiver<StemRequest>) {
    const KEEP_MODELS_FOR: std::time::Duration = std::time::Duration::from_secs(300);
    let mut worker: Option<(StemWorker, StemSettings)> = None;
    loop {
        let request = match queue.recv_timeout(KEEP_MODELS_FOR) {
            Ok(request) => request,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                worker = None;
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        };
        let StemRequest { job, source, name, cancel, reporter, stem_job } = request;
        let settings = stem_job.settings.clone();
        if worker.as_ref().is_some_and(|(_, s)| s.model_dir() != settings.model_dir()) {
            worker = None;
        }
        if worker.as_mut().is_some_and(|(w, _)| !w.is_alive()) {
            worker = None;
        }
        let mut running = None;
        let result = match worker.take() {
            Some(pair) => Ok(pair.0),
            None => StemWorker::start(&settings, &cancel),
        }
        .and_then(|mut w| {
            let label = format!("Splitting {name} on {}", device_label(&w.device));
            let result = w.separate(&stem_job, &cancel, &mut |fraction, stage| {
                reporter.send(JobEvent::Progress { job, fraction });
                reporter.send(JobEvent::Label { job, label: format!("{label}: {stage}") });
            });
            running = Some(w);
            result
        });
        // Cancelling kills the process, and so does running out of memory on some systems.
        // A worker from a broken install goes too, so that the repaired one starts fresh.
        if !matches!(result, Err(StemError::Cancelled | StemError::BrokenInstall(_))) {
            worker = running.map(|w| (w, settings));
        }
        reporter.send(match result {
            Ok(files) => JobEvent::StemsReady { source, files },
            Err(error) => JobEvent::StemsFailed { source: Some(source), error },
        });
        reporter.send(JobEvent::Done { job });
    }
}

fn device_label(device: &str) -> &'static str {
    match device {
        "cuda" => "the GPU",
        "mps" => "the Apple GPU",
        _ => "the CPU",
    }
}

fn run_transcribe(job: TranscribeJob) {
    let TranscribeJob { job, source, request, path, cancel, reporter } = job;
    if !cancel.load(Ordering::Relaxed) {
        let progress = reporter.clone();
        let result = transcribe::transcribe(
            &request,
            move |fraction| progress.send(JobEvent::Progress { job, fraction }),
            cancel,
        );
        reporter.send(match result {
            Ok(transcript) => {
                transcribe::save_cached(&path, request.model.id, &transcript);
                JobEvent::Transcribed { source, transcript }
            }
            Err(error) => JobEvent::TranscribeFailed { error },
        });
    }
    reporter.send(JobEvent::Done { job });
}

pub fn file_stem(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().to_string())
}
