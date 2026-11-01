//! Effects on the picture. They sit in a chain per track, like audio devices, and any parameter can
//! move: by keyframes, on the beat, or following a track's level. Any effect can be limited to an area.

use crate::{DeviceId, Project, Seconds, TrackId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PictureKind {
    Transform,
    Color,
    Blur,
    Warp,
    RgbSplit,
    Pixelate,
    Glitch,
    Shake,
    Mirror,
    Flash,
}

impl PictureKind {
    pub const ALL: [PictureKind; 10] = [
        PictureKind::Transform,
        PictureKind::Color,
        PictureKind::Blur,
        PictureKind::Warp,
        PictureKind::RgbSplit,
        PictureKind::Pixelate,
        PictureKind::Glitch,
        PictureKind::Shake,
        PictureKind::Mirror,
        PictureKind::Flash,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PictureKind::Transform => "Transform",
            PictureKind::Color => "Color",
            PictureKind::Blur => "Blur",
            PictureKind::Warp => "Warp",
            PictureKind::RgbSplit => "RGB Split",
            PictureKind::Pixelate => "Pixelate",
            PictureKind::Glitch => "Glitch",
            PictureKind::Shake => "Shake",
            PictureKind::Mirror => "Mirror",
            PictureKind::Flash => "Flash",
        }
    }
}

/// How a value travels from one keyframe to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    Smooth,
    Linear,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: Seconds,
    pub value: f32,
    pub ease: Ease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PulseShape {
    /// A hit that dies away, the usual choice for something on the beat.
    Decay,
    Sine,
    Saw,
    Square,
}

/// Moves a value in time with the grid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatMotion {
    /// How many beats between pulses.
    pub every: f32,
    pub shape: PulseShape,
    pub amount: f32,
}

/// Moves a value with how loud a track is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FollowMotion {
    pub track: TrackId,
    pub amount: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// In time order.
    pub keys: Vec<Key>,
    pub beat: Option<BeatMotion>,
    pub follow: Option<FollowMotion>,
}

impl Motion {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.beat.is_none() && self.follow.is_none()
    }

    /// The value at a time. `beats` is the position on the grid, and `level` the followed track's level.
    pub fn value(&self, base: f32, t: Seconds, beats: f64, level: f32) -> f32 {
        let mut value = self.keyed(base, t);
        if let Some(beat) = &self.beat {
            value += beat.pulse(beats) * beat.amount;
        }
        if let Some(follow) = &self.follow {
            value += level * follow.amount;
        }
        value
    }

    /// The value from keyframes alone.
    pub fn keyed(&self, base: f32, t: Seconds) -> f32 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else { return base };
        if t <= first.t {
            return first.value;
        }
        if t >= last.t {
            return last.value;
        }
        let next = self.keys.partition_point(|k| k.t <= t);
        let (a, b) = (self.keys[next - 1], self.keys[next]);
        let x = ((t - a.t) / (b.t - a.t).max(1e-9)) as f32;
        let shaped = match a.ease {
            Ease::Hold => 0.0,
            Ease::Linear => x,
            Ease::Smooth => x * x * (3.0 - 2.0 * x),
        };
        a.value + (b.value - a.value) * shaped
    }

    /// Sets the value at a time, replacing a key that's very close. Returns its index.
    pub fn set_key(&mut self, t: Seconds, value: f32) -> usize {
        const SAME_KEY: Seconds = 0.005;
        if let Some(i) = self.keys.iter().position(|k| (k.t - t).abs() < SAME_KEY) {
            self.keys[i].value = value;
            return i;
        }
        let at = self.keys.partition_point(|k| k.t < t);
        self.keys.insert(at, Key { t, value, ease: Ease::Smooth });
        at
    }

    /// Moves a key in time, keeping the keys in order. Returns its new index.
    pub fn move_key(&mut self, index: usize, t: Seconds, value: f32) -> usize {
        let Some(mut key) = self.keys.get(index).copied() else { return index };
        self.keys.remove(index);
        key.t = t.max(0.0);
        key.value = value;
        let at = self.keys.partition_point(|k| k.t < key.t);
        self.keys.insert(at, key);
        at
    }
}

impl BeatMotion {
    /// 0..=1, starting from each pulse.
    pub fn pulse(&self, beats: f64) -> f32 {
        const DECAY_RATE: f32 = 5.0;
        let phase = (beats / self.every.max(0.01) as f64).rem_euclid(1.0) as f32;
        match self.shape {
            PulseShape::Decay => (-phase * DECAY_RATE).exp(),
            PulseShape::Sine => 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos(),
            PulseShape::Saw => 1.0 - phase,
            PulseShape::Square => (phase < 0.5) as u8 as f32,
        }
    }
}

// The parameters every effect has for its area. These ids are out of the way of any effect's own.
pub const AREA_X: u32 = 100;
pub const AREA_Y: u32 = 101;
pub const AREA_W: u32 = 102;
pub const AREA_H: u32 = 103;
pub const AREA_FEATHER: u32 = 104;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PictureEffect {
    pub id: DeviceId,
    pub kind: PictureKind,
    pub enabled: bool,
    pub params: BTreeMap<u32, f32>,
    pub motion: BTreeMap<u32, Motion>,
    /// Limits the effect to part of the picture.
    pub area: bool,
    pub area_ellipse: bool,
    pub area_invert: bool,
}

