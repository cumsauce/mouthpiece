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
}

struct TranscribeJob {
    job: u64,
    source: SourceId,
    request: TranscribeRequest,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
}

pub struct Jobs {
    tx: Sender<JobEvent>,
    pub rx: Receiver<JobEvent>,
    ctx: egui::Context,
    next_id: u64,
    pub active: Vec<ActiveJob>,
    transcribe_queue: Sender<TranscribeJob>,
}

impl Jobs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded();
        let (transcribe_queue, queue) = unbounded::<TranscribeJob>();
        // One at a time: Whisper already uses every core.
        let worker_tx = tx.clone();
        let worker_ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("transcribe".into()).spawn(move || {
            for job in queue.iter() {
                run_transcribe(job, &worker_tx, &worker_ctx);
            }
        });
        Self { tx, rx, ctx, next_id: 1, active: Vec::new(), transcribe_queue }
    }

    fn register(&mut self, label: String) -> (u64, Arc<AtomicBool>) {
        let id = self.next_id;
        self.next_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.push(ActiveJob { id, label, fraction: None, cancel: cancel.clone() });
        (id, cancel)
    }

    pub fn import(&mut self, path: PathBuf, sample_rate: u32, existing: Option<SourceId>) {
        let name = file_stem(&path);
        let (job, _) = self.register(format!("Analyzing {name}"));
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
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
            let _ = tx.send(event);
            let _ = tx.send(JobEvent::Done { job });
            ctx.request_repaint();
        });
    }

    pub fn transcribe(&mut self, source: SourceId, name: &str, path: PathBuf, request: TranscribeRequest) {
        let (job, cancel) = self.register(format!("Transcribing {name}"));
        let _ = self.transcribe_queue.send(TranscribeJob { job, source, request, path, cancel });
        self.ctx.request_repaint();
    }

    pub fn spectrogram(&mut self, source: SourceId, mono: Arc<Vec<f32>>, sample_rate: f32) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let spectrogram = Arc::new(mp_analysis::spectrogram::compute(&mono, sample_rate));
            let _ = tx.send(JobEvent::Spectrogram { source, spectrogram });
            ctx.request_repaint();
        });
    }

    pub fn download_model(&mut self, model: WhisperModel) {
        let (job, cancel) = self.register(format!("Downloading {} model", model.label));
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let (ptx, pctx) = (tx.clone(), ctx.clone());
            let result = transcribe::download_model(
                &model,
                move |fraction| {
                    let _ = ptx.send(JobEvent::Progress { job, fraction });
                    pctx.request_repaint();
                },
                &cancel,
            );
            let _ = tx.send(match result {
                Ok(()) => JobEvent::ModelReady(model),
                Err(error) => JobEvent::DownloadFailed { error },
            });
            let _ = tx.send(JobEvent::Done { job });
            ctx.request_repaint();
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
        let (job, cancel) = self.register(format!("Rendering {}", file_stem(&path)));
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            // The first compile queues any clip renders that aren't ready yet. The second, once
            // they've landed, gets the final audio, with fresh devices that have no leftover tails.
            drop(GraphCompiler::default().compile(&project, &bank, &cache, sample_rate));
            cache.wait_idle();
            let graph = GraphCompiler::default().compile(&project, &bank, &cache, sample_rate);
            let seconds = settings.end.saturating_sub(settings.start) as f64 / sample_rate as f64;
            let (ptx, pctx) = (tx.clone(), ctx.clone());
            let mut last = 0.0;
            let result = mp_export::export(graph, sample_rate, &settings, &path, |p| {
                if p - last >= 0.01 {
                    last = p;
                    let _ = ptx.send(JobEvent::Progress { job, fraction: p });
                    pctx.request_repaint();
                }
                !cancel.load(Ordering::Relaxed)
            });
            match result {
                Ok(true) => {
                    let _ = tx.send(JobEvent::Exported { path, seconds });
                }
                Ok(false) => {}
                Err(error) => {
                    let _ = tx.send(JobEvent::ExportFailed { error });
                }
            }
            let _ = tx.send(JobEvent::Done { job });
            ctx.request_repaint();
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

fn run_transcribe(job: TranscribeJob, tx: &Sender<JobEvent>, ctx: &egui::Context) {
    let TranscribeJob { job, source, request, path, cancel } = job;
    if !cancel.load(Ordering::Relaxed) {
        let (ptx, pctx) = (tx.clone(), ctx.clone());
        let result = transcribe::transcribe(
            &request,
            move |fraction| {
                let _ = ptx.send(JobEvent::Progress { job, fraction });
                pctx.request_repaint();
            },
            cancel,
        );
        let _ = tx.send(match result {
            Ok(transcript) => {
                transcribe::save_cached(&path, request.model.id, &transcript);
                JobEvent::Transcribed { source, transcript }
            }
            Err(error) => JobEvent::TranscribeFailed { error },
        });
    }
    let _ = tx.send(JobEvent::Done { job });
    ctx.request_repaint();
}

pub fn file_stem(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().to_string())
}
