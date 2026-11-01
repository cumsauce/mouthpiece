//! Runs the real worker script. The first test swaps audio-separator for a stand-in, so that it can run anywhere with Python.
//! The second is the real thing, and needs the app's stem environment set up.

use mp_analysis::stems::{StemError, StemFormat, StemJob, StemSettings, StemWorker, STEMS};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

// The environment variables below apply to the whole process.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const FAKE_LIBRARY: &str = r#"
import os, shutil, sys
class Separator:
    def __init__(self, log_level=None, model_file_dir=None, output_dir=None, output_format=None, normalization_threshold=None, mdxc_params=None, demucs_params=None):
        self.out = output_dir
    def load_model(self, model_filename=None):
        if model_filename == "missing.ckpt":
            raise ValueError("Model file missing.ckpt not found")
        if model_filename == "broken.ckpt":
            raise ModuleNotFoundError("No module named 'audioread'")
    def separate(self, path, names):
        if open(path, "rb").read(4) == b"oom!":
            raise RuntimeError("CUDA out of memory")
        print(" 50%|#####     | 1/2", file=sys.stderr)
        out = []
        for stem, name in names.items():
            # The real library can return paths relative to its output directory.
            shutil.copyfile(path, os.path.join(self.out, name + ".wav"))
            out.append(name + ".wav")
        return out
"#;

fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| std::process::Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mp-stems-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn job(input: &Path, settings: &StemSettings) -> StemJob {
    StemJob { input: input.to_path_buf(), settings: settings.clone(), low_memory: false, ffmpeg: None }
}

#[test]
fn the_worker_makes_every_stem_and_explains_failures() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(python) = python() else { return };
    let dir = scratch("fake");
    let library = dir.join("lib/audio_separator");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(library.join("__init__.py"), "").unwrap();
    std::fs::write(library.join("separator.py"), FAKE_LIBRARY).unwrap();
    std::env::set_var("PYTHONPATH", dir.join("lib"));
    std::env::set_var("MOUTHPIECE_STEMS_PYTHON", python);

    let input = dir.join("song.wav");
    std::fs::write(&input, b"audio").unwrap();
    let settings = StemSettings { model_dir: Some(dir.join("models")), ..Default::default() };
    let cancel = AtomicBool::new(false);
    let mut worker = StemWorker::start(&settings, &cancel).unwrap();
    assert!(["cpu", "cuda", "mps"].contains(&worker.device.as_str()));

    let mut stages = Vec::new();
    let mut last = 0.0;
    let files = worker
        .separate(&job(&input, &settings), &cancel, &mut |p, s| {
            assert!(p >= last, "progress went backwards: {p} after {last}");
            last = p;
            stages.push(s.to_string());
        })
        .unwrap();
    for name in STEMS {
        assert!(files[name].is_file(), "{name}");
        assert_eq!(files[name].parent(), Some(dir.join("song stems").as_path()));
    }
    assert!(stages.iter().any(|s| s == "Separating the vocal"));
    assert!(stages.iter().any(|s| s == "Saving the stems"));
    assert!(last > 0.9);

    // The worker survives failures, and carries on with the next job.
    let oom = dir.join("oom.wav");
    std::fs::write(&oom, b"oom!").unwrap();
    assert!(matches!(worker.separate(&job(&oom, &settings), &cancel, &mut |_, _| {}), Err(StemError::OutOfMemory(_))));
    let gone = dir.join("gone.wav");
    assert!(matches!(worker.separate(&job(&gone, &settings), &cancel, &mut |_, _| {}), Err(StemError::BadInput(_))));
    let missing = StemSettings { vocal_model: "missing.ckpt".to_string(), ..settings.clone() };
    assert!(matches!(worker.separate(&job(&input, &missing), &cancel, &mut |_, _| {}), Err(StemError::MissingModel(_))));
    let broken = StemSettings { vocal_model: "broken.ckpt".to_string(), ..settings.clone() };
    assert!(matches!(worker.separate(&job(&input, &broken), &cancel, &mut |_, _| {}), Err(StemError::BrokenInstall(_))));
    assert!(worker.separate(&job(&input, &settings), &cancel, &mut |_, _| {}).is_ok());

    drop(worker);
    std::env::remove_var("PYTHONPATH");
    std::env::remove_var("MOUTHPIECE_STEMS_PYTHON");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Downloads the models the first time. Run with:
/// cargo test -p mp-analysis --test stems -- --ignored --nocapture
#[test]
#[ignore]
fn real_models_make_five_stems_of_the_same_length() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    const SAMPLE_RATE: u32 = 44100;
    const SECONDS: u32 = 10;
    let dir = scratch("real");
    let input = dir.join("clip.wav");
    let spec = hound::WavSpec { channels: 2, sample_rate: SAMPLE_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(&input, spec).unwrap();
    for i in 0..SAMPLE_RATE * SECONDS {
        let t = i as f32 / SAMPLE_RATE as f32;
        let kick = (t * 200.0).sin() * (-(t * 2.0).fract() * 8.0).exp();
        let voice = (t * std::f32::consts::TAU * 220.0).sin() * 0.3;
        let sample = ((kick * 0.5 + voice) * 20000.0) as i16;
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();

    let settings = StemSettings { format: StemFormat::Wav, ..Default::default() };
    let cancel = AtomicBool::new(false);
    mp_analysis::tools::init();
    if mp_analysis::stems::setup() != mp_analysis::stems::Setup::Ready {
        println!("installing");
        mp_analysis::stems::install(&cancel, &mut |label, p| println!("{:3.0}% {label}", p * 100.0)).unwrap();
    }
    let mut worker = StemWorker::start(&settings, &cancel).unwrap();
    println!("running on {}", worker.device);
    let files = worker.separate(&job(&input, &settings), &cancel, &mut |p, s| println!("{:3.0}% {s}", p * 100.0)).unwrap();
    for name in STEMS {
        let audio = mp_analysis::decode::decode_file(&files[name], SAMPLE_RATE).unwrap();
        let seconds = audio.frames() as f64 / SAMPLE_RATE as f64;
        assert!((seconds - SECONDS as f64).abs() < 0.1, "{name} is {seconds} s");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
