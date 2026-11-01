//! Background work. Nothing here runs on the UI or audio thread.

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use mp_analysis::peaks::Peaks;
use mp_analysis::spectrogram::Spectrogram;
use mp_analysis::tempo::TempoResult;
use mp_analysis::transcribe::{self, TranscribeRequest, WhisperModel};
use mp_analysis::{decode, key, tempo, Transcript};
use mp_dsp::Stereo;
use mp_engine::{GraphCompiler, RenderCache, SourceBank};
use mp_export::ExportSettings;
use mp_model::{Project, SourceId};
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
    ModelReady(WhisperModel),
    DownloadFailed { error: String },
    Exported { path: PathBuf, seconds: f64 },
    ExportFailed { error: String },
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
        Self { tx, rx, ctx, next_id: 1, project: 0, active: Vec::new(), transcribe_queue }
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
            let result = mp_export::export(graph, sample_rate, &settings, &path, |p| {
                if p - last >= 0.01 {
                    last = p;
                    progress.send(JobEvent::Progress { job, fraction: p });
                }
                !cancel.load(Ordering::Relaxed)
            });
            match result {
                Ok(true) => reporter.send(JobEvent::Exported { path, seconds }),
                Ok(false) => {}
                Err(error) => reporter.send(JobEvent::ExportFailed { error }),
            }
            reporter.send(JobEvent::Done { job });
        });
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
