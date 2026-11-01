//! Speech to words with whisper.cpp, then boundaries and phonemes refined against the audio.

use crate::{align, g2p, normalize_word, Transcript, Word};
use mp_dsp::{pitch, resample::resample};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A downloadable Whisper model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhisperModel {
    pub id: &'static str,
    pub label: &'static str,
    pub size_mb: u32,
    pub note: &'static str,
}

pub const MODELS: &[WhisperModel] = &[
    WhisperModel { id: "tiny.en", label: "Tiny", size_mb: 75, note: "Fastest, for clear speech" },
    WhisperModel { id: "base.en", label: "Base", size_mb: 142, note: "Good for speech" },
    WhisperModel { id: "small.en", label: "Small", size_mb: 466, note: "Good balance" },
    WhisperModel { id: "large-v3-turbo-q5_0", label: "Large Turbo", size_mb: 574, note: "Best for singing, any language" },
];

impl WhisperModel {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin", self.id)
    }

    pub fn path(&self) -> PathBuf {
        models_dir().join(format!("ggml-{}.bin", self.id))
    }

    pub fn is_installed(&self) -> bool {
        self.path().exists()
    }
}

pub fn app_data_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
        return base.join("Mouthpiece");
    }
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/Mouthpiece");
    }
    let data = std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
    data.join("mouthpiece")
}

pub fn models_dir() -> PathBuf {
    app_data_dir().join("models")
}

