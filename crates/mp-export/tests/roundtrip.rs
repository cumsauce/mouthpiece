//! Renders real files and decodes them again, to prove the whole output path.

use mp_analysis::decode::decode_file;
use mp_dsp::pitch::median_pitch;
use mp_dsp::Stereo;
use mp_engine::{GraphCompiler, RenderCache, SourceBank};
use mp_export::{export, ExportSettings, Format};
use mp_model::{Project, TrackKind};
use std::sync::Arc;

const SR: u32 = 48000;

fn graph(seconds: f64) -> Box<mp_engine::EngineGraph> {
    let frames = (seconds * SR as f64) as usize;
    let l: Vec<f32> = (0..frames).map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / SR as f32).sin() * 0.5).collect();
    let mut project = Project::default();
    let t = project.add_track("T", TrackKind::Music);
    let s = project.add_source("a".into(), "a".into(), seconds, TrackKind::Music);
    project.add_clip(t, s, 0.0, 0.0, seconds).unwrap();
    let mut bank = SourceBank::default();
    bank.buffers.insert(s, Arc::new(Stereo { r: l.clone(), l }));
    GraphCompiler::default().compile(&project, &bank, &RenderCache::new(), SR)
}

fn settings(format: Format, seconds: f64) -> ExportSettings {
    ExportSettings { format, start: 0, end: (seconds * SR as f64) as u64, tail_seconds: 0.0, title: "Test".into() }
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mouthpiece-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn mp3_renders_and_decodes_back_to_the_same_audio() {
    let path = temp("out.mp3");
    assert!(export(graph(3.0), SR, &settings(Format::Mp3 { kbps: 320 }, 3.0), &path, |_| true).unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 100_000, "320 kbps for 3 s should be near 120 KB, got {}", bytes.len());
    let decoded = decode_file(&path, SR).unwrap();
    let seconds = decoded.frames() as f64 / SR as f64;
    assert!((seconds - 3.0).abs() < 0.15, "decoded {seconds} s");
    let hz = median_pitch(&decoded.l[24000..96000], SR as f32).unwrap();
    assert!((hz - 220.0).abs() < 2.0, "pitch {hz}");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn wav_formats_preserve_audio() {
    for (format, name) in [(Format::Wav16, "a.wav"), (Format::Wav24, "b.wav"), (Format::WavFloat, "c.wav")] {
        let path = temp(name);
        assert!(export(graph(1.0), SR, &settings(format, 1.0), &path, |_| true).unwrap());
        let decoded = decode_file(&path, SR).unwrap();
        assert_eq!(decoded.frames(), 48000, "{format:?}");
        let peak = decoded.l.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.02, "{format:?} peak {peak}");
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn stretch_pitch_and_reverse_reach_the_rendered_file() {
    let seconds = 2.0;
    let frames = (seconds * SR as f64) as usize;
    let l: Vec<f32> = (0..frames).map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / SR as f32).sin() * 0.5).collect();
    let mut project = Project::default();
    let t = project.add_track("T", TrackKind::Music);
    let s = project.add_source("a".into(), "a".into(), seconds, TrackKind::Music);
    let c = project.add_clip(t, s, 0.0, 0.0, seconds).unwrap();
    project.clip_mut(c).unwrap().pitch = 12.0;
    project.clip_mut(c).unwrap().reversed = true;
    project.set_stretch(c, 1.5);
    let mut bank = SourceBank::default();
    bank.buffers.insert(s, Arc::new(Stereo { r: l.clone(), l }));

    let cache = RenderCache::new();
    drop(GraphCompiler::default().compile(&project, &bank, &cache, SR));
    cache.wait_idle();
    let graph = GraphCompiler::default().compile(&project, &bank, &cache, SR);

    let path = temp("fx.wav");
    assert!(export(graph, SR, &settings(Format::Wav24, 3.5), &path, |_| true).unwrap());
    let decoded = decode_file(&path, SR).unwrap();
    // Stretched by 1.5, so sound stops at about 3 s rather than 2 s.
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    assert!(rms(&decoded.l[(2.2 * SR as f64) as usize..(2.8 * SR as f64) as usize]) > 0.1);
    assert!(rms(&decoded.l[(3.2 * SR as f64) as usize..]) < 0.01);
    // An octave up from 220 Hz.
    let hz = median_pitch(&decoded.l[48000..96000], SR as f32).unwrap();
    assert!((hz - 440.0).abs() < 6.0, "pitch {hz}");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn cancelling_leaves_no_file_behind() {
    let path = temp("cancelled.mp3");
    let finished = export(graph(3.0), SR, &settings(Format::Mp3 { kbps: 128 }, 3.0), &path, |p| p < 0.2).unwrap();
    assert!(!finished);
    assert!(!path.exists());
    assert!(!path.with_extension("mp3.part").exists());
}
