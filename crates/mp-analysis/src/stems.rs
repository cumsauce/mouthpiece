//! Splits a song into stems with `audio-separator`, a Python package that wraps the UVR models.
//! It has no Rust equivalent, so it runs as a long-lived worker process in a private Python environment,
//! which is created on first use. Models stay loaded between jobs, and the worker goes when it's idle.

use crate::tools::{self, hide_console, Tool};
use crate::transcribe::app_data_dir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, SystemTime};

const WORKER: &str = include_str!("../assets/separate.py");
const PYTHON_FOR_UV: &str = "3.12";
const LOG_LINES_KEPT: usize = 40;
/// audio-separator imports audioread without declaring it, and only got it through librosa, which
/// dropped it in 1.0. Staying below 1.0 is also what audio-separator has been built against.
const PINNED: [&str; 2] = ["librosa<1", "audioread"];
const INSTALL_CHECK: &str = "from audio_separator.separator import Separator; import librosa, numpy; librosa.stft(numpy.zeros(4096))";

/// What's split out. Vocals, drums, bass and other make the tracks. Instrumental is a file for keeping.
pub const STEMS: [&str; 5] = ["Vocals", "Drums", "Bass", "Other", "Instrumental"];
pub const TRACK_STEMS: [&str; 4] = ["Vocals", "Drums", "Bass", "Other"];

/// The vocal models on offer, best first by audio-separator's published vocal scores.
pub const VOCAL_MODELS: [(&str, &str); 3] = [
    ("model_bs_roformer_ep_317_sdr_12.9755.ckpt", "BS-RoFormer 317"),
    ("model_bs_roformer_ep_368_sdr_12.9628.ckpt", "BS-RoFormer 368"),
    ("vocals_mel_band_roformer.ckpt", "Mel-RoFormer"),
];
pub const INSTRUMENT_MODEL: &str = "htdemucs_ft.yaml";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StemFormat {
    Wav,
    Flac,
    Mp3,
}

