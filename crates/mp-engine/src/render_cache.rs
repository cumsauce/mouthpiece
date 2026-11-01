//! Stretched and repitched clip renders, computed on background threads.

use crossbeam_channel::{unbounded, Receiver, Sender};
use mp_dsp::stretch::{self, Algorithm};
use mp_dsp::Stereo;
use mp_model::{Clip, ClipMode, SourceId};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub source: SourceId,
    pub in_frame: usize,
    pub out_frame: usize,
    ratio_bits: u64,
    pitch_bits: u32,
    mode: ClipMode,
    formants: bool,
}

impl RenderKey {
    pub fn of(c: &Clip, in_frame: usize, out_frame: usize) -> Self {
        Self {
            source: c.source,
            in_frame,
            out_frame,
            ratio_bits: c.stretch.to_bits(),
            pitch_bits: c.pitch.to_bits(),
            mode: c.mode,
            formants: c.preserve_formants,
        }
    }

    fn algorithm(&self) -> Algorithm {
        match self.mode {
            ClipMode::Varispeed => Algorithm::Varispeed,
            ClipMode::Elastic => Algorithm::Elastic {
                semitones: f32::from_bits(self.pitch_bits),
                preserve_formants: self.formants,
            },
        }
    }

    fn render(&self, source: &Stereo, sr: u32) -> Stereo {
        let ratio = f64::from_bits(self.ratio_bits);
        stretch::render(source, self.in_frame..self.out_frame, ratio, self.algorithm(), sr)
    }
}

struct Job {
    key: RenderKey,
    source: Arc<Stereo>,
    sr: u32,
}

#[derive(Default)]
struct Inner {
    ready: Mutex<HashMap<RenderKey, Arc<Stereo>>>,
    pending: Mutex<HashSet<RenderKey>>,
    /// Bumped when a render lands, so the UI knows to recompile the graph.
    revision: AtomicU64,
    queued: AtomicUsize,
}

pub struct RenderCache {
    inner: Arc<Inner>,
    jobs: Sender<Job>,
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderCache {
    pub fn new() -> Self {
        let (jobs, rx) = unbounded();
        let inner = Arc::new(Inner::default());
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(1, 4));
        for i in 0..workers {
            let inner = inner.clone();
            let rx: Receiver<Job> = rx.clone();
            let _ = std::thread::Builder::new().name(format!("clip-render-{i}")).spawn(move || {
                for job in rx.iter() {
                    let rendered = Arc::new(job.key.render(&job.source, job.sr));
                    inner.ready.lock().insert(job.key.clone(), rendered);
                    inner.pending.lock().remove(&job.key);
                    inner.queued.fetch_sub(1, Ordering::Relaxed);
                    inner.revision.fetch_add(1, Ordering::Release);
                }
            });
        }
        Self { inner, jobs }
    }

    /// Returns the render if it's ready. Otherwise queues it and returns `None`.
    pub fn get_or_request(&self, key: &RenderKey, source: &Arc<Stereo>, sr: u32) -> Option<Arc<Stereo>> {
        if let Some(r) = self.inner.ready.lock().get(key) {
            return Some(r.clone());
        }
        if self.inner.pending.lock().insert(key.clone()) {
            self.inner.queued.fetch_add(1, Ordering::Relaxed);
            let _ = self.jobs.send(Job { key: key.clone(), source: source.clone(), sr });
        }
        None
    }

    pub fn revision(&self) -> u64 {
        self.inner.revision.load(Ordering::Acquire)
    }

    /// Number of renders still in progress.
    pub fn pending(&self) -> usize {
        self.inner.queued.load(Ordering::Relaxed)
    }

    pub fn is_ready(&self, key: &RenderKey) -> bool {
        self.inner.ready.lock().contains_key(key)
    }

    /// Blocks until all queued renders are done. Used by export.
    pub fn wait_idle(&self) {
        while self.pending() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    pub fn memory_bytes(&self) -> usize {
        self.inner.ready.lock().values().map(|s| s.memory_bytes()).sum()
    }

    /// Drops every render not in `keep`. Called when the cache grows large.
    pub fn retain(&self, keep: &HashSet<RenderKey>) {
        self.inner.ready.lock().retain(|k, _| keep.contains(k));
    }
}
