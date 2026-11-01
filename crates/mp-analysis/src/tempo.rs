//! Tempo and beat-grid detection from an onset-strength envelope.

use rustfft::{num_complex::Complex, FftPlanner};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoResult {
    pub bpm: f64,
    /// Time of the first detected downbeat, in seconds.
    pub offset: f64,
    /// Roughly 0..1: how clearly periodic the audio is.
    pub confidence: f32,
}

const HOP: usize = 512;
const FFT: usize = 1024;

/// Spectral flux: how much new energy appears in each frame. Log compression makes every onset
/// count about equally, which suits finding the tempo. Without it loud hits dominate, which suits
/// finding where the beat falls, because a kick outweighs the quieter hi-hats between beats.
fn onset_envelope(mono: &[f32], compress: bool) -> Vec<f32> {
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let window: Vec<f32> =
        (0..FFT).map(|i| (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2)).collect();
    let mut prev = vec![0f32; FFT / 2];
    let mut buf = vec![Complex::new(0f32, 0f32); FFT];
    let mut env = Vec::with_capacity(mono.len() / HOP);
    let mut at = 0;
    while at + FFT <= mono.len() {
        for i in 0..FFT {
            buf[i] = Complex::new(mono[at + i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        let mut flux = 0.0;
        for k in 0..FFT / 2 {
            let mag = if compress { (1.0 + 1000.0 * buf[k].norm()).ln() } else { buf[k].norm() };
            flux += (mag - prev[k]).max(0.0);
            prev[k] = mag;
        }
        env.push(flux);
        at += HOP;
    }
    // Remove the local mean so that only bursts of change count, not sustained loud passages.
    const AVG: usize = 16;
    let mut sum = 0f32;
    let mut out = vec![0f32; env.len()];
    for i in 0..env.len() {
        sum += env[i];
        if i >= AVG {
            sum -= env[i - AVG];
        }
        out[i] = (env[i] - sum / (i.min(AVG - 1) + 1) as f32).max(0.0);
    }
    out
}

fn comb_score(env: &[f32], period: f64, phase: f64) -> f32 {
    let mut total = 0.0;
    let mut t = phase;
    let mut hits = 0;
    while (t as usize) < env.len() {
        total += env[t as usize];
        hits += 1;
        t += period;
    }
    if hits == 0 { 0.0 } else { total / hits as f32 }
}

/// Best phase of a pulse train of period `period`, and the mean onset strength on it.
fn best_phase(env: &[f32], period: f64) -> (f64, f32) {
    let mut best = (0.0, 0.0f32);
    for p in 0..period.ceil() as usize {
        let score = comb_score(env, period, p as f64);
        if score > best.1 {
            best = (p as f64, score);
        }
    }
    best
}

/// Autocorrelation lags are coarse (about 10 ms), so this searches near a tempo for the sharpest
/// pulse train. Returns the refined tempo and its fit.
fn refine_bpm(env: &[f32], fps: f64, coarse: f64) -> (f64, f32) {
    let (mut bpm, mut top) = (coarse, f32::MIN);
    let mut candidate = coarse * 0.97;
    while candidate <= coarse * 1.03 {
        let score = best_phase(env, fps * 60.0 / candidate).1;
        if score > top {
            top = score;
            bpm = candidate;
        }
        candidate += 0.05;
    }
    (bpm, top)
}

/// Detects tempo over a mono signal. Returns `None` if the signal is too short or has no pulse.
pub fn detect(mono: &[f32], sample_rate: f32) -> Option<TempoResult> {
    let fps = sample_rate as f64 / HOP as f64;
    let env = onset_envelope(mono, true);
    if (env.len() as f64) < fps * 4.0 {
        return None;
    }
    let mean = env.iter().sum::<f32>() / env.len() as f32;
    if mean < 1e-4 {
        return None;
    }
    let centred: Vec<f32> = env.iter().map(|v| v - mean).collect();

    // Tempogram: autocorrelation weighted by a log-normal prior on tempo, centred on 120 BPM.
    // Pulses at a beat also repeat at its multiples, so harmonics are summed to favour the true beat.
    let min_lag = (fps * 60.0 / 200.0) as usize;
    let max_lag = (fps * 60.0 / 50.0) as usize;
    let autocorr = |lag: usize| -> f32 {
        centred.iter().zip(centred.iter().skip(lag)).map(|(a, b)| a * b).sum::<f32>()
            / (centred.len() - lag.min(centred.len() - 1)) as f32
    };
    let raw: Vec<f32> = (0..=max_lag * 4).map(|l| if l < centred.len() { autocorr(l) } else { 0.0 }).collect();
    let mut best = (min_lag, f32::MIN);
    for lag in min_lag..=max_lag {
        let bpm = fps * 60.0 / lag as f64;
        let octaves = (bpm / 120.0).log2();
        let prior = (-0.5 * (octaves / 0.9).powi(2)).exp() as f32;
        let harmonics = raw[lag] + 0.5 * raw[lag * 2] + 0.25 * raw[lag * 4];
        let score = harmonics * prior;
        if score > best.1 {
            best = (lag, score);
        }
    }
    let energy = raw[0].max(1e-9);
    let confidence = (raw[best.0] / energy).clamp(0.0, 1.0);
    if confidence < 0.02 {
        return None;
    }

    // Pulses a whole number of beats apart also look periodic, so the winner may be a slow
    // subdivision of the real beat. A faster tempo that lands on onsets just as often is the beat.
    let smoothed: Vec<f32> = (0..env.len())
        .map(|i| {
            let side = |j: Option<usize>| j.and_then(|j| env.get(j)).copied().unwrap_or(0.0);
            env[i] + 0.5 * (side(i.checked_sub(1)) + side(Some(i + 1)))
        })
        .collect();
    let env = smoothed;
    let refine = |coarse: f64| refine_bpm(&env, fps, coarse);
    let coarse_bpm = fps * 60.0 / best.0 as f64;
    let (mut bpm, mut top) = refine(coarse_bpm);
    const FASTEST_BEAT: f64 = 200.0;
    const SAME_FIT: f32 = 0.85;
    let slow_fit = top;
    for multiple in [1.5, 2.0, 3.0, 4.0] {
        let candidate = coarse_bpm * multiple;
        if candidate <= FASTEST_BEAT {
            let (fast, fit) = refine(candidate);
            if fit >= slow_fit * SAME_FIT && fast > bpm {
                (bpm, top) = (fast, fit);
            }
        }
    }

    // Produced music nearly always sits on a whole BPM, so prefer it when the evidence allows.
    let rounded = bpm.round();
    if (bpm - rounded).abs() < 0.3 && best_phase(&env, fps * 60.0 / rounded).1 >= top * 0.97 {
        bpm = rounded;
    }

    let period = fps * 60.0 / bpm;
    let loudness = onset_envelope(mono, false);
    let (phase, _) = best_phase(&loudness, period);
    // Of the four candidate beats in a bar, the strongest is taken as the downbeat.
    let downbeat = (0..4)
        .map(|b| phase + b as f64 * period)
        .max_by(|a, b| comb_score(&loudness, period * 4.0, *a).total_cmp(&comb_score(&loudness, period * 4.0, *b)))
        .unwrap_or(phase);
    // Flux peaks in the frame whose window first takes in the hit, which starts at about the hit.
    let offset = downbeat * HOP as f64 / sample_rate as f64;
    Some(TempoResult { bpm, offset, confidence })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    /// Clicks with a decaying noise body, the shape of a simple drum hit.
    fn click_track(bpm: f64, seconds: f64, first: f64) -> Vec<f32> {
        let n = (seconds * SR as f64) as usize;
        let mut x = vec![0f32; n];
        let mut seed = 3u32;
        let mut t = first;
        let mut beat = 0;
        while t < seconds {
            let at = (t * SR as f64) as usize;
            let amp = if beat % 4 == 0 { 1.0 } else { 0.5 };
            for i in 0..2400.min(n - at) {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                x[at + i] += noise * amp * (-(i as f32) / 400.0).exp();
            }
            t += 60.0 / bpm;
            beat += 1;
        }
        x
    }

    #[test]
    fn finds_common_tempos() {
        for bpm in [85.0, 100.0, 120.0, 128.0, 140.0, 174.0] {
            let r = detect(&click_track(bpm, 30.0, 0.0), SR).unwrap();
            // Half and double time are musically the same grid, but exact tempo must be right.
            let ratio = r.bpm / bpm;
            let close = (ratio - 1.0).abs() < 0.01 || (ratio - 2.0).abs() < 0.02 || (ratio - 0.5).abs() < 0.005;
            assert!(close, "true {bpm}, detected {}", r.bpm);
        }
    }

    #[test]
    fn typical_tempos_are_not_doubled_or_halved() {
        for bpm in [100.0, 120.0, 128.0] {
            let r = detect(&click_track(bpm, 30.0, 0.0), SR).unwrap();
            assert!((r.bpm - bpm).abs() < 0.5, "true {bpm}, detected {}", r.bpm);
        }
    }

    #[test]
    fn grid_offset_lands_on_the_downbeat() {
        let r = detect(&click_track(120.0, 30.0, 0.25), SR).unwrap();
        assert!((r.bpm - 120.0).abs() < 0.5);
        let bar = 2.0;
        let error = ((r.offset - 0.25).rem_euclid(bar)).min((0.25 - r.offset).rem_euclid(bar));
        assert!(error < 0.03, "offset {} is {error} s from a downbeat", r.offset);
    }

    #[test]
    fn beat_phase_follows_the_kick_not_the_offbeat_hats() {
        let bpm = 128.0;
        let mut x = click_track(bpm, 30.0, 0.0);
        let hats = click_track(bpm, 30.0, 60.0 / bpm / 2.0);
        for (a, h) in x.iter_mut().zip(&hats) {
            *a += h * 0.2;
        }
        let r = detect(&x, SR).unwrap();
        assert!((r.bpm - bpm).abs() < 0.5, "{}", r.bpm);
        let beat = 60.0 / bpm;
        let error = r.offset.rem_euclid(beat).min(beat - r.offset.rem_euclid(beat));
        assert!(error < 0.03, "offset {} is {error} s from a beat", r.offset);
    }

    #[test]
    fn silence_has_no_tempo() {
        assert!(detect(&vec![0.0; 480000], SR).is_none());
        assert!(detect(&vec![0.0; 100], SR).is_none());
    }
}
