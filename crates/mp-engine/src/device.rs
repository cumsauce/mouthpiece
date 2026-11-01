//! The device interface.
//!
//! It mirrors the CLAP plugin lifecycle (activate, stable parameter ids, process, latency), so a
//! CLAP plugin can later be wrapped as a `Device` with no change to the graph or the UI.

#[derive(Clone, Copy, Debug)]
pub struct ProcessContext {
    pub sample_rate: f32,
    pub bpm: f32,
    pub playing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Db,
    Hz,
    Ms,
    Seconds,
    Percent,
    Ratio,
    Beats,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    /// Knob travel is logarithmic in the value, for frequencies and times.
    Log,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    /// Stable id stored in project files. Never reuse one.
    pub id: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: Unit,
    pub curve: Curve,
}

impl ParamInfo {
    /// Value to knob position in 0..=1.
    pub fn normalize(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.curve {
            Curve::Linear => (v - self.min) / (self.max - self.min),
            Curve::Log => (v / self.min).ln() / (self.max / self.min).ln(),
        }
    }

    /// Knob position in 0..=1 to value.
    pub fn denormalize(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        match self.curve {
            Curve::Linear => self.min + n * (self.max - self.min),
            Curve::Log => self.min * (self.max / self.min).powf(n),
        }
    }

    pub fn format(&self, v: f32) -> String {
        match self.unit {
            Unit::Db => format!("{v:+.1} dB"),
            Unit::Hz if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
            Unit::Hz => format!("{v:.0} Hz"),
            Unit::Ms if v >= 100.0 => format!("{v:.0} ms"),
            Unit::Ms => format!("{v:.1} ms"),
            Unit::Seconds => format!("{v:.2} s"),
            Unit::Percent => format!("{:.0}%", v * 100.0),
            Unit::Ratio => format!("{v:.1}:1"),
            Unit::Beats if v < 0.001 => "Free".to_string(),
            Unit::Beats => format!("{v:.2} beats"),
            Unit::None => format!("{v:.2}"),
        }
    }
}

/// An audio processor in a track's chain. `process` runs on the audio thread and must not block
/// or allocate. Everything else runs off the audio thread, except `set_param`, which must be
/// real-time safe too.
pub trait Device: Send {
    /// Prepare for a sample rate. May allocate.
    fn activate(&mut self, sample_rate: f32);
    /// Clear all internal audio state, such as delay lines.
    fn reset(&mut self);
    fn params(&self) -> &[ParamInfo];
    fn set_param(&mut self, id: u32, value: f32);
    /// Process in place. Both slices have the same length, at most `graph::MAX_BLOCK`.
    fn process(&mut self, ctx: &ProcessContext, l: &mut [f32], r: &mut [f32]);
    /// Frames of delay this device adds.
    fn latency(&self) -> usize {
        0
    }
}
