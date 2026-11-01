//! Effect kernels used by the engine's devices.

use crate::{db_to_gain, gain_to_db};

fn coeff(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}

/// Feed-forward stereo-linked compressor with a soft knee.
#[derive(Clone, Debug, Default)]
pub struct Compressor {
    env_db: f32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub makeup: f32,
    /// Gain reduction of the last sample, in dB, for metering.
    pub reduction_db: f32,
}

impl Compressor {
    pub fn set(&mut self, threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, makeup_db: f32, sr: f32) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
        self.attack = coeff(attack_ms, sr);
        self.release = coeff(release_ms, sr);
        self.makeup = db_to_gain(makeup_db);
    }

    pub fn reset(&mut self) {
        self.env_db = 0.0;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        const KNEE: f32 = 6.0;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let over = gain_to_db(a.abs().max(b.abs())) - self.threshold_db;
            let slope = 1.0 - 1.0 / self.ratio;
            let target = if over <= -KNEE / 2.0 {
                0.0
            } else if over >= KNEE / 2.0 {
                over * slope
            } else {
                slope * (over + KNEE / 2.0).powi(2) / (2.0 * KNEE)
            };
            let c = if target > self.env_db { self.attack } else { self.release };
            self.env_db = target + (self.env_db - target) * c;
            let g = db_to_gain(-self.env_db) * self.makeup;
            *a *= g;
            *b *= g;
        }
        self.reduction_db = self.env_db;
    }
}

/// Noise gate with hold and hysteresis, for cleaning spill between words.
#[derive(Clone, Debug, Default)]
pub struct Gate {
    gain: f32,
    hold_left: f32,
    open: bool,
    threshold: f32,
    close_threshold: f32,
    floor: f32,
    attack: f32,
    release: f32,
    hold: f32,
}

impl Gate {
    pub fn set(&mut self, threshold_db: f32, floor_db: f32, attack_ms: f32, hold_ms: f32, release_ms: f32, sr: f32) {
        self.threshold = db_to_gain(threshold_db);
        self.close_threshold = db_to_gain(threshold_db - 4.0);
        self.floor = db_to_gain(floor_db);
        self.attack = coeff(attack_ms, sr);
        self.release = coeff(release_ms, sr);
        self.hold = hold_ms * 0.001 * sr;
    }

    pub fn reset(&mut self) {
        self.gain = 0.0;
        self.open = false;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let level = a.abs().max(b.abs());
            if level > self.threshold {
                self.open = true;
                self.hold_left = self.hold;
            } else if level < self.close_threshold {
                if self.hold_left > 0.0 {
                    self.hold_left -= 1.0;
                } else {
                    self.open = false;
                }
            }
            let (target, c) = if self.open { (1.0, self.attack) } else { (self.floor, self.release) };
            self.gain = target + (self.gain - target) * c;
            *a *= self.gain;
            *b *= self.gain;
        }
    }
}

/// Soft-clipping saturation with a mix control.
#[inline]
pub fn saturate(x: f32, drive: f32) -> f32 {
    (x * drive).tanh() / drive.tanh().max(1e-3)
}

/// Stereo delay with cross-feedback and a damping filter in the loop.
#[derive(Clone, Debug, Default)]
pub struct Delay {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    pos: usize,
    lp_l: f32,
    lp_r: f32,
}

impl Delay {
    pub fn allocate(&mut self, max_seconds: f32, sr: f32) {
        let n = (max_seconds * sr) as usize + 1;
        self.buf_l = vec![0.0; n];
        self.buf_r = vec![0.0; n];
        self.pos = 0;
    }

    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.lp_l = 0.0;
        self.lp_r = 0.0;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], delay_frames: usize, feedback: f32, ping_pong: f32, damp: f32, mix: f32) {
        let n = self.buf_l.len();
        if n == 0 {
            return;
        }
        let delay = delay_frames.clamp(1, n - 1);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let read = (self.pos + n - delay) % n;
            let (dl, dr) = (self.buf_l[read], self.buf_r[read]);
            self.lp_l += (dl - self.lp_l) * (1.0 - damp);
            self.lp_r += (dr - self.lp_r) * (1.0 - damp);
            let (fl, fr) = (self.lp_l * feedback, self.lp_r * feedback);
            self.buf_l[self.pos] = *a + fl * (1.0 - ping_pong) + fr * ping_pong;
            self.buf_r[self.pos] = *b + fr * (1.0 - ping_pong) + fl * ping_pong;
            self.pos = (self.pos + 1) % n;
            *a += dl * mix;
            *b += dr * mix;
        }
    }
}

