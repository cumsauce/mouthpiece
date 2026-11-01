//! Musical key detection: a chroma profile compared with Krumhansl-Schmuckler key profiles.

use rustfft::{num_complex::Complex, FftPlanner};

const MAJOR: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// Returns (root, is_minor), with root 0 = C.
pub fn detect(mono: &[f32], sample_rate: f32) -> Option<(u8, bool)> {
    const FFT: usize = 16384;
    if mono.len() < FFT * 2 {
        return None;
    }
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let window: Vec<f32> =
        (0..FFT).map(|i| (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2)).collect();
    let mut chroma = [0f32; 12];
    let mut buf = vec![Complex::new(0f32, 0f32); FFT];
    let (min_bin, max_bin) = (
        (55.0 * FFT as f32 / sample_rate) as usize,
        ((2000.0 * FFT as f32 / sample_rate) as usize).min(FFT / 2),
    );
    for chunk in mono.chunks_exact(FFT).step_by(2) {
        for i in 0..FFT {
            buf[i] = Complex::new(chunk[i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (k, v) in buf.iter().enumerate().take(max_bin).skip(min_bin.max(1)) {
            let hz = k as f32 * sample_rate / FFT as f32;
            let pitch_class = (12.0 * (hz / 261.6256).log2()).round().rem_euclid(12.0) as usize % 12;
            chroma[pitch_class] += v.norm();
        }
    }
    if chroma.iter().sum::<f32>() < 1e-3 {
        return None;
    }
    let mut best = (0u8, false, f32::MIN);
    for root in 0..12 {
        for (profile, minor) in [(&MAJOR, false), (&MINOR, true)] {
            let score = correlation(&chroma, profile, root);
            if score > best.2 {
                best = (root as u8, minor, score);
            }
        }
    }
    Some((best.0, best.1))
}

fn correlation(chroma: &[f32; 12], profile: &[f32; 12], root: usize) -> f32 {
    let mean = |x: &[f32; 12]| x.iter().sum::<f32>() / 12.0;
    let (mc, mp) = (mean(chroma), mean(profile));
    let (mut num, mut dc, mut dp) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let c = chroma[(i + root) % 12] - mc;
        let p = profile[i] - mp;
        num += c * p;
        dc += c * c;
        dp += p * p;
    }
    num / (dc * dp).sqrt().max(1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_key_from_a_scale_workout() {
        let sr = 48000.0;
        // A minor triad and scale tones weighted by their usual importance in the key.
        let notes = [(220.0, 1.0), (261.63, 0.8), (329.63, 0.9), (293.66, 0.3), (392.0, 0.3), (349.23, 0.3), (246.94, 0.25)];
        let x: Vec<f32> = (0..(sr as usize * 8))
            .map(|i| {
                let t = i as f32 / sr * std::f32::consts::TAU;
                notes.iter().map(|(hz, a)| a * (hz * t).sin()).sum::<f32>() * 0.1
            })
            .collect();
        assert_eq!(detect(&x, sr), Some((9, true)));
    }

    #[test]
    fn silence_has_no_key() {
        assert_eq!(detect(&vec![0.0; 96000], 48000.0), None);
    }
}
