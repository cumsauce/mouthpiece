//! Built-in devices.

use crate::device::{Curve, Device, ParamInfo, ProcessContext, Unit};
use mp_dsp::filter::{Biquad, FilterKind};
use mp_dsp::fx;
use mp_dsp::{db_to_gain, pan_gains};
use mp_model::DeviceKind;

const fn param(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, curve: Curve) -> ParamInfo {
    ParamInfo { id, name, min, max, default, unit, curve }
}
use Curve::{Linear as Lin, Log};

const UTILITY: &[ParamInfo] = &[
    param(0, "Gain", -48.0, 24.0, 0.0, Unit::Db, Lin),
    param(1, "Pan", -1.0, 1.0, 0.0, Unit::None, Lin),
    param(2, "Width", 0.0, 2.0, 1.0, Unit::Percent, Lin),
];

/// EQ layout that the UI's curve display also reads. Each band: (kind, freq id, gain id, q id).
pub const EQ_BANDS: [(FilterKind, u32, Option<u32>, Option<u32>); 6] = [
    (FilterKind::HighPass, 0, None, None),
    (FilterKind::LowShelf, 1, Some(2), None),
    (FilterKind::Peak, 3, Some(4), Some(5)),
    (FilterKind::Peak, 6, Some(7), Some(8)),
    (FilterKind::HighShelf, 9, Some(10), None),
    (FilterKind::LowPass, 11, None, None),
];

const EQ: &[ParamInfo] = &[
    param(0, "HP", 20.0, 2000.0, 20.0, Unit::Hz, Log),
    param(1, "Low", 30.0, 800.0, 120.0, Unit::Hz, Log),
    param(2, "Low Gain", -18.0, 18.0, 0.0, Unit::Db, Lin),
    param(3, "Mid 1", 100.0, 8000.0, 500.0, Unit::Hz, Log),
    param(4, "M1 Gain", -18.0, 18.0, 0.0, Unit::Db, Lin),
    param(5, "M1 Q", 0.2, 8.0, 1.0, Unit::None, Log),
    param(6, "Mid 2", 200.0, 12000.0, 2500.0, Unit::Hz, Log),
    param(7, "M2 Gain", -18.0, 18.0, 0.0, Unit::Db, Lin),
    param(8, "M2 Q", 0.2, 8.0, 1.0, Unit::None, Log),
    param(9, "High", 1000.0, 18000.0, 8000.0, Unit::Hz, Log),
    param(10, "High Gain", -18.0, 18.0, 0.0, Unit::Db, Lin),
    param(11, "LP", 1000.0, 22000.0, 22000.0, Unit::Hz, Log),
];

const COMPRESSOR: &[ParamInfo] = &[
    param(0, "Threshold", -60.0, 0.0, -18.0, Unit::Db, Lin),
    param(1, "Ratio", 1.0, 20.0, 3.0, Unit::Ratio, Log),
    param(2, "Attack", 0.1, 200.0, 8.0, Unit::Ms, Log),
    param(3, "Release", 10.0, 1500.0, 120.0, Unit::Ms, Log),
    param(4, "Makeup", 0.0, 24.0, 0.0, Unit::Db, Lin),
];

const GATE: &[ParamInfo] = &[
    param(0, "Threshold", -80.0, 0.0, -40.0, Unit::Db, Lin),
    param(1, "Floor", -90.0, 0.0, -60.0, Unit::Db, Lin),
    param(2, "Attack", 0.1, 50.0, 1.0, Unit::Ms, Log),
    param(3, "Hold", 0.5, 500.0, 30.0, Unit::Ms, Log),
    param(4, "Release", 5.0, 1000.0, 80.0, Unit::Ms, Log),
];

const SATURATOR: &[ParamInfo] = &[
    param(0, "Drive", 0.0, 36.0, 6.0, Unit::Db, Lin),
    param(1, "Tone", 500.0, 20000.0, 12000.0, Unit::Hz, Log),
    param(2, "Output", -24.0, 12.0, 0.0, Unit::Db, Lin),
    param(3, "Mix", 0.0, 1.0, 1.0, Unit::Percent, Lin),
];

const DELAY: &[ParamInfo] = &[
    param(0, "Time", 1.0, 2000.0, 350.0, Unit::Ms, Log),
    param(1, "Sync", 0.0, 4.0, 0.75, Unit::Beats, Lin),
    param(2, "Feedback", 0.0, 0.97, 0.35, Unit::Percent, Lin),
    param(3, "Ping-pong", 0.0, 1.0, 0.0, Unit::Percent, Lin),
    param(4, "Damping", 0.0, 0.95, 0.3, Unit::Percent, Lin),
    param(5, "Mix", 0.0, 1.0, 0.25, Unit::Percent, Lin),
];