impl StemFormat {
    pub fn extension(self) -> &'static str {
        match self {
            StemFormat::Wav => "wav",
            StemFormat::Flac => "flac",
            StemFormat::Mp3 => "mp3",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StemSettings {
    pub vocal_model: String,
    pub instrument_model: String,
    pub format: StemFormat,
    /// Where models are kept, since they're large. `None` is inside the app's data folder.
    pub model_dir: Option<PathBuf>,
}

impl Default for StemSettings {
    fn default() -> Self {
        Self { vocal_model: VOCAL_MODELS[0].0.to_string(), instrument_model: INSTRUMENT_MODEL.to_string(), format: StemFormat::Wav, model_dir: None }
    }
}

impl StemSettings {
    pub fn model_dir(&self) -> PathBuf {
        self.model_dir.clone().unwrap_or_else(|| stems_home().join("models"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StemError {
    Cancelled,
    /// The file couldn't be read.
    BadInput(String),
    /// Trying again with `low_memory` may help.
    OutOfMemory(String),
    /// A model couldn't be found or downloaded.
    MissingModel(String),
    /// Part of the separation software is missing. Installing again repairs it.
    BrokenInstall(String),
    Other(String),
}

impl StemError {
    pub fn message(&self) -> String {
        match self {
            StemError::Cancelled => "Cancelled".to_string(),
            StemError::BadInput(_) => "That file couldn't be read. It may be damaged, or in a format that isn't supported.".to_string(),
            StemError::OutOfMemory(_) => "This ran out of memory. Close other apps, or try again using less memory.".to_string(),
            StemError::MissingModel(detail) => format!("The model couldn't be loaded or downloaded. Check your connection and the model's name. {}", last_line(detail)),
            StemError::BrokenInstall(detail) => format!("Part of the separation software is missing, and it can be repaired. {}", last_line(detail)),
            StemError::Other(detail) => format!("Splitting failed. {}", last_line(detail)),
        }
    }
}

fn last_line(text: &str) -> &str {
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")
}

fn stems_home() -> PathBuf {
    app_data_dir().join("stems")
}

fn venv_python() -> PathBuf {
    let venv = stems_home().join("venv");
    if cfg!(windows) { venv.join("Scripts/python.exe") } else { venv.join("bin/python") }
}

fn marker() -> PathBuf {
    stems_home().join("installed")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Setup {
    Ready,
    /// The first split has to download the separation software.
    NeedsInstall,
    /// It's here but incomplete or out of date, which only needs what's missing.
    NeedsRepair,
}

/// Whether splitting can run now.
pub fn setup() -> Setup {
    if std::env::var_os("MOUTHPIECE_STEMS_PYTHON").is_some() {
        return Setup::Ready;
    }
    let record = std::fs::read_to_string(marker()).ok();
    setup_state(venv_python().is_file(), record.as_deref(), &install_record(), tools::locate(Tool::Ffmpeg).is_some())
}

fn setup_state(has_environment: bool, record: Option<&str>, wanted: &str, has_ffmpeg: bool) -> Setup {
    match has_environment {
        false => Setup::NeedsInstall,
        true if record == Some(wanted) && has_ffmpeg => Setup::Ready,
        true => Setup::NeedsRepair,
    }
}

/// Makes the next split repair the install, after finding something missing from it.
pub fn forget_install() {
    let _ = std::fs::remove_file(marker());
}

/// What an install consists of. A change here makes existing installs repair themselves.
fn install_record() -> String {
    std::iter::once(package()).chain(PINNED).collect::<Vec<_>>().join(" ")
}

fn package() -> &'static str {
    // On Apple Silicon the cpu build is the one that uses the GPU, through MPS.
    if !cfg!(target_os = "macos") && tools::which("nvidia-smi").is_some() {
        "audio-separator[gpu]"
    } else {
        "audio-separator[cpu]"
    }
}

/// Runs to the end, or kills it on cancel. Returns what it last said on an error.
fn run_step(mut command: Command, cancel: &AtomicBool) -> Result<(), StemError> {
    hide_console(&mut command);
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| StemError::Other(e.to_string()))?;
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(s) = &mut stderr {
            let _ = s.read_to_string(&mut text);
        }
        text
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(StemError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(StemError::Other(reader.join().unwrap_or_default())),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(StemError::Other(e.to_string())),
        }
    }
}

/// Gets everything splitting needs, including uv and ffmpeg if they aren't here. A first install downloads
/// several gigabytes. A repair keeps what's there and only fetches what's missing.
pub fn install(cancel: &AtomicBool, on_progress: &mut dyn FnMut(&str, f32)) -> Result<(), StemError> {
    let failed = |e: String| if e == "cancelled" { StemError::Cancelled } else { StemError::Other(e) };
    tools::ensure(&[Tool::Ffmpeg, Tool::Uv], cancel, on_progress).map_err(failed)?;
    let uv = tools::locate(Tool::Uv).ok_or_else(|| StemError::Other("uv isn't installed".to_string()))?;
    let venv = stems_home().join("venv");
    std::fs::create_dir_all(stems_home()).map_err(|e| StemError::Other(e.to_string()))?;
    on_progress("Getting the stem splitter", 0.0);
    if !venv_python().is_file() {
        let mut make = Command::new(&uv);
        make.args(["venv", "--clear", "--python", PYTHON_FOR_UV]).arg(&venv);
        run_step(make, cancel)?;
    }
    let mut add = Command::new(&uv);
    add.args(["pip", "install", "--python"]).arg(venv_python()).arg(package()).args(PINNED);
    run_step(add, cancel)?;
    // Only trust an install that has actually loaded. A package going missing upstream shows up here.
    on_progress("Checking the stem splitter", 1.0);
    let mut check = Command::new(venv_python());
    check.args(["-c", INSTALL_CHECK]);
    run_step(check, cancel).map_err(|e| match e {
        StemError::Other(detail) => StemError::BrokenInstall(detail),
        other => other,
    })?;
    std::fs::write(marker(), install_record()).map_err(|e| StemError::Other(e.to_string()))
}

fn folder_beside(input: &Path) -> (Option<PathBuf>, PathBuf) {
    let stem = input.file_stem().map_or("song".into(), |s| s.to_string_lossy().into_owned());
    let folder = format!("{stem} stems");
    (input.parent().map(|p| p.join(&folder)), stems_home().join("output").join(folder))
}

/// The folder a song's stems go in, creating it: beside the song, or in the app's folder if that can't be written.
pub fn output_dir(input: &Path) -> PathBuf {
    let (beside, fallback) = folder_beside(input);
    match beside {
        Some(beside) if std::fs::create_dir_all(&beside).is_ok() => beside,
        _ => fallback,
    }
}

const MADE_WITH: &str = "made-with.txt";

/// Notes which models made a song's stems, because other models give different stems.
fn made_with(settings: &StemSettings) -> String {
    format!("{}\n{}\n", settings.vocal_model, settings.instrument_model)
}

/// Stems already made from this exact file with these models, which saves a long wait.
pub fn existing(input: &Path, settings: &StemSettings) -> Option<BTreeMap<String, PathBuf>> {
    let (beside, fallback) = folder_beside(input);
    let made_after = std::fs::metadata(input).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    beside.into_iter().chain([fallback]).find_map(|dir| {
        if std::fs::read_to_string(dir.join(MADE_WITH)).ok()? != made_with(settings) {
            return None;
        }
        let files: BTreeMap<String, PathBuf> = STEMS.iter().map(|n| (n.to_string(), dir.join(format!("{n}.{}", settings.format.extension())))).collect();
        files.values().all(|p| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t >= made_after)).then_some(files)
    })
}

pub struct StemJob {
    pub input: PathBuf,
    pub settings: StemSettings,
    pub low_memory: bool,
    /// Folder for ffmpeg, and for turning WAV into other formats.
    pub ffmpeg: Option<PathBuf>,
}

enum Message {
    Event(serde_json::Value),
    Log(String),
    Closed,
}

pub struct StemWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Message>,
    log: VecDeque<String>,
    /// What it's running on: cuda, mps or cpu.
    pub device: String,
}

/// How much of the job's progress bar each stage takes up, and what to call it.
fn stage_span(stage: &str) -> (f32, f32, &'static str) {
    match stage {
        "load_vocals" => (0.0, 0.05, "Getting the vocal model"),
        "vocals" => (0.05, 0.5, "Separating the vocal"),
        "load_instruments" => (0.5, 0.55, "Getting the instrument model"),
        "instruments" => (0.55, 0.98, "Separating the instruments"),
        _ => (0.98, 1.0, "Saving the stems"),
    }
}

/// Reads a percentage from a progress bar line such as ` 42%|████  | 10/24`.
fn tqdm_percent(line: &str) -> Option<f32> {
    let end = line.find("%|")?;
    let start = line[..end].rfind(|c: char| !c.is_ascii_digit() && c != '.').map_or(0, |i| i + 1);
    line[start..end].parse().ok()
}

impl StemWorker {
    /// Starts the worker, and waits until it has found out what it can run on.
    pub fn start(settings: &StemSettings, cancel: &AtomicBool) -> Result<Self, StemError> {
        std::fs::create_dir_all(stems_home()).map_err(|e| StemError::Other(e.to_string()))?;
        let script = stems_home().join("separate.py");
        std::fs::write(&script, WORKER).map_err(|e| StemError::Other(e.to_string()))?;
        let python = std::env::var_os("MOUTHPIECE_STEMS_PYTHON").map_or_else(venv_python, PathBuf::from);
        let mut command = Command::new(python);
        command.arg(&script).env("PYTHONUNBUFFERED", "1").env("AUDIO_SEPARATOR_MODEL_DIR", settings.model_dir());
        // Whatever it calls has to find ffmpeg too, which a GUI app's PATH may not include.
        if let Some(dir) = mp_model::video::find_ffmpeg().and_then(|p| p.parent().map(Path::to_path_buf)) {
            let mut paths = vec![dir];
            paths.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
            if let Ok(joined) = std::env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }
        hide_console(&mut command);
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| StemError::Other(format!("Python wouldn't start: {e}")))?;
        let stdin = child.stdin.take();
        let (tx, messages) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(value) = serde_json::from_str(&line) {
                        let _ = tx.send(Message::Event(value));
                    }
                }
                let _ = tx.send(Message::Closed);
            });
        }
        if let Some(mut stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                // Progress bars rewrite a line with carriage returns, so lines end either way.
                let mut chunk = [0u8; 4096];
                let mut line = Vec::new();
                while let Ok(n) = stderr.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    for byte in &chunk[..n] {
                        if *byte == b'\n' || *byte == b'\r' {
                            let _ = tx.send(Message::Log(String::from_utf8_lossy(&line).into_owned()));
                            line.clear();
                        } else {
                            line.push(*byte);
                        }
                    }
                }
            });
        }
        let mut worker = Self { child, stdin, messages, log: VecDeque::new(), device: String::new() };
        worker.wait_for(cancel, &mut |event, _| {
            if event["event"] == "ready" {
                return Some(Ok(event["device"].as_str().unwrap_or("cpu").to_string()));
            }
            None
        })
        .map(|device| {
            worker.device = device;
            worker
        })
    }

    /// Whether it's still running. A crash, as from running out of memory, ends it between jobs.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Reads messages until `handle` has an answer, the process ends, or the job's cancelled.
    fn wait_for<T>(&mut self, cancel: &AtomicBool, handle: &mut dyn FnMut(&serde_json::Value, Option<f32>) -> Option<Result<T, StemError>>) -> Result<T, StemError> {
        const POLL: Duration = Duration::from_millis(100);
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(StemError::Cancelled);
            }
            match self.messages.recv_timeout(POLL) {
                Ok(Message::Event(event)) => {
                    if let Some(answer) = handle(&event, None) {
                        return answer;
                    }
                }
                Ok(Message::Log(line)) => {
                    if !line.trim().is_empty() {
                        if let Some(percent) = tqdm_percent(&line) {
                            let _ = handle(&serde_json::Value::Null, Some(percent / 100.0));
                        } else {
                            self.log.push_back(line);
                            if self.log.len() > LOG_LINES_KEPT {
                                self.log.pop_front();
                            }
                        }
                    }
                }
                Ok(Message::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    let _ = self.child.wait();
                    let log = self.log.iter().cloned().collect::<Vec<_>>().join("\n");
                    let lower = log.to_lowercase();
                    return Err(if lower.contains("out of memory") { StemError::OutOfMemory(log) } else { StemError::Other(log) });
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Runs one job. `on_progress` receives 0..=1 and what's happening.
    pub fn separate(&mut self, job: &StemJob, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f32, &str)) -> Result<BTreeMap<String, PathBuf>, StemError> {
        let output = output_dir(&job.input);
        let request = serde_json::json!({
            "input": job.input,
            "output_dir": output,
            "format": job.settings.format.extension(),
            "vocal_model": job.settings.vocal_model,
            "instrumental_model": job.settings.instrument_model,
            "model_dir": job.settings.model_dir(),
            "low_memory": job.low_memory,
            "ffmpeg": job.ffmpeg,
        });
        let sent = self.stdin.as_mut().ok_or(()).and_then(|s| writeln!(s, "{request}").and_then(|_| s.flush()).map_err(|_| ()));
        if sent.is_err() {
            return Err(StemError::Other("The worker stopped".to_string()));
        }
        let mut span = stage_span("load_vocals");
        let mut best = 0f32;
        on_progress(0.0, span.2);
        let result = self.wait_for(cancel, &mut |event, percent| {
            if let Some(percent) = percent {
                best = best.max(span.0 + (span.1 - span.0) * percent);
                on_progress(best, span.2);
                return None;
            }
            match event["event"].as_str() {
                Some("stage") => {
                    span = stage_span(event["stage"].as_str().unwrap_or(""));
                    best = span.0;
                    on_progress(best, span.2);
                    None
                }
                Some("done") => {
                    let files: BTreeMap<String, PathBuf> = serde_json::from_value(event["files"].clone()).unwrap_or_default();
                    Some(Ok(files))
                }
                Some("error") => {
                    let message = event["message"].as_str().unwrap_or("").to_string();
                    Some(Err(match event["kind"].as_str() {
                        Some("bad_input") => StemError::BadInput(message),
                        Some("out_of_memory") => StemError::OutOfMemory(message),
                        Some("missing_model") => StemError::MissingModel(message),
                        Some("broken_install") => StemError::BrokenInstall(message),
                        _ => StemError::Other(message),
                    }))
                }
                _ => None,
            }
        })?;
        if let Some(missing) = STEMS.iter().find(|n| !result.get(**n).is_some_and(|p| p.is_file())) {
            return Err(StemError::Other(format!("{missing} wasn't produced")));
        }
        let _ = std::fs::write(output.join(MADE_WITH), made_with(&job.settings));
        Ok(result)
    }
}

