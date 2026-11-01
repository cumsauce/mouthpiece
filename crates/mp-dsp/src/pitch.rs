//! YIN fundamental frequency estimation.

/// Median pitch of a mono signal in Hz, or `None` if it isn't clearly pitched.
pub fn median_pitch(x: &[f32], sample_rate: f32) -> Option<f32> {
    let window = (sample_rate * 0.03) as usize;
    if x.len() < window * 2 {
        return None;
    }
    // Evenly spaced samples of a long clip give the same median at a bounded cost.
    const MAX_FRAMES: usize = 40;
    let hop = (window / 2).max((x.len() - window * 2) / MAX_FRAMES).max(1);
    let mut pitches: Vec<f32> = (0..=(x.len() - window * 2) / hop)
        .filter_map(|i| yin(&x[i * hop..i * hop + window * 2], window, sample_rate))
        .collect();
    if pitches.len() < 2 {
        return None;
    }
    pitches.sort_by(f32::total_cmp);
    Some(pitches[pitches.len() / 2])
}

fn yin(x: &[f32], window: usize, sr: f32) -> Option<f32> {
    const THRESHOLD: f32 = 0.15;
    let energy: f32 = x[..window].iter().map(|v| v * v).sum::<f32>() / window as f32;
    if energy < 1e-6 {
        return None;
    }
    let min_tau = (sr / 1000.0) as usize;
    let max_tau = ((sr / 60.0) as usize).min(window - 1);
    let mut diff = vec![0f32; max_tau + 1];
    for (tau, d) in diff.iter_mut().enumerate().skip(1) {
        *d = (0..window).map(|i| (x[i] - x[i + tau]).powi(2)).sum();
    }
    // Cumulative mean normalised difference.
    let mut running = 0.0;
    for tau in 1..=max_tau {
        running += diff[tau];
        diff[tau] = if running > 0.0 { diff[tau] * tau as f32 / running } else { 1.0 };
    }
    let mut tau = min_tau;
    while tau < max_tau {
        if diff[tau] < THRESHOLD {
            while tau + 1 < max_tau && diff[tau + 1] < diff[tau] {
                tau += 1;
            }
            // Parabolic interpolation around the minimum for sub-sample accuracy.
            let (a, b, c) = (diff[tau - 1], diff[tau], diff[tau + 1]);
            let denom = 2.0 * (a - 2.0 * b + c);
            let refined = tau as f32 + if denom.abs() > 1e-9 { (a - c) / denom } else { 0.0 };
            return Some(sr / refined);
        }
        tau += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sine_and_harmonic_tones() {
        let sr = 48000.0;
        let tone: Vec<f32> = (0..24000)
            .map(|n| {
                let t = n as f32 / sr * 2.0 * std::f32::consts::PI;
                0.5 * (220.0 * t).sin() + 0.2 * (440.0 * t).sin() + 0.1 * (660.0 * t).sin()
            })
            .collect();
        let hz = median_pitch(&tone, sr).unwrap();
        assert!((hz - 220.0).abs() < 1.0, "{hz}");
    }

    #[test]
    fn noise_and_silence_are_not_pitched() {
        assert!(median_pitch(&vec![0.0; 24000], 48000.0).is_none());
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..24000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect();
        assert!(median_pitch(&noise, 48000.0).is_none());
    }
}