/// Downloads a model to its install path. The file appears only once fully downloaded.
pub fn download_model(model: &WhisperModel, on_progress: impl Fn(f32), cancel: &AtomicBool) -> Result<(), String> {
    let path = model.path();
    std::fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;
    let response = ureq::get(&model.url()).call().map_err(|e| format!("download failed: {e}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let tmp = path.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut reader = response.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut written = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err("cancelled".to_string());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        written += n as u64;
        if let Some(t) = total {
            on_progress(written as f32 / t as f32);
        }
    }
    drop(file);
    if total.is_some_and(|t| t != written) {
        let _ = std::fs::remove_file(&tmp);
        return Err("download was incomplete".to_string());
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// A word as recognised, before its timing is refined against the audio.
#[derive(Clone, Debug, PartialEq)]
pub struct RawWord {
    pub text: String,
    /// A time near the middle of the word.
    pub anchor: f64,
    pub confidence: f32,
}

/// A run of recognised words, usually one phrase.
#[derive(Clone, Debug, PartialEq)]
pub struct RawSegment {
    pub start: f64,
    pub end: f64,
    pub words: Vec<RawWord>,
}

/// Turns recognised phrases into the final transcript: words and phonemes aligned to the audio and
/// pitch measured. It's separate from Whisper so that it can be tested without a model.
pub fn finish_transcript(segments: Vec<RawSegment>, mono: &[f32], sample_rate: f32, model: &str) -> Transcript {
    let mut segments: Vec<RawSegment> = segments
        .into_iter()
        .map(|mut s| {
            s.words.retain(|w| !normalize_word(&w.text).is_empty());
            s
        })
        .filter(|s| !s.words.is_empty())
        .collect();
    segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    let total = mono.len() as f64 / sample_rate as f64;

    // Split the audio between phrases at the quietest moment near each recognised boundary.
    const SPLIT_REACH: f64 = 0.6;
    let mut splits = vec![0.0];
    for pair in segments.windows(2) {
        let previous = *splits.last().unwrap_or(&0.0);
        let boundary = (pair[0].end + pair[1].start) * 0.5;
        splits.push(align::quietest_point(mono, sample_rate, boundary, SPLIT_REACH, previous));
    }
    splits.push(total);

    let mut aligned: Vec<(RawWord, align::AlignedWord)> = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        let (from, to) = (splits[i], splits[i + 1]);
        let to_frame = |t: f64| ((t * sample_rate as f64) as usize).min(mono.len());
        let audio = &mono[to_frame(from)..to_frame(to).max(to_frame(from))];
        let phrase: Vec<align::PhraseWord> = segment
            .words
            .iter()
            .map(|w| {
                let symbols = g2p::phonemes(&normalize_word(&w.text));
                // Numbers and the like have no phonemes, but still take up time.
                let symbols = if symbols.is_empty() { vec!["ah".to_string()] } else { symbols };
                align::PhraseWord { symbols, anchor: w.anchor }
            })
            .collect();
        let words = align::align_phrase(audio, sample_rate, from, &phrase);
        aligned.extend(segment.words.iter().cloned().zip(words));
    }

    let mut spans: Vec<(f64, f64)> = aligned.iter().map(|(_, a)| (a.start, a.end)).collect();
    align::refine_boundaries(mono, sample_rate, &mut spans);
    let words = aligned
        .into_iter()
        .zip(spans)
        .map(|((raw, a), (start, end))| {
            let to_frame = |t: f64| ((t * sample_rate as f64) as usize).min(mono.len());
            let audio = &mono[to_frame(start)..to_frame(end).max(to_frame(start))];
            // Refining moved the word's edges, so its phonemes scale to fit. Only moving the outer
            // two could put an edge past the phoneme's other end when the word's edge moved far.
            let scale = (end - start) / (a.end - a.start).max(1e-9);
            let mut phonemes = a.phonemes;
            for p in &mut phonemes {
                p.start = start + (p.start - a.start) * scale;
                p.end = start + (p.end - a.start) * scale;
            }
            if let Some(first) = phonemes.first_mut() {
                first.start = start;
            }
            if let Some(last) = phonemes.last_mut() {
                last.end = end;
            }
            Word {
                pitch_hz: pitch::median_pitch(audio, sample_rate),
                text: clean_text(&raw.text),
                start,
                end,
                confidence: raw.confidence,
                phonemes,
            }
        })
        .collect();
    Transcript { words, model: model.to_string() }
}

fn clean_text(s: &str) -> String {
    s.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

pub struct TranscribeRequest {
    pub mono: Arc<Vec<f32>>,
    pub sample_rate: f32,
    pub model: WhisperModel,
    pub language: Option<String>,
}

/// Runs recognition and refinement. `on_progress` receives 0..=1. Blocks, so call off the UI thread.
#[cfg(feature = "whisper")]
pub fn transcribe(
    request: &TranscribeRequest,
    on_progress: impl FnMut(f32) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<Transcript, String> {
    let segments = recognize(&request.model, &request.mono, request.sample_rate, request.language.as_deref(), on_progress, cancel)?;
    Ok(finish_transcript(segments, &request.mono, request.sample_rate, request.model.id))
}

/// Set once the GPU has failed, so later runs skip straight to the CPU. Some GPUs load Whisper's
/// Metal shaders but can't run them, as with the paravirtual GPU in a macOS virtual machine.
#[cfg(feature = "whisper")]
static GPU_UNUSABLE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "whisper")]
fn recognize(
    model: &WhisperModel,
    mono: &[f32],
    sample_rate: f32,
    language: Option<&str>,
    on_progress: impl FnMut(f32) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<RawSegment>, String> {
    const WHISPER_RATE: f64 = 16000.0;
    whisper_rs::install_logging_hooks();
    let audio = resample(mono, WHISPER_RATE / sample_rate as f64);
    let progress = Arc::new(std::sync::Mutex::new(on_progress));
    let attempts: &[bool] = if GPU_UNUSABLE.load(Ordering::Relaxed) { &[false] } else { &[true, false] };
    let mut last_error = String::new();
    for &use_gpu in attempts {
        let progress = progress.clone();
        let report = move |fraction: f32| {
            if let Ok(mut f) = progress.lock() {
                f(fraction);
            }
        };
        match run_whisper(model, &audio, language, use_gpu, report, cancel.clone()) {
            Ok(words) => return Ok(words),
            Err(_) if cancel.load(Ordering::Relaxed) => return Err("cancelled".to_string()),
            Err(error) => {
                if use_gpu {
                    GPU_UNUSABLE.store(true, Ordering::Relaxed);
                }
                last_error = error;
            }
        }
    }
    Err(format!("transcription failed: {last_error}"))
}

#[cfg(feature = "whisper")]
fn dtw_preset(model: &WhisperModel) -> whisper_rs::DtwModelPreset {
    use whisper_rs::DtwModelPreset::*;
    match model.id {
        "tiny.en" => TinyEn,
        "base.en" => BaseEn,
        "small.en" => SmallEn,
        _ => LargeV3Turbo,
    }
}

#[cfg(feature = "whisper")]
fn run_whisper(
    model: &WhisperModel,
    audio: &[f32],
    language: Option<&str>,
    use_gpu: bool,
    mut on_progress: impl FnMut(f32) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<RawSegment>, String> {
    use whisper_rs::{DtwMode, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    let path = model.path();
    let path = path.to_str().ok_or("model path isn't valid UTF-8")?;
    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(use_gpu);
    // DTW over the decoder's attention gives far better word timing than default token timestamps.
    context_params.dtw_parameters(DtwParameters {
        mode: DtwMode::ModelPreset { model_preset: dtw_preset(model) },
        dtw_mem_size: 1 << 27,
    });
    let context = WhisperContext::new_with_params(path, context_params).map_err(|e| format!("could not load the model: {e}"))?;
    let mut state = context.create_state().map_err(|e| format!("could not start the model: {e}"))?;

    let mut params = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    params.set_n_threads(threads as i32);
    params.set_language(language);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(true);
    // whisper-rs 0.16's safe callback setters box the closure as `dyn FnMut`, but their trampoline
    // casts that pointer back to `F`. The two only agree when `F` is itself a boxed `dyn FnMut`.
    // With a plain closure the abort callback reads garbage and whisper fails at once with -6.
    let progress: Box<dyn FnMut(i32)> = Box::new(move |p| on_progress(p as f32 / 100.0));
    params.set_progress_callback_safe(progress);
    let abort: Box<dyn FnMut() -> bool> = Box::new(move || cancel.load(Ordering::Relaxed));
    params.set_abort_callback_safe(abort);

    state.full(params, audio).map_err(|e| e.to_string())?;

    let mut segments = Vec::new();
    for segment in state.as_iter() {
        // Timestamps are in hundredths of a second.
        let mut out = RawSegment {
            start: segment.start_timestamp() as f64 / 100.0,
            end: segment.end_timestamp() as f64 / 100.0,
            words: Vec::new(),
        };
        let mut tokens_in_word = 0;
        for i in 0..segment.n_tokens() {
            let Some(token) = segment.get_token(i) else { continue };
            let text = token.to_str_lossy().map_err(|e| e.to_string())?;
            if text.starts_with("[_") {
                continue;
            }
            let data = token.token_data();
            let time = if data.t_dtw >= 0 { data.t_dtw } else { data.t0 } as f64 / 100.0;
            let probability = token.token_probability();
            // A leading space starts a new word. Other tokens continue the current one.
            let continues = !text.starts_with(' ') && !out.words.is_empty();
            if continues {
                if let Some(word) = out.words.last_mut() {
                    word.text.push_str(&text);
                    word.confidence += probability;
                    tokens_in_word += 1;
                }
            } else {
                if let Some(word) = out.words.last_mut() {
                    word.confidence /= tokens_in_word.max(1) as f32;
                }
                out.words.push(RawWord { text: text.trim().to_string(), anchor: time, confidence: probability });
                tokens_in_word = 1;
            }
        }
        if let Some(word) = out.words.last_mut() {
            word.confidence /= tokens_in_word.max(1) as f32;
        }
        // Whisper describes non-speech like "[Music]" or "(sighs)", which aren't words to find.
        out.words.retain(|w| !w.text.starts_with('[') && !w.text.starts_with('('));
        segments.push(out);
    }
    Ok(segments)
}

/// Bump when recognition or alignment changes, so that old timings aren't reused.
const TRANSCRIPT_VERSION: u32 = 3;

/// Where a transcript for a given audio file is cached, keyed by its path, size and mtime.
pub fn cache_path(audio_path: &Path, model: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(audio_path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    audio_path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok()?.hash(&mut h);
    model.hash(&mut h);
    TRANSCRIPT_VERSION.hash(&mut h);
    Some(app_data_dir().join("transcripts").join(format!("{:016x}.json", h.finish())))
}

pub fn load_cached(audio_path: &Path, model: &str) -> Option<Transcript> {
    let data = std::fs::read(cache_path(audio_path, model)?).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn save_cached(audio_path: &Path, model: &str, transcript: &Transcript) {
    if let Some(path) = cache_path(audio_path, model) {
        if let (Some(dir), Ok(json)) = (path.parent(), serde_json::to_vec(transcript)) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_builds_phonemes_and_drops_empty_words() {
        let sr = 48000.0;
        let mono: Vec<f32> = (0..96000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let word = |text: &str, anchor, confidence| RawWord { text: text.to_string(), anchor, confidence };
        let raw = vec![
            RawSegment { start: 1.0, end: 1.5, words: vec![word("world!", 1.2, 0.9)] },
            RawSegment { start: 0.2, end: 0.9, words: vec![word(" Hello,", 0.4, 0.8), word("...", 0.8, 0.5)] },
        ];
        let t = finish_transcript(raw, &mono, sr, "test");
        assert_eq!(t.words.len(), 2);
        assert_eq!(t.words[0].text, "Hello");
        assert_eq!(t.words[1].text, "world");
        assert_eq!(t.words[0].phonemes[0].symbol, "hh");
        assert!(t.words[0].pitch_hz.is_some());
    }

    #[test]
    fn phonemes_stay_ordered_when_word_edges_are_refined() {
        // A word with a long silent start, the worst case for moving its first edge forward.
        let sr = 48000.0;
        let mut mono = vec![0f32; 48000];
        mono.extend((0..24000).map(|i| (i as f32 * 0.05).sin() * 0.5));
        mono.extend(vec![0f32; 24000]);
        let word = |text: &str, anchor| RawWord { text: text.to_string(), anchor, confidence: 0.9 };
        let raw = vec![RawSegment { start: 0.0, end: 2.0, words: vec![word("people", 1.0), word("up", 1.4)] }];
        let t = finish_transcript(raw, &mono, sr, "test");
        for w in &t.words {
            assert!(w.end > w.start, "{w:?}");
            for p in &w.phonemes {
                assert!(p.end > p.start, "inverted phoneme in {w:?}");
                assert!(p.start >= w.start - 1e-9 && p.end <= w.end + 1e-9, "{w:?}");
            }
        }
    }

    #[test]
    fn model_urls_point_at_the_official_repo() {
        for m in MODELS {
            assert!(m.url().starts_with("https://huggingface.co/ggerganov/whisper.cpp/"));
            assert!(m.path().to_string_lossy().ends_with(".bin"));
        }
    }
}
