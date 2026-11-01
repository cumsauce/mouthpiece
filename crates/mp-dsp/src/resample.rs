//! Band-limited resampling with a Kaiser-windowed sinc table.

use std::f64::consts::PI;
use std::sync::OnceLock;

const ZERO_CROSSINGS: usize = 24;
const TABLE_RES: usize = 512;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..32 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

/// One side of the windowed sinc, sampled at `TABLE_RES` points per zero crossing.
fn table() -> &'static [f64] {
    static TABLE: OnceLock<Vec<f64>> = OnceLock::new();
    TABLE.get_or_init(|| {
        const BETA: f64 = 12.0;
        let n = ZERO_CROSSINGS * TABLE_RES + 2;
        (0..n)
            .map(|i| {
                let x = i as f64 / TABLE_RES as f64;
                let r = x / ZERO_CROSSINGS as f64;
                let window = if r < 1.0 { bessel_i0(BETA * (1.0 - r * r).sqrt()) / bessel_i0(BETA) } else { 0.0 };
                let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
                sinc * window
            })
            .collect()
    })
}

/// Resamples so that the output is `ratio` times as long as the input.
pub fn resample(input: &[f32], ratio: f64) -> Vec<f32> {
    if (ratio - 1.0).abs() < 1e-9 {
        return input.to_vec();
    }
    let t = table();
    let out_len = (input.len() as f64 * ratio).round() as usize;
    // When downsampling, widen the kernel so the cutoff follows the new Nyquist frequency.
    let cutoff = ratio.min(1.0);
    let half_width = ZERO_CROSSINGS as f64 / cutoff;
    (0..out_len)
        .map(|n| {
            let pos = n as f64 / ratio;
            let first = (pos - half_width).ceil().max(0.0) as usize;
            let last = ((pos + half_width).floor() as usize).min(input.len().saturating_sub(1));
            let mut sum = 0.0;
            for (j, x) in input.iter().enumerate().take(last + 1).skip(first) {
                let d = (pos - j as f64).abs() * cutoff * TABLE_RES as f64;
                let i = d as usize;
                if i + 1 < t.len() {
                    let frac = d - i as f64;
                    sum += *x as f64 * (t[i] + (t[i + 1] - t[i]) * frac);
                }
            }
            (sum * cutoff) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, sr: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * hz * i as f64 / sr).sin() as f32).collect()
    }

    #[test]
    fn upsampling_preserves_a_sine() {
        let out = resample(&sine(1000.0, 44100.0, 44100), 48000.0 / 44100.0);
        let expected = sine(1000.0, 48000.0, out.len());
        assert_eq!(out.len(), 48000);
        for i in 1000..47000 {
            assert!((out[i] - expected[i]).abs() < 1e-3, "at {i}");
        }
    }

    #[test]
    fn downsampling_rejects_frequencies_above_new_nyquist() {
        let out = resample(&sine(18000.0, 48000.0, 48000), 0.5);
        let peak = out[1000..23000].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.01, "aliasing peak {peak}");
    }
}
