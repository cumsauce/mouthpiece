//! Turns a `Project` into an `EngineGraph`. Runs on the UI thread.

use crate::devices;
use crate::graph::{DeviceSlot, EngineGraph, TrackGraph, Voice};
use crate::render_cache::{RenderCache, RenderKey};
use mp_dsp::{db_to_gain, pan_gains, Stereo};
use mp_model::{Clip, DeviceId, DeviceInstance, Project, SourceId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Decoded audio by source id, always at the engine's sample rate.
#[derive(Default, Clone)]
pub struct SourceBank {
    pub buffers: HashMap<SourceId, Arc<Stereo>>,
}

impl SourceBank {
    pub fn get(&self, id: SourceId) -> Option<&Arc<Stereo>> {
        self.buffers.get(&id)
    }
}

/// Tracks which devices are already running, so a recompile only builds new ones.
#[derive(Default)]
pub struct GraphCompiler {
    running: HashSet<DeviceId>,
}

impl GraphCompiler {
    pub fn compile(&mut self, project: &Project, bank: &SourceBank, cache: &RenderCache, sr: u32) -> Box<EngineGraph> {
        let to_frame = |t: f64| (t.max(0.0) * sr as f64).round() as u64;
        let any_solo = project.tracks.iter().any(|t| t.solo);
        let mut running = HashSet::new();
        let tracks: Vec<TrackGraph> = project
            .tracks
            .iter()
            .map(|track| {
                let hidden: Vec<(u64, u64)> = project.hidden_ranges(track.id).iter().map(|(a, b)| (to_frame(*a), to_frame(*b))).collect();
                let mut voices: Vec<Voice> = Vec::with_capacity(track.clips.len());
                for clip in track.clips.iter().filter(|c| !c.muted) {
                    if clip.patch.is_some() && project.bypass_patches {
                        continue;
                    }
                    let Some(voice) = clip_voice(clip, bank, cache, sr) else { continue };
                    if clip.patch.is_some() {
                        voices.push(voice);
                    } else {
                        split_voice(voice, &hidden, (HIDE_FADE_S * sr as f64) as usize, &mut voices);
                    }
                }
                voices.sort_by_key(|v| v.start);
                let longest_voice = voices.iter().map(|v| v.frames as u64).max().unwrap_or(0);
                let (pl, pr) = pan_gains(track.pan);
                let gain = db_to_gain(track.gain_db);
                TrackGraph {
                    id: track.id,
                    gain_l: gain * pl,
                    gain_r: gain * pr,
                    audible: !track.mute && (!any_solo || track.solo),
                    voices,
                    longest_voice,
                    devices: self.slots(&track.devices, sr as f32, &mut running),
                    smooth_l: None,
                    smooth_r: None,
                }
            })
            .collect();
        let master_devices = self.slots(&project.master.devices, sr as f32, &mut running);
        self.running = running;
        let end_frame = to_frame(project.duration());
        Box::new(EngineGraph {
            tracks,
            master_gain: db_to_gain(project.master.gain_db),
            master_devices,
            bpm: project.bpm as f32,
            grid_offset_frames: to_frame(project.grid_offset),
            loop_frames: project
                .loop_region
                .enabled
                .then(|| (to_frame(project.loop_region.start), to_frame(project.loop_region.end)))
                .filter(|(a, b)| b > a),
            metronome: project.metronome,
            end_frame,
        })
    }

    fn slots(&self, devices: &[DeviceInstance], sr: f32, running: &mut HashSet<DeviceId>) -> Vec<DeviceSlot> {
        devices
            .iter()
            .map(|d| {
                running.insert(d.id);
                let defaults = devices::params_of(&d.kind).iter().map(|p| (p.id, p.default));
                let params: Vec<(u32, f32)> =
                    defaults.map(|(id, def)| (id, d.params.get(&id).copied().unwrap_or(def))).collect();
                DeviceSlot {
                    id: d.id,
                    enabled: d.enabled,
                    params,
                    device: (!self.running.contains(&d.id)).then(|| devices::create(&d.kind, sr)),
                }
            })
            .collect()
    }
}

fn clip_voice(c: &Clip, bank: &SourceBank, cache: &RenderCache, sr: u32) -> Option<Voice> {
    let source = bank.get(c.source)?;
    let (pl, pr) = pan_gains(c.pan);
    let gain = db_to_gain(c.gain_db);
    let start = (c.start.max(0.0) * sr as f64).round() as u64;
    let to_frames = |s: f64| (s * sr as f64).round() as usize;
    let in_frame = to_frames(c.src_in).min(source.frames());
    let out_frame = to_frames(c.src_out).min(source.frames());
    if out_frame <= in_frame {
        return None;
    }
    let (buffer, buffer_start, frames) = if c.needs_render() {
        let key = RenderKey::of(c, in_frame, out_frame);
        match cache.get_or_request(&key, source, sr) {
            Some(rendered) => {
                let frames = rendered.frames();
                (rendered, 0, frames)
            }
            // Until the render lands, play the unprocessed audio so editing is never silent.
            None => (source.clone(), in_frame, out_frame - in_frame),
        }
    } else {
        (source.clone(), in_frame, out_frame - in_frame)
    };
    if frames == 0 {
        return None;
    }
    Some(Voice {
        buffer,
        buffer_start,
        frames,
        start,
        reversed: c.reversed,
        gain_l: gain * pl,
        gain_r: gain * pr,
        fade_in: to_frames(c.fade_in),
        fade_out: to_frames(c.fade_out),
    })
}

/// How quickly a recording fades out and in around a change.
const HIDE_FADE_S: f64 = 0.005;

/// Plays a voice except in the given ranges of the timeline, without touching what it plays.
fn split_voice(voice: Voice, hidden: &[(u64, u64)], fade: usize, out: &mut Vec<Voice>) {
    let mut ranges: Vec<(u64, u64)> = hidden.iter().copied().filter(|(a, b)| *a < voice.end() && *b > voice.start).collect();
    if ranges.is_empty() {
        out.push(voice);
        return;
    }
    ranges.sort_unstable();
    let mut cursor = voice.start;
    let mut parts = Vec::new();
    for (a, b) in ranges {
        if a > cursor {
            parts.push((cursor, a));
        }
        cursor = cursor.max(b);
    }
    if cursor < voice.end() {
        parts.push((cursor, voice.end()));
    }
    for (a, b) in parts {
        let mut part = voice.clone();
        let (offset, frames) = ((a - voice.start) as usize, (b - a) as usize);
        // A reversed voice reads its buffer from the far end, so a later part is an earlier stretch.
        part.buffer_start += if voice.reversed { voice.frames - offset - frames } else { offset };
        part.frames = frames;
        part.start = a;
        part.fade_in = if a == voice.start { voice.fade_in } else { fade };
        part.fade_out = if b == voice.end() { voice.fade_out } else { fade };
        out.push(part);
    }
}

/// A voice that auditions part of a source immediately through the preview path.
pub fn audition_voice(buffer: Arc<Stereo>, from: f64, to: f64, reversed: bool, sr: u32) -> Option<Box<Voice>> {
    let a = ((from * sr as f64) as usize).min(buffer.frames());
    let b = ((to * sr as f64) as usize).min(buffer.frames());
    (b > a).then(|| {
        Box::new(Voice {
            buffer,
            buffer_start: a,
            frames: b - a,
            start: 0,
            reversed,
            gain_l: 1.0,
            gain_r: 1.0,
            fade_in: (0.003 * sr as f64) as usize,
            fade_out: (0.005 * sr as f64) as usize,
        })
    })
}
