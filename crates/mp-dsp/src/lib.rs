//! Pure signal processing. No threads, no I/O, no allocation in any `process` method.

pub mod filter;
pub mod fx;
pub mod pitch;
pub mod resample;
pub mod stretch;

use std::f32::consts::FRAC_PI_4;

/// A stereo audio buffer with separate channels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Stereo {
    pub fn silence(frames: usize) -> Self {
        Self { l: vec![0.0; frames], r: vec![0.0; frames] }
    }

    pub fn frames(&self) -> usize {
        self.l.len()
    }

    pub fn mono(&self) -> Vec<f32> {
        self.l.iter().zip(&self.r).map(|(l, r)| (l + r) * 0.5).collect()
    }

    pub fn memory_bytes(&self) -> usize {
        (self.l.len() + self.r.len()) * std::mem::size_of::<f32>()
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-6).log10()
}

/// Constant-power pan law. `pan` is -1 (left) to 1 (right). Returns (left gain, right gain),
/// normalised so that centre is unity.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2)
}

/// One-pole parameter smoother that removes zipper noise from gain changes.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    current: f32,
    coeff: f32,
}

impl Smoother {
    pub fn new(value: f32, sample_rate: f32, time_ms: f32) -> Self {
        Self { current: value, coeff: (-1.0 / (sample_rate * time_ms * 0.001)).exp() }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.current = target + (self.current - target) * self.coeff;
        self.current
    }
}

/// MIDI note number (fractional) for a frequency.
pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Name such as "A4" for a MIDI note.
pub fn midi_name(midi: f32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let n = midi.round() as i32;
    format!("{}{}", NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_centre_is_unity_and_hard_pan_silences_a_side() {
        let (l, r) = pan_gains(0.0);
        assert!((l - 1.0).abs() < 1e-5 && (r - 1.0).abs() < 1e-5);
        let (l, r) = pan_gains(-1.0);
        assert!(r.abs() < 1e-5 && l > 1.0);
    }

    #[test]
    fn note_names() {
        assert_eq!(midi_name(69.0), "A4");
        assert_eq!(midi_name(60.0), "C4");
        assert!((hz_to_midi(440.0) - 69.0).abs() < 1e-4);
    }
}
