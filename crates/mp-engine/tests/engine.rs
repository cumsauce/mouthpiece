//! Tests that run the same engine code used for playback, through offline rendering.

use mp_dsp::Stereo;
use mp_engine::graph::MAX_BLOCK;
use mp_engine::{render_offline, EngineCore, GraphCompiler, RenderCache, SourceBank};
use mp_model::{DeviceKind, Project, SourceId, TrackId, TrackKind};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

const SR: u32 = 48000;

// Counts allocations made on the current thread, to prove the audio path never allocates.
struct CountingAlloc;
thread_local!(static ALLOCS: Cell<usize> = const { Cell::new(0) });
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|a| a.set(a.get() + 1));
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let _ = ALLOCS.try_with(|a| a.set(a.get() + 1));
        System.dealloc(p, l)
    }
}
#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// A source whose samples are a ramp, so each output sample shows which source frame it came from.
fn ramp(frames: usize) -> Arc<Stereo> {
    let l: Vec<f32> = (0..frames).map(|i| (i + 1) as f32 / frames as f32).collect();
    Arc::new(Stereo { r: l.clone(), l })
}

struct Setup {
    project: Project,
    track: TrackId,
    source: SourceId,
    bank: SourceBank,
    cache: RenderCache,
}

fn setup(frames: usize) -> Setup {
    let mut project = Project::default();
    project.master.devices.clear(); // no limiter, so output equals input exactly
    let track = project.add_track("T", TrackKind::Music);
    let source = project.add_source("a.wav".into(), "a".into(), frames as f64 / SR as f64, TrackKind::Music);
    let mut bank = SourceBank::default();
    bank.buffers.insert(source, ramp(frames));
    Setup { project, track, source, bank, cache: RenderCache::new() }
}

fn render(s: &Setup, seconds: f64) -> Stereo {
    let graph = GraphCompiler::default().compile(&s.project, &s.bank, &s.cache, SR);
    let mut out = Stereo::default();
    render_offline(
        graph,
        SR as f32,
        0,
        (seconds * SR as f64) as u64,
        0,
        |l, r| {
            out.l.extend_from_slice(l);
            out.r.extend_from_slice(r);
        },
        |_| true,
    );
    out
}

#[test]
fn clip_plays_at_its_position_from_its_source_offset() {
    let mut s = setup(48000);
    s.project.add_clip(s.track, s.source, 0.25, 0.5, 0.75).unwrap();
    let out = render(&s, 1.0);
    assert_eq!(out.frames(), 48000);
    assert_eq!(out.l[11999], 0.0, "silent before the clip");
    let mid = 12000 + 6000;
    let expected = (24000 + 6000 + 1) as f32 / 48000.0;
    assert!((out.l[mid] - expected).abs() < 1e-5, "{} vs {expected}", out.l[mid]);
    assert_eq!(out.l[24000], 0.0, "silent after the clip");
}

#[test]
fn reversed_clip_plays_backwards() {
    let mut s = setup(48000);
    let c = s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    s.project.clip_mut(c).unwrap().reversed = true;
    let out = render(&s, 1.0);
    assert!(out.l[1000] > out.l[2000]);
    assert!((out.l[1000] - (48000 - 1000) as f32 / 48000.0).abs() < 1e-4);
}

#[test]
fn fades_shape_the_edges() {
    let mut s = setup(48000);
    let c = s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    s.project.clip_mut(c).unwrap().fade_in = 0.5;
    let out = render(&s, 1.0);
    // Halfway through an equal-power fade the gain is sin(pi/4).
    let expected = (12000f32 / 48000.0) * std::f32::consts::FRAC_1_SQRT_2;
    assert!((out.l[11999] - expected).abs() < 1e-3, "{} vs {expected}", out.l[11999]);
}

#[test]
fn every_clip_starts_and_ends_without_a_click() {
    let mut s = setup(48000);
    let flat = Arc::new(Stereo { l: vec![1.0; 48000], r: vec![1.0; 48000] });
    s.bank.buffers.insert(s.source, flat);
    s.project.add_clip(s.track, s.source, 0.1, 0.0, 0.1).unwrap();
    let out = render(&s, 0.3);
    let biggest_step = out.l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(biggest_step < 0.1, "step of {biggest_step}");
}