const FDN_LINES: usize = 8;
const FDN_PRIMES: [usize; FDN_LINES] = [1423, 1777, 2089, 2341, 2729, 3163, 3517, 3943];

/// 8-line feedback delay network reverb with a Householder matrix and damping.
#[derive(Clone, Debug, Default)]
pub struct Reverb {
    lines: Vec<Vec<f32>>,
    pos: [usize; FDN_LINES],
    damp_state: [f32; FDN_LINES],
    predelay_l: Vec<f32>,
    predelay_r: Vec<f32>,
    pre_pos: usize,
}

impl Reverb {
    pub fn allocate(&mut self, sr: f32) {
        let scale = sr / 48000.0 * 1.6;
        self.lines = FDN_PRIMES.iter().map(|p| vec![0.0; (*p as f32 * scale) as usize + 1]).collect();
        self.pos = [0; FDN_LINES];
        self.damp_state = [0.0; FDN_LINES];
        let pre = (0.25 * sr) as usize + 1;
        self.predelay_l = vec![0.0; pre];
        self.predelay_r = vec![0.0; pre];
        self.pre_pos = 0;
    }

    pub fn reset(&mut self) {
        self.lines.iter_mut().for_each(|l| l.fill(0.0));
        self.damp_state = [0.0; FDN_LINES];
        self.predelay_l.fill(0.0);
        self.predelay_r.fill(0.0);
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], decay_s: f32, damp: f32, predelay_frames: usize, mix: f32, sr: f32) {
        if self.lines.is_empty() {
            return;
        }
        let pre_n = self.predelay_l.len();
        let pre = predelay_frames.min(pre_n - 1);
        // Per-line feedback gain that gives the requested RT60.
        let mut fb = [0f32; FDN_LINES];
        for (i, f) in fb.iter_mut().enumerate() {
            *f = 10f32.powf(-3.0 * self.lines[i].len() as f32 / (sr * decay_s.max(0.05)));
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let read = (self.pre_pos + pre_n - pre) % pre_n;
            let (in_l, in_r) = (self.predelay_l[read], self.predelay_r[read]);
            self.predelay_l[self.pre_pos] = *a;
            self.predelay_r[self.pre_pos] = *b;
            self.pre_pos = (self.pre_pos + 1) % pre_n;

            let mut out = [0f32; FDN_LINES];
            let mut sum = 0.0;
            for i in 0..FDN_LINES {
                out[i] = self.lines[i][self.pos[i]];
                sum += out[i];
            }
            // Householder reflection: energy preserving, with maximum mixing between lines.
            let reflect = sum * 2.0 / FDN_LINES as f32;
            for i in 0..FDN_LINES {
                self.damp_state[i] += (out[i] - self.damp_state[i]) * (1.0 - damp);
                let input = if i % 2 == 0 { in_l } else { in_r };
                let line_len = self.lines[i].len();
                self.lines[i][self.pos[i]] = input * 0.35 + (self.damp_state[i] - reflect) * fb[i];
                self.pos[i] = (self.pos[i] + 1) % line_len;
            }
            let wet_l = out[0] + out[2] + out[4] + out[6];
            let wet_r = out[1] + out[3] + out[5] + out[7];
            *a += wet_l * 0.25 * mix;
            *b += wet_r * 0.25 * mix;
        }
    }
}

/// Look-ahead brickwall limiter. Output never exceeds the ceiling.
#[derive(Clone, Debug, Default)]
pub struct Limiter {
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    peak_window: Vec<f32>,
    pos: usize,
    gain: f32,
    release: f32,
    attack: f32,
    /// Gain reduction of the last sample in dB, for metering.
    pub reduction_db: f32,
}

impl Limiter {
    pub fn allocate(&mut self, sr: f32) {
        let n = ((0.002 * sr) as usize).max(8);
        self.delay_l = vec![0.0; n];
        self.delay_r = vec![0.0; n];
        self.peak_window = vec![0.0; n];
        self.pos = 0;
        self.gain = 1.0;
        self.release = coeff(120.0, sr);
        // Reaches a required reduction within the look-ahead window.
        self.attack = (-5.0 / n as f32).exp();
    }