impl Drop for StemWorker {
    fn drop(&mut self) {
        // Closing stdin ends it, and frees the memory the models held.
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_percentages_from_progress_bars() {
        assert_eq!(tqdm_percent(" 42%|████      | 10/24 [00:01<00:02]"), Some(42.0));
        assert_eq!(tqdm_percent("100%|██████████|"), Some(100.0));
        assert_eq!(tqdm_percent("INFO: loading 12% of nothing"), None);
    }

    #[test]
    fn an_old_or_incomplete_install_repairs_rather_than_starting_over() {
        assert_eq!(setup_state(false, None, "a", true), Setup::NeedsInstall);
        assert_eq!(setup_state(true, Some("a"), "a", true), Setup::Ready);
        assert_eq!(setup_state(true, Some("old"), "a", true), Setup::NeedsRepair);
        assert_eq!(setup_state(true, None, "a", true), Setup::NeedsRepair);
        assert_eq!(setup_state(true, Some("a"), "a", false), Setup::NeedsRepair);
    }

    #[test]
    fn stages_fill_the_bar_in_order() {
        let stages = ["load_vocals", "vocals", "load_instruments", "instruments", "saving"];
        for pair in stages.windows(2) {
            assert_eq!(stage_span(pair[0]).1, stage_span(pair[1]).0);
        }
    }

    #[test]
    fn stems_are_reused_only_if_all_are_there_from_the_same_models() {
        let dir = std::env::temp_dir().join(format!("mp-stems-reuse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("song.wav");
        std::fs::write(&input, b"x").unwrap();
        let settings = StemSettings::default();
        assert!(existing(&input, &settings).is_none());
        assert!(!dir.join("song stems").exists(), "looking mustn't leave a folder behind");
        let out = output_dir(&input);
        for name in STEMS {
            std::fs::write(out.join(format!("{name}.wav")), b"x").unwrap();
        }
        assert!(existing(&input, &settings).is_none(), "no record of what made them");
        std::fs::write(out.join(MADE_WITH), made_with(&settings)).unwrap();
        assert!(existing(&input, &settings).is_some());
        assert!(existing(&input, &StemSettings { format: StemFormat::Flac, ..settings.clone() }).is_none());
        assert!(existing(&input, &StemSettings { vocal_model: "other.ckpt".to_string(), ..settings.clone() }).is_none());
        std::fs::remove_file(out.join("Bass.wav")).unwrap();
        assert!(existing(&input, &settings).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