#[test]
fn mute_and_solo() {
    let mut s = setup(48000);
    s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    let other = s.project.add_track("Other", TrackKind::Music);
    s.project.track_mut(other).unwrap().solo = true;
    let out = render(&s, 0.5);
    let late_peak = out.l[12000..].iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(late_peak < 1e-3, "soloing another track must silence this one: {late_peak}");
}

#[test]
fn clip_gain_and_track_pan_apply() {
    let mut s = setup(48000);
    let c = s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    s.project.clip_mut(c).unwrap().gain_db = -6.0206;
    s.project.track_mut(s.track).unwrap().pan = -1.0;
    let out = render(&s, 1.0);
    assert!(out.r[40000].abs() < 1e-3, "hard left must silence right");
    let expected = (40001f32 / 48000.0) * 0.5 * std::f32::consts::SQRT_2;
    assert!((out.l[40000] - expected).abs() < 1e-3, "{} vs {expected}", out.l[40000]);
}

#[test]
fn stretched_clip_lands_after_its_render_is_ready() {
    let mut s = setup(48000);
    let c = s.project.add_clip(s.track, s.source, 0.0, 0.0, 0.5).unwrap();
    s.project.set_stretch(c, 2.0);
    // First compile queues the render and falls back to the raw audio.
    let graph = GraphCompiler::default().compile(&s.project, &s.bank, &s.cache, SR);
    assert_eq!(graph.tracks[0].voices[0].frames, 24000);
    s.cache.wait_idle();
    let graph = GraphCompiler::default().compile(&s.project, &s.bank, &s.cache, SR);
    assert_eq!(graph.tracks[0].voices[0].frames, 48000);
}

#[test]
fn loop_wraps_the_transport() {
    let mut s = setup(48000);
    s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    s.project.loop_region.enabled = true;
    s.project.loop_region.start = 0.0;
    s.project.loop_region.end = 0.1;
    let mut core = EngineCore::new(SR as f32);
    drop(core.install_graph(GraphCompiler::default().compile(&s.project, &s.bank, &s.cache, SR)));
    core.play();
    let (mut l, mut r) = (vec![0.0; 10000], vec![0.0; 10000]);
    core.process(&mut l, &mut r);
    assert_eq!(core.position, 10000 - 4800 * 2);
    // Audio jumps back at the loop point.
    assert!(l[4801] < l[4700]);
}

#[test]
fn device_state_survives_a_graph_swap() {
    let mut s = setup(48000);
    s.project.add_clip(s.track, s.source, 0.0, 0.0, 0.1).unwrap();
    let d = s.project.add_device(Some(s.track), DeviceKind::Reverb);
    s.project.device_chain_mut(Some(s.track)).unwrap()[0].params.insert(3, 1.0);
    let _ = d;
    let mut compiler = GraphCompiler::default();
    let mut core = EngineCore::new(SR as f32);
    drop(core.install_graph(compiler.compile(&s.project, &s.bank, &s.cache, SR)));
    core.play();
    let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
    for _ in 0..6 {
        core.process(&mut l, &mut r);
    }
    // Change a parameter and swap graphs: the tail already in the reverb must keep ringing.
    s.project.track_mut(s.track).unwrap().gain_db = -1.0;
    let next = compiler.compile(&s.project, &s.bank, &s.cache, SR);
    assert!(next.tracks[0].devices[0].device.is_none(), "running device must be reused, not rebuilt");
    drop(core.install_graph(next));
    core.stop();
    core.process(&mut l, &mut r);
    let peak = l.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak > 1e-3, "tail was lost: {peak}");
}

#[test]
fn master_limiter_holds_the_ceiling_and_latency_is_compensated() {
    let mut s = setup(48000);
    let flat = Arc::new(Stereo { l: vec![0.1; 48000], r: vec![0.1; 48000] });
    s.bank.buffers.insert(s.source, flat);
    s.project.add_clip(s.track, s.source, 0.5, 0.0, 0.25).unwrap();
    s.project.add_device(None, DeviceKind::Limiter);
    let out = render(&s, 1.0);
    assert_eq!(out.frames(), 48000);
    // Onset must land at 0.5 s despite the limiter's look-ahead.
    let onset = out.l.iter().position(|v| v.abs() > 0.05).unwrap();
    assert!((24000..24040).contains(&onset), "onset at {onset}");

    let c = s.project.tracks[0].clips[0].id;
    s.project.clip_mut(c).unwrap().gain_db = 24.0;
    let out = render(&s, 1.0);
    let peak = out.l.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak <= 0.97, "peak {peak}"); // default ceiling is -0.3 dB
}

