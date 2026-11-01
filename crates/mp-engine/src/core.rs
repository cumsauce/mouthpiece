//! The block processor. Real-time safe: nothing in `process` allocates, locks or blocks.

use crate::device::ProcessContext;
use crate::graph::{DeviceSlot, EngineGraph, Voice, MAX_BLOCK};
use mp_dsp::Smoother;

const CLICK_SECONDS: f32 = 0.03;

pub struct EngineCore {
    sample_rate: f32,
    graph: Box<EngineGraph>,
    pub position: u64,
    pub playing: bool,
    /// Set while the transport ramps down to a stop, so stopping mid-clip doesn't click.
    stopping: bool,
    /// Gain on clip audio, ramped when the transport starts, stops or jumps.
    voice_gain: f32,
    voice_gain_step: f32,
    audition: Option<Box<Voice>>,
    audition_pos: u64,
    master_smooth: Smoother,
    track_l: Vec<f32>,
    track_r: Vec<f32>,
    click_phase: f32,
    click_left: usize,
    click_accent: bool,
    /// Peak levels of the last block, per track as [left, right], for meters.
    pub track_peaks: Vec<[f32; 2]>,
    pub master_peak: [f32; 2],
}

impl EngineCore {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            graph: Box::new(EngineGraph::empty()),
            position: 0,
            playing: false,
            stopping: false,
            voice_gain: 0.0,
            voice_gain_step: 1.0 / (0.005 * sample_rate),
            audition: None,
            audition_pos: 0,
            master_smooth: Smoother::new(1.0, sample_rate, 10.0),
            track_l: vec![0.0; MAX_BLOCK],
            track_r: vec![0.0; MAX_BLOCK],
            click_phase: 0.0,
            click_left: 0,
            click_accent: false,
            track_peaks: vec![[0.0; 2]; crate::output::MAX_METERS],
            master_peak: [0.0; 2],
        }
    }

    /// Installs a new graph and returns the old one, which must be dropped off the audio thread.
    /// Running device instances move into the new graph, so tails and filter memory survive edits.
    pub fn install_graph(&mut self, mut new: Box<EngineGraph>) -> Box<EngineGraph> {
        let EngineGraph { tracks: old_tracks, master_devices: old_master, .. } = &mut *self.graph;
        for track in &mut new.tracks {
            if let Some(old) = old_tracks.iter_mut().find(|t| t.id == track.id) {
                track.smooth_l = old.smooth_l;
                track.smooth_r = old.smooth_r;
            }
            for slot in &mut track.devices {
                adopt_device(slot, old_tracks, old_master);
            }
        }
        for slot in &mut new.master_devices {
            adopt_device(slot, old_tracks, old_master);
        }
        for slot in new.tracks.iter_mut().flat_map(|t| t.devices.iter_mut()).chain(new.master_devices.iter_mut()) {
            if let Some(device) = &mut slot.device {
                for (id, value) in &slot.params {
                    device.set_param(*id, *value);
                }
            }
        }
        std::mem::replace(&mut self.graph, new)
    }

    pub fn graph(&self) -> &EngineGraph {
        &self.graph
    }

    pub fn seek(&mut self, position: u64) {
        self.position = position;
        self.voice_gain = 0.0;
    }

    pub fn play(&mut self) {
        self.playing = true;
        self.stopping = false;
    }

    /// Stops after a few milliseconds' fade. Effect tails keep ringing.
    pub fn stop(&mut self) {
        self.stopping = self.playing;
    }

    /// Starts an audition voice, which must have `start == 0`. It plays from its first frame right
    /// away. Returns the previous voice for deallocation off the audio thread.
    pub fn audition(&mut self, voice: Option<Box<Voice>>) -> Option<Box<Voice>> {
        self.audition_pos = 0;
        std::mem::replace(&mut self.audition, voice)
    }

    /// Progress through the current audition, as (frames played, total frames).
    pub fn audition_progress(&self) -> Option<(u64, u64)> {
        self.audition.as_ref().map(|v| (self.audition_pos.min(v.frames as u64), v.frames as u64))
    }

    /// Fills `l`/`r` (equal lengths, any size) with the next frames of output.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut done = 0;
        while done < l.len() {
            let mut n = (l.len() - done).min(MAX_BLOCK);
            if self.playing {
                if let Some((_, end)) = self.graph.loop_frames {
                    if self.position < end {
                        n = n.min((end - self.position) as usize);
                    }
                }
            }
            self.process_block(&mut l[done..done + n], &mut r[done..done + n]);
            done += n;
        }
    }

    fn process_block(&mut self, l: &mut [f32], r: &mut [f32]) {
        let frames = l.len();
        l.fill(0.0);
        r.fill(0.0);
        let ctx = ProcessContext { sample_rate: self.sample_rate, bpm: self.graph.bpm, playing: self.playing };
        let pos = self.position;
        let playing = self.playing;
        let ramp_from = self.voice_gain;
        let ramp_step = if self.stopping { -self.voice_gain_step } else { self.voice_gain_step };
        self.voice_gain = (ramp_from + ramp_step * frames as f32).clamp(0.0, 1.0);
        for (index, track) in self.graph.tracks.iter_mut().enumerate() {
            let tl = &mut self.track_l[..frames];
            let tr = &mut self.track_r[..frames];
            tl.fill(0.0);
            tr.fill(0.0);
            if playing && track.audible {
                let first = track.first_voice_at(pos);
                let block_end = pos + frames as u64;
                for voice in track.voices[first..].iter().take_while(|v| v.start < block_end) {
                    voice.mix(pos, tl, tr);
                }
                if ramp_from < 1.0 || self.stopping {
                    for i in 0..frames {
                        let g = (ramp_from + ramp_step * (i + 1) as f32).clamp(0.0, 1.0);
                        tl[i] *= g;
                        tr[i] *= g;
                    }
                }
            }
            // Devices keep running when stopped, so reverb and delay tails ring out naturally.
            for slot in &mut track.devices {
                if slot.enabled {
                    if let Some(device) = &mut slot.device {
                        device.process(&ctx, tl, tr);
                    }
                }
            }
            let sr = self.sample_rate;
            let smooth_l = track.smooth_l.get_or_insert_with(|| Smoother::new(track.gain_l, sr, 10.0));
            let smooth_r = track.smooth_r.get_or_insert_with(|| Smoother::new(track.gain_r, sr, 10.0));
            let (mut peak_l, mut peak_r) = (0f32, 0f32);
            let target_l = if track.audible { track.gain_l } else { 0.0 };
            let target_r = if track.audible { track.gain_r } else { 0.0 };
            for i in 0..frames {
                let a = tl[i] * smooth_l.next(target_l);
                let b = tr[i] * smooth_r.next(target_r);
                peak_l = peak_l.max(a.abs());
                peak_r = peak_r.max(b.abs());
                l[i] += a;
                r[i] += b;
            }
            if let Some(p) = self.track_peaks.get_mut(index) {
                *p = [peak_l, peak_r];
            }
        }

        if playing && self.graph.metronome {
            self.mix_click(pos, l, r);
        }

        for slot in &mut self.graph.master_devices {
            if slot.enabled {
                if let Some(device) = &mut slot.device {
                    device.process(&ctx, l, r);
                }
            }
        }

        let master = self.graph.master_gain;
        let (mut peak_l, mut peak_r) = (0f32, 0f32);
        for i in 0..frames {
            let m = self.master_smooth.next(master);
            l[i] *= m;
            r[i] *= m;
            peak_l = peak_l.max(l[i].abs());
            peak_r = peak_r.max(r[i].abs());
        }
        self.master_peak = [peak_l, peak_r];

        // The audition voice skips the master chain, so previews are always immediate and clean.
        if let Some(voice) = &self.audition {
            voice.mix(self.audition_pos, l, r);
            self.audition_pos += frames as u64;
        }

        if self.stopping && self.voice_gain == 0.0 {
            self.playing = false;
            self.stopping = false;
        }
        if playing {
            self.position += frames as u64;
            if let Some((start, end)) = self.graph.loop_frames {
                if self.position == end {
                    self.position = start;
                }
            }
        }
    }

    fn mix_click(&mut self, pos: u64, l: &mut [f32], r: &mut [f32]) {
        let beat_frames = (60.0 / self.graph.bpm.max(1.0) * self.sample_rate) as f64;
        let offset = self.graph.grid_offset_frames as f64;
        let click_frames = (CLICK_SECONDS * self.sample_rate) as usize;
        for i in 0..l.len() {
            let t = (pos + i as u64) as f64 - offset;
            if t >= 0.0 {
                let beat = t / beat_frames;
                if beat.fract() * beat_frames < 1.0 {
                    self.click_left = click_frames;
                    self.click_phase = 0.0;
                    self.click_accent = (beat.floor() as u64) % 4 == 0;
                }
            }
            if self.click_left > 0 {
                let env = self.click_left as f32 / click_frames as f32;
                let hz = if self.click_accent { 1760.0 } else { 1320.0 };
                let s = (self.click_phase * std::f32::consts::TAU).sin() * env * env * 0.35;
                self.click_phase = (self.click_phase + hz / self.sample_rate).fract();
                self.click_left -= 1;
                l[i] += s;
                r[i] += s;
            }
        }
    }
}