const REVERB: &[ParamInfo] = &[
    param(0, "Decay", 0.1, 12.0, 1.6, Unit::Seconds, Log),
    param(1, "Damping", 0.0, 0.95, 0.35, Unit::Percent, Lin),
    param(2, "Pre-delay", 0.0, 200.0, 15.0, Unit::Ms, Lin),
    param(3, "Mix", 0.0, 1.0, 0.2, Unit::Percent, Lin),
];

const LIMITER: &[ParamInfo] = &[
    param(0, "Input", -12.0, 24.0, 0.0, Unit::Db, Lin),
    param(1, "Ceiling", -12.0, 0.0, -0.3, Unit::Db, Lin),
];

/// Parameters of a device kind, for building UI without an instance.
pub fn params_of(kind: &DeviceKind) -> &'static [ParamInfo] {
    match kind {
        DeviceKind::Utility => UTILITY,
        DeviceKind::Eq => EQ,
        DeviceKind::Compressor => COMPRESSOR,
        DeviceKind::Gate => GATE,
        DeviceKind::Saturator => SATURATOR,
        DeviceKind::Delay => DELAY,
        DeviceKind::Reverb => REVERB,
        DeviceKind::Limiter => LIMITER,
        DeviceKind::Clap { .. } => &[],
    }
}

/// Creates and activates a device. Clap devices aren't hosted yet, so they pass audio through,
/// which keeps a project with one playable.
pub fn create(kind: &DeviceKind, sample_rate: f32) -> Box<dyn Device> {
    let mut device: Box<dyn Device> = match kind {
        DeviceKind::Utility => Box::new(Utility::default()),
        DeviceKind::Eq => Box::new(Eq::default()),
        DeviceKind::Compressor => Box::new(Compressor::default()),
        DeviceKind::Gate => Box::new(Gate::default()),
        DeviceKind::Saturator => Box::new(Saturator::default()),
        DeviceKind::Delay => Box::new(Delay::default()),
        DeviceKind::Reverb => Box::new(Reverb::default()),
        DeviceKind::Limiter => Box::new(Limiter::default()),
        DeviceKind::Clap { .. } => Box::new(Passthrough),
    };
    device.activate(sample_rate);
    for p in device.params().to_vec() {
        device.set_param(p.id, p.default);
    }
    device
}

struct Passthrough;

impl Device for Passthrough {
    fn activate(&mut self, _: f32) {}
    fn reset(&mut self) {}
    fn params(&self) -> &[ParamInfo] {
        &[]
    }
    fn set_param(&mut self, _: u32, _: f32) {}
    fn process(&mut self, _: &ProcessContext, _: &mut [f32], _: &mut [f32]) {}
}

#[derive(Default)]
struct Utility {
    p: [f32; 3],
}

impl Device for Utility {
    fn activate(&mut self, _: f32) {}
    fn reset(&mut self) {}
    fn params(&self) -> &[ParamInfo] {
        UTILITY
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        let gain = db_to_gain(self.p[0]);
        let (pl, pr) = pan_gains(self.p[1]);
        let width = self.p[2];
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let mid = (*a + *b) * 0.5;
            let side = (*a - *b) * 0.5 * width;
            *a = (mid + side) * gain * pl;
            *b = (mid - side) * gain * pr;
        }
    }
}

#[derive(Default)]
struct Eq {
    p: [f32; 12],
    sr: f32,
    bands: [[Biquad; 2]; 6],
    active: [bool; 6],
}

impl Eq {
    fn update(&mut self) {
        for (i, (kind, f, g, q)) in EQ_BANDS.iter().enumerate() {
            let freq = self.p[*f as usize];
            let gain = g.map_or(0.0, |g| self.p[g as usize]);
            let q = q.map_or(0.707, |q| self.p[q as usize]);
            for ch in &mut self.bands[i] {
                ch.set(*kind, freq, q, gain, self.sr);
            }
            self.active[i] = match kind {
                FilterKind::HighPass => freq > 20.5,
                FilterKind::LowPass => freq < 21500.0,
                _ => gain.abs() > 0.01,
            };
        }
    }
}

impl Device for Eq {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
        self.update();
    }
    fn reset(&mut self) {
        self.bands.iter_mut().flatten().for_each(Biquad::reset);
    }
    fn params(&self) -> &[ParamInfo] {
        EQ
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            if *p != v {
                *p = v;
                self.update();
            }
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        for (i, band) in self.bands.iter_mut().enumerate() {
            if self.active[i] {
                let [fl, fr] = band;
                l.iter_mut().for_each(|x| *x = fl.process(*x));
                r.iter_mut().for_each(|x| *x = fr.process(*x));
            }
        }
    }
}

#[derive(Default)]
struct Compressor {
    p: [f32; 5],
    sr: f32,
    inner: fx::Compressor,
}

impl Device for Compressor {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn params(&self) -> &[ParamInfo] {
        COMPRESSOR
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
            let [t, ra, a, re, m] = self.p;
            self.inner.set(t, ra, a, re, m, self.sr);
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        self.inner.process(l, r);
    }
}

#[derive(Default)]
struct Gate {
    p: [f32; 5],
    sr: f32,
    inner: fx::Gate,
}