#[test]
fn stopping_mid_clip_does_not_click() {
    let mut s = setup(48000);
    let flat = Arc::new(Stereo { l: vec![0.8; 48000], r: vec![0.8; 48000] });
    s.bank.buffers.insert(s.source, flat);
    s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    let mut core = EngineCore::new(SR as f32);
    drop(core.install_graph(GraphCompiler::default().compile(&s.project, &s.bank, &s.cache, SR)));
    core.play();
    let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
    core.process(&mut l, &mut r);
    let mut out = l.clone();
    core.stop();
    core.process(&mut l, &mut r);
    out.extend_from_slice(&l);
    assert!(!core.playing);
    let biggest_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(biggest_step < 0.05, "step of {biggest_step}");
    assert_eq!(*out.last().unwrap(), 0.0);
}

#[test]
fn a_change_replaces_the_recording_only_where_it_applies() {
    let mut s = setup(48000);
    s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    let silent = Arc::new(Stereo { l: vec![0.0; 48000], r: vec![0.0; 48000] });
    let other = s.project.add_source("b.wav".into(), "b".into(), 1.0, TrackKind::Music);
    s.bank.buffers.insert(other, silent);
    let id = s.project.create_patch(s.track, (0.25, 0.5), "old".into(), (s.source, 0, 0));
    let clip = s.project.add_clip(s.track, other, 0.25, 0.0, 0.25).unwrap();
    s.project.add_patch_version(id, "new".into(), &[clip]);

    // The ramp source plays either side, keeps its position, and is silent during the change.
    let out = render(&s, 1.0);
    assert!((out.l[6000] - 6001.0 / 48000.0).abs() < 1e-4);
    assert!(out.l[18000].abs() < 1e-4, "{}", out.l[18000]);
    assert!((out.l[36000] - 36001.0 / 48000.0).abs() < 1e-4, "{}", out.l[36000]);

    // Switching back plays the recording straight through, as though nothing happened.
    s.project.bypass_patches = true;
    let out = render(&s, 1.0);
    assert!((out.l[18000] - 18001.0 / 48000.0).abs() < 1e-4, "{}", out.l[18000]);
}

#[test]
fn a_change_hides_the_right_part_of_a_reversed_clip() {
    let mut s = setup(48000);
    let c = s.project.add_clip(s.track, s.source, 0.0, 0.0, 1.0).unwrap();
    s.project.clip_mut(c).unwrap().reversed = true;
    let id = s.project.create_patch(s.track, (0.25, 0.5), "old".into(), (s.source, 0, 0));
    s.project.add_patch_version(id, "new".into(), &[]);
    let out = render(&s, 1.0);
    assert!(out.l[18000].abs() < 1e-4);
    assert!((out.l[36000] - (48000.0 - 36000.0) / 48000.0).abs() < 1e-4, "{}", out.l[36000]);
}

#[test]
fn audio_path_never_allocates() {
    let mut s = setup(480000);
    for i in 0..200 {
        let t = i as f64 * 0.05;
        s.project.add_clip(s.track, s.source, t, t, t + 0.1).unwrap();
    }
    for kind in [DeviceKind::Utility, DeviceKind::Eq, DeviceKind::Compressor, DeviceKind::Gate, DeviceKind::Saturator, DeviceKind::Delay, DeviceKind::Reverb] {
        s.project.add_device(Some(s.track), kind);
    }
    s.project.add_device(None, DeviceKind::Limiter);
    s.project.metronome = true;
    s.project.loop_region.enabled = true;
    s.project.loop_region.end = 4.0;
    let mut compiler = GraphCompiler::default();
    let mut core = EngineCore::new(SR as f32);
    drop(core.install_graph(compiler.compile(&s.project, &s.bank, &s.cache, SR)));
    core.play();
    let (mut l, mut r) = (vec![0.0; MAX_BLOCK * 2], vec![0.0; MAX_BLOCK * 2]);
    let next = compiler.compile(&s.project, &s.bank, &s.cache, SR);

    ALLOCS.with(|a| a.set(0));
    for _ in 0..300 {
        core.process(&mut l, &mut r);
    }
    let old = core.install_graph(next);
    for _ in 0..10 {
        core.process(&mut l, &mut r);
    }
    let count = ALLOCS.with(|a| a.get());
    assert_eq!(count, 0, "audio path allocated {count} times");
    assert!(l.iter().all(|x| x.is_finite()));
    drop(old);
}