fn adopt_device(slot: &mut DeviceSlot, old_tracks: &mut [crate::graph::TrackGraph], old_master: &mut [DeviceSlot]) {
    if slot.device.is_some() {
        return;
    }
    let found = old_tracks
        .iter_mut()
        .flat_map(|t| t.devices.iter_mut())
        .chain(old_master.iter_mut())
        .find(|s| s.id == slot.id);
    if let Some(old) = found {
        slot.device = old.device.take();
    }
}

/// Renders frames `start..end` of a graph. `tail_frames` of silence are added so effects ring out.
/// `on_block` receives audio; `on_progress` receives 0..=1. Rendering stops if it returns false.
pub fn render_offline(
    graph: Box<EngineGraph>,
    sample_rate: f32,
    start: u64,
    end: u64,
    tail_frames: u64,
    mut on_block: impl FnMut(&[f32], &[f32]),
    mut on_progress: impl FnMut(f32) -> bool,
) {
    let latency = graph.latency() as u64;
    let mut graph = graph;
    graph.loop_frames = None;
    graph.metronome = false;
    let mut core = EngineCore::new(sample_rate);
    drop(core.install_graph(graph));
    core.seek(start);
    core.play();
    core.voice_gain = 1.0;

    const BLOCK: usize = 1024;
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let total = end.saturating_sub(start) + tail_frames + latency;
    let mut produced = 0u64;
    while produced < total {
        let n = (total - produced).min(BLOCK as u64) as usize;
        core.process(&mut l[..n], &mut r[..n]);
        // The first frames are only the master chain's look-ahead delay, so drop them.
        let skip = latency.saturating_sub(produced).min(n as u64) as usize;
        if skip < n {
            on_block(&l[skip..n], &r[skip..n]);
        }
        produced += n as u64;
        if !on_progress(produced as f32 / total as f32) {
            break;
        }
    }
}