impl Device for Gate {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn params(&self) -> &[ParamInfo] {
        GATE
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
            let [t, f, a, h, r] = self.p;
            self.inner.set(t, f, a, h, r, self.sr);
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        self.inner.process(l, r);
    }
}

#[derive(Default)]
struct Saturator {
    p: [f32; 4],
    sr: f32,
    tone: [Biquad; 2],
}

impl Device for Saturator {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
    }
    fn reset(&mut self) {
        self.tone.iter_mut().for_each(Biquad::reset);
    }
    fn params(&self) -> &[ParamInfo] {
        SATURATOR
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
            let (freq, sr) = (self.p[1], self.sr);
            self.tone.iter_mut().for_each(|t| t.set(FilterKind::LowPass, freq, 0.707, 0.0, sr));
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        let drive = db_to_gain(self.p[0]);
        let out = db_to_gain(self.p[2]);
        let mix = self.p[3];
        let [tl, tr] = &mut self.tone;
        let run = |x: &mut [f32], t: &mut Biquad| {
            for s in x {
                let wet = t.process(fx::saturate(*s, drive)) * out;
                *s += (wet - *s) * mix;
            }
        };
        run(l, tl);
        run(r, tr);
    }
}

#[derive(Default)]
struct Delay {
    p: [f32; 6],
    sr: f32,
    inner: fx::Delay,
}

impl Device for Delay {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
        // Long enough for four beats at a slow 30 BPM.
        self.inner.allocate(8.0, sr);
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn params(&self) -> &[ParamInfo] {
        DELAY
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
        }
    }
    fn process(&mut self, ctx: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        let [time_ms, sync, fb, pp, damp, mix] = self.p;
        let secs = if sync > 0.001 { sync * 60.0 / ctx.bpm.max(1.0) } else { time_ms * 0.001 };
        self.inner.process(l, r, (secs * self.sr) as usize, fb, pp, damp, mix);
    }
}

#[derive(Default)]
struct Reverb {
    p: [f32; 4],
    sr: f32,
    inner: fx::Reverb,
}

impl Device for Reverb {
    fn activate(&mut self, sr: f32) {
        self.sr = sr;
        self.inner.allocate(sr);
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn params(&self) -> &[ParamInfo] {
        REVERB
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        let [decay, damp, pre_ms, mix] = self.p;
        self.inner.process(l, r, decay, damp, (pre_ms * 0.001 * self.sr) as usize, mix, self.sr);
    }
}

#[derive(Default)]
struct Limiter {
    p: [f32; 2],
    inner: fx::Limiter,
}

impl Device for Limiter {
    fn activate(&mut self, sr: f32) {
        self.inner.allocate(sr);
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn params(&self) -> &[ParamInfo] {
        LIMITER
    }
    fn set_param(&mut self, id: u32, v: f32) {
        if let Some(p) = self.p.get_mut(id as usize) {
            *p = v;
        }
    }
    fn process(&mut self, _: &ProcessContext, l: &mut [f32], r: &mut [f32]) {
        self.inner.process(l, r, db_to_gain(self.p[1]), db_to_gain(self.p[0]));
    }
    fn latency(&self) -> usize {
        self.inner.latency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_param_default_roundtrips_through_the_knob_mapping() {
        for kind in [DeviceKind::Utility, DeviceKind::Eq, DeviceKind::Compressor, DeviceKind::Gate, DeviceKind::Saturator, DeviceKind::Delay, DeviceKind::Reverb, DeviceKind::Limiter] {
            for p in params_of(&kind) {
                let back = p.denormalize(p.normalize(p.default));
                let tolerance = (p.max - p.min) * 1e-3;
                assert!((back - p.default).abs() < tolerance, "{} {}", kind.name(), p.name);
            }
        }
    }

    #[test]
    fn default_devices_pass_finite_audio() {
        let ctx = ProcessContext { sample_rate: 48000.0, bpm: 120.0, playing: true };
        for kind in [DeviceKind::Utility, DeviceKind::Eq, DeviceKind::Compressor, DeviceKind::Gate, DeviceKind::Saturator, DeviceKind::Delay, DeviceKind::Reverb, DeviceKind::Limiter, DeviceKind::Clap { path: "x".into(), plugin_id: "y".into(), state: vec![] }] {
            let mut d = create(&kind, 48000.0);
            let mut l: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
            let mut r = l.clone();
            d.process(&ctx, &mut l, &mut r);
            assert!(l.iter().chain(&r).all(|x| x.is_finite()), "{}", kind.name());
        }
    }

    #[test]
    fn neutral_eq_is_transparent() {
        let ctx = ProcessContext { sample_rate: 48000.0, bpm: 120.0, playing: true };
        let mut d = create(&DeviceKind::Eq, 48000.0);
        let src: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.03).sin()).collect();
        let (mut l, mut r) = (src.clone(), src.clone());
        d.process(&ctx, &mut l, &mut r);
        assert_eq!(l, src);
    }
}