    pub fn latency(&self) -> usize {
        self.delay_l.len()
    }

    pub fn reset(&mut self) {
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.peak_window.fill(0.0);
        self.gain = 1.0;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ceiling: f32, input_gain: f32) {
        let n = self.delay_l.len();
        if n == 0 {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (x, y) = (*a * input_gain, *b * input_gain);
            self.peak_window[self.pos] = x.abs().max(y.abs());
            // Window is short (about 96 samples), so a direct scan is cheap and exact.
            let peak = self.peak_window.iter().copied().fold(0.0, f32::max);
            let target = if peak > ceiling { ceiling / peak } else { 1.0 };
            let c = if target < self.gain { self.attack } else { self.release };
            self.gain = target + (self.gain - target) * c;
            let out_l = self.delay_l[self.pos];
            let out_r = self.delay_r[self.pos];
            self.delay_l[self.pos] = x;
            self.delay_r[self.pos] = y;
            self.pos = (self.pos + 1) % n;
            // The clamp catches the residual of the exponential attack, guaranteeing the ceiling.
            *a = (out_l * self.gain).clamp(-ceiling, ceiling);
            *b = (out_r * self.gain).clamp(-ceiling, ceiling);
        }
        self.reduction_db = -gain_to_db(self.gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn sine(amp: f32, frames: usize) -> Vec<f32> {
        (0..frames).map(|n| amp * (n as f32 * 0.05).sin()).collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut lim = Limiter::default();
        lim.allocate(SR);
        let mut l = sine(4.0, 48000);
        let mut r = sine(4.0, 48000);
        lim.process(&mut l, &mut r, 0.9, 1.0);
        assert!(peak(&l) <= 0.9 + 1e-5);
        assert!(peak(&r) <= 0.9 + 1e-5);
    }

    #[test]
    fn limiter_leaves_quiet_audio_alone() {
        let mut lim = Limiter::default();
        lim.allocate(SR);
        let input = sine(0.3, 4800);
        let (mut l, mut r) = (input.clone(), input.clone());
        lim.process(&mut l, &mut r, 0.9, 1.0);
        let d = lim.latency();
        for i in d..4800 {
            assert!((l[i] - input[i - d]).abs() < 1e-4);
        }
    }

    #[test]
    fn compressor_reduces_loud_signal() {
        let mut c = Compressor::default();
        c.set(-20.0, 4.0, 5.0, 50.0, 0.0, SR);
        let (mut l, mut r) = (sine(1.0, 48000), sine(1.0, 48000));
        c.process(&mut l, &mut r);
        let tail = peak(&l[24000..]);
        assert!(tail < 0.4, "tail peak {tail}");
        assert!(c.reduction_db > 10.0);
    }

    #[test]
    fn gate_silences_quiet_passages() {
        let mut g = Gate::default();
        g.set(-30.0, -80.0, 1.0, 10.0, 20.0, SR);
        let (mut l, mut r) = (sine(0.001, 48000), sine(0.001, 48000));
        g.process(&mut l, &mut r);
        assert!(peak(&l[24000..]) < 1e-5);
    }

    #[test]
    fn delay_echoes_after_delay_time() {
        let mut d = Delay::default();
        d.allocate(1.0, SR);
        let mut l = vec![0.0; 2000];
        let mut r = vec![0.0; 2000];
        l[0] = 1.0;
        d.process(&mut l, &mut r, 1000, 0.0, 0.0, 0.0, 1.0);
        assert!((l[1000] - 1.0).abs() < 1e-5);
        assert_eq!(l[999], 0.0);
    }

    #[test]
    fn reverb_is_stable_and_has_a_tail() {
        let mut rv = Reverb::default();
        rv.allocate(SR);
        let mut l = vec![0.0; 192000];
        let mut r = vec![0.0; 192000];
        l[0] = 1.0;
        r[0] = 1.0;
        rv.process(&mut l, &mut r, 1.5, 0.3, 0, 1.0, SR);
        let early = peak(&l[4800..24000]);
        let late = peak(&l[144000..]);
        assert!(early > 0.005, "needs a tail, got {early}");
        assert!(late < early * 0.05, "must decay: early {early} late {late}");
        assert!(l.iter().all(|x| x.is_finite()));
    }
}
