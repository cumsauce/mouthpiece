//! RBJ biquad filters.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    HighPass,
    LowPass,
    Peak,
    LowShelf,
    HighShelf,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn set(&mut self, kind: FilterKind, freq: f32, q: f32, gain_db: f32, sample_rate: f32) {
        let (b0, b1, b2, a1, a2) = coefficients(kind, freq, q, gain_db, sample_rate);
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

fn coefficients(kind: FilterKind, freq: f32, q: f32, gain_db: f32, sr: f32) -> (f32, f32, f32, f32, f32) {
    let w = 2.0 * PI * (freq / sr).clamp(1e-4, 0.49);
    let (sin, cos) = w.sin_cos();
    let alpha = sin / (2.0 * q.max(0.05));
    let a = 10f32.powf(gain_db / 40.0);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        FilterKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        FilterKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        FilterKind::Peak => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
        FilterKind::LowShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cos + s),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - s),
                (a + 1.0) + (a - 1.0) * cos + s,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - s,
            )
        }
        FilterKind::HighShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cos + s),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - s),
                (a + 1.0) - (a - 1.0) * cos + s,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - s,
            )
        }
    };
    (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// Magnitude response in dB of a filter design, used to draw EQ curves.
pub fn response_db(kind: FilterKind, freq: f32, q: f32, gain_db: f32, sr: f32, at_hz: f32) -> f32 {
    let (b0, b1, b2, a1, a2) = coefficients(kind, freq, q, gain_db, sr);
    let w = 2.0 * PI * at_hz / sr;
    let (c1, s1) = (w.cos(), w.sin());
    let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
    let num = (b0 + b1 * c1 + b2 * c2).powi(2) + (b1 * s1 + b2 * s2).powi(2);
    let den = (1.0 + a1 * c1 + a2 * c2).powi(2) + (a1 * s1 + a2 * s2).powi(2);
    10.0 * (num / den).max(1e-12).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_at(kind: FilterKind, freq: f32, gain_db: f32, test_hz: f32) -> f32 {
        let sr = 48000.0;
        let mut f = Biquad::default();
        f.set(kind, freq, 0.707, gain_db, sr);
        let mut peak = 0f32;
        for n in 0..48000 {
            let y = f.process((2.0 * PI * test_hz * n as f32 / sr).sin());
            if n > 24000 {
                peak = peak.max(y.abs());
            }
        }
        20.0 * peak.log10()
    }

    #[test]
    fn highpass_cuts_lows_and_passes_highs() {
        assert!(gain_at(FilterKind::HighPass, 1000.0, 0.0, 100.0) < -30.0);
        assert!(gain_at(FilterKind::HighPass, 1000.0, 0.0, 7919.0).abs() < 0.5);
    }

    #[test]
    fn peak_boosts_by_requested_gain() {
        assert!((gain_at(FilterKind::Peak, 1000.0, 6.0, 1000.0) - 6.0).abs() < 0.3);
    }

    #[test]
    fn computed_response_matches_measured() {
        let measured = gain_at(FilterKind::HighShelf, 4000.0, 6.0, 12000.0);
        let computed = response_db(FilterKind::HighShelf, 4000.0, 0.707, 6.0, 48000.0, 12000.0);
        assert!((measured - computed).abs() < 0.3);
    }
}
