//! The immutable snapshot of a project that the audio thread plays.

use crate::device::Device;
use mp_dsp::{Smoother, Stereo};
use mp_model::{DeviceId, TrackId};
use std::sync::Arc;

/// Largest block a device is ever given. Longer callbacks are split.
pub const MAX_BLOCK: usize = 1024;

/// Shortest fade at a voice edge, to stop clicks even on clips with no fades set.
pub const MIN_FADE_FRAMES: usize = 32;

/// One clip, resolved to frames and ready to mix.
#[derive(Clone)]
pub struct Voice {
    pub buffer: Arc<Stereo>,
    pub buffer_start: usize,
    pub frames: usize,
    /// Timeline position in frames.
    pub start: u64,
    pub reversed: bool,
    pub gain_l: f32,
    pub gain_r: f32,
    pub fade_in: usize,
    pub fade_out: usize,
}

impl Voice {
    /// Mix the voice into `l`/`r`, which cover timeline frames `pos..pos + l.len()`.
    /// `pos` may be any frame, so a voice can be entered in the middle.
    pub fn mix(&self, pos: u64, l: &mut [f32], r: &mut [f32]) {
        let end = self.start + self.frames as u64;
        let block_end = pos + l.len() as u64;
        if end <= pos || self.start >= block_end {
            return;
        }
        let from = self.start.saturating_sub(pos) as usize;
        let to = (end.min(block_end) - pos) as usize;
        let fade_in = self.fade_in.max(MIN_FADE_FRAMES);
        let fade_out = self.fade_out.max(MIN_FADE_FRAMES);
        let last = self.frames - 1;
        let (src_l, src_r) = (&self.buffer.l, &self.buffer.r);
        for i in from..to {
            let idx = (pos + i as u64 - self.start) as usize;
            let at = self.buffer_start + if self.reversed { last - idx } else { idx };
            let (Some(sl), Some(sr)) = (src_l.get(at), src_r.get(at)) else { continue };
            let mut env = 1.0;
            if idx < fade_in {
                env *= fade_shape((idx + 1) as f32 / fade_in as f32);
            }
            let remaining = last - idx;
            if remaining < fade_out {
                env *= fade_shape((remaining + 1) as f32 / fade_out as f32);
            }
            l[i] += sl * self.gain_l * env;
            r[i] += sr * self.gain_r * env;
        }
    }

    pub fn end(&self) -> u64 {
        self.start + self.frames as u64
    }
}

/// Equal-power fade curve, so crossfaded splices don't dip in level.
#[inline]
fn fade_shape(x: f32) -> f32 {
    (x * std::f32::consts::FRAC_PI_2).sin()
}

pub struct DeviceSlot {
    pub id: DeviceId,
    pub enabled: bool,
    pub params: Vec<(u32, f32)>,
    /// `None` means: take over the running instance with this id from the current graph.
    pub device: Option<Box<dyn Device>>,
}

pub struct TrackGraph {
    pub id: TrackId,
    pub gain_l: f32,
    pub gain_r: f32,
    pub audible: bool,
    /// Sorted by start.
    pub voices: Vec<Voice>,
    /// Longest voice, which bounds how far back a block's search must look.
    pub longest_voice: u64,
    pub devices: Vec<DeviceSlot>,
    pub smooth_l: Option<Smoother>,
    pub smooth_r: Option<Smoother>,
}

impl TrackGraph {
    /// Voices that may overlap frames starting at `pos`.
    pub fn first_voice_at(&self, pos: u64) -> usize {
        let earliest = pos.saturating_sub(self.longest_voice);
        self.voices.partition_point(|v| v.start < earliest)
    }
}

pub struct EngineGraph {
    pub tracks: Vec<TrackGraph>,
    pub master_gain: f32,
    pub master_devices: Vec<DeviceSlot>,
    pub bpm: f32,
    pub grid_offset_frames: u64,
    pub loop_frames: Option<(u64, u64)>,
    pub metronome: bool,
    pub end_frame: u64,
}

impl EngineGraph {
    pub fn empty() -> Self {
        Self {
            tracks: Vec::new(),
            master_gain: 1.0,
            master_devices: Vec::new(),
            bpm: 120.0,
            grid_offset_frames: 0,
            loop_frames: None,
            metronome: false,
            end_frame: 0,
        }
    }

    pub fn latency(&self) -> usize {
        self.master_devices
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| s.device.as_ref())
            .map(|d| d.latency())
            .sum()
    }
}