impl PictureEffect {
    /// Sets what a parameter is at a time. If it has keyframes, that's a keyframe at that time.
    pub fn set_value(&mut self, param: u32, value: f32, t: Seconds) {
        match self.motion.get_mut(&param) {
            Some(motion) if !motion.keys.is_empty() => {
                motion.set_key(t, value);
            }
            _ => {
                self.params.insert(param, value);
            }
        }
    }
}

impl Project {
    pub fn picture_chain(&self, track: Option<TrackId>) -> Option<&Vec<PictureEffect>> {
        match track {
            Some(t) => self.track(t).map(|t| &t.picture),
            None => Some(&self.master.picture),
        }
    }

    pub fn picture_chain_mut(&mut self, track: Option<TrackId>) -> Option<&mut Vec<PictureEffect>> {
        match track {
            Some(t) => self.track_mut(t).map(|t| &mut t.picture),
            None => Some(&mut self.master.picture),
        }
    }

    pub fn add_picture_effect(&mut self, track: Option<TrackId>, kind: PictureKind) -> DeviceId {
        let id = DeviceId(self.alloc_id());
        let effect = PictureEffect {
            id,
            kind,
            enabled: true,
            params: BTreeMap::new(),
            motion: BTreeMap::new(),
            area: false,
            area_ellipse: true,
            area_invert: false,
        };
        if let Some(chain) = self.picture_chain_mut(track) {
            chain.push(effect);
        }
        id
    }

    pub fn picture_effect_mut(&mut self, id: DeviceId) -> Option<&mut PictureEffect> {
        self.tracks.iter_mut().flat_map(|t| t.picture.iter_mut()).chain(self.master.picture.iter_mut()).find(|e| e.id == id)
    }

    pub fn picture_effect(&self, id: DeviceId) -> Option<&PictureEffect> {
        self.tracks.iter().flat_map(|t| t.picture.iter()).chain(self.master.picture.iter()).find(|e| e.id == id)
    }

    /// Whether anything would change the picture, which decides whether rendering needs the GPU.
    pub fn has_picture_effects(&self) -> bool {
        self.tracks.iter().flat_map(|t| t.picture.iter()).chain(self.master.picture.iter()).any(|e| e.enabled)
    }

    /// The position on the beat grid at a time.
    pub fn beats_at(&self, t: Seconds) -> f64 {
        (t - self.grid_offset) / self.beat_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ease: Ease) -> Motion {
        let mut m = Motion::default();
        m.set_key(1.0, 0.0);
        m.set_key(3.0, 10.0);
        m.keys[0].ease = ease;
        m
    }

    #[test]
    fn keys_interpolate_by_their_ease() {
        assert_eq!(keys(Ease::Linear).value(99.0, 2.0, 0.0, 0.0), 5.0);
        assert_eq!(keys(Ease::Hold).value(99.0, 2.5, 0.0, 0.0), 0.0);
        let smooth = keys(Ease::Smooth).value(99.0, 1.5, 0.0, 0.0);
        assert!(smooth < 2.5 && smooth > 0.0, "{smooth}");
    }

    #[test]
    fn keys_hold_outside_their_range_and_base_applies_with_none() {
        let m = keys(Ease::Linear);
        assert_eq!(m.value(99.0, 0.0, 0.0, 0.0), 0.0);
        assert_eq!(m.value(99.0, 9.0, 0.0, 0.0), 10.0);
        assert_eq!(Motion::default().value(7.0, 1.0, 0.0, 0.0), 7.0);
    }

    #[test]
    fn beat_motion_hits_on_the_grid_and_decays() {
        let m = Motion { beat: Some(BeatMotion { every: 1.0, shape: PulseShape::Decay, amount: 2.0 }), ..Default::default() };
        assert_eq!(m.value(1.0, 0.0, 4.0, 0.0), 3.0);
        assert!(m.value(1.0, 0.0, 4.5, 0.0) < 1.3);
    }

    #[test]
    fn follow_adds_the_tracks_level() {
        let m = Motion { follow: Some(FollowMotion { track: TrackId(1), amount: 0.5 }), ..Default::default() };
        assert_eq!(m.value(1.0, 0.0, 0.0, 0.5), 1.25);
    }

    #[test]
    fn setting_a_keyframed_value_keys_it_at_that_time() {
        let mut p = Project::default();
        let id = p.add_picture_effect(None, PictureKind::Transform);
        let e = p.picture_effect_mut(id).unwrap();
        e.set_value(2, 1.5, 3.0);
        assert_eq!(e.params[&2], 1.5);
        e.motion.insert(2, keys(Ease::Linear));
        e.set_value(2, 4.0, 2.0);
        assert_eq!(e.motion[&2].keys.len(), 3);
        assert_eq!(e.params[&2], 1.5, "the base value stays");
    }

    #[test]
    fn moved_keys_stay_in_order() {
        let mut m = keys(Ease::Linear);
        let at = m.move_key(0, 5.0, 1.0);
        assert_eq!(at, 1);
        assert!(m.keys[0].t < m.keys[1].t);
    }
}
