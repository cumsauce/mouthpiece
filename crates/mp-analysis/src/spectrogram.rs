//! Spectrogram images for the detail editor.

use rustfft::{num_complex::Complex, FftPlanner};

pub struct Spectrogram {
    pub columns: usize,
    pub rows: usize,
    /// Row-major with row 0 at the top (highest frequency). Values are 0..=255.
    pub data: Vec<u8>,
    pub seconds_per_column: f64,
}

/// Log-frequency magnitude spectrogram of a mono signal.
pub fn compute(mono: &[f32], sample_rate: f32) -> Spectrogram {
    const FFT: usize = 2048;
    const ROWS: usize = 96;
    const COLUMNS_PER_SECOND: f64 = 100.0;
    const FLOOR_DB: f32 = -80.0;
    let hop = (sample_rate as f64 / COLUMNS_PER_SECOND) as usize;
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let window: Vec<f32> =
        (0..FFT).map(|i| (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2)).collect();
    let (min_hz, max_hz) = (60f32, (sample_rate / 2.0).min(16000.0));
    let bin_for_row: Vec<(usize, usize)> = (0..ROWS)
        .map(|row| {
            let edge = |r: f32| min_hz * (max_hz / min_hz).powf(r / ROWS as f32) / sample_rate * FFT as f32;
            let lo = edge(row as f32) as usize;
            (lo, (edge(row as f32 + 1.0) as usize).max(lo + 1).min(FFT / 2))
        })
        .collect();
    let columns = mono.len() / hop.max(1);
    let mut data = vec![0u8; columns * ROWS];
    let mut buf = vec![Complex::new(0f32, 0f32); FFT];
    for c in 0..columns {
        let center = c * hop;
        let start = center as isize - (FFT / 2) as isize;
        for (i, b) in buf.iter_mut().enumerate() {
            let at = start + i as isize;
            let x = if at >= 0 { mono.get(at as usize).copied().unwrap_or(0.0) } else { 0.0 };
            *b = Complex::new(x * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (row, (lo, hi)) in bin_for_row.iter().enumerate() {
            let peak = buf[*lo..*hi].iter().map(|v| v.norm_sqr()).fold(0.0, f32::max);
            let db = 10.0 * (peak / (FFT * FFT / 16) as f32).max(1e-10).log10();
            let v = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
            data[(ROWS - 1 - row) * columns + c] = (v * 255.0) as u8;
        }
    }
    Spectrogram { columns, rows: ROWS, data, seconds_per_column: hop as f64 / sample_rate as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_lights_one_band_only() {
        let sr = 48000.0;
        let x: Vec<f32> = (0..48000).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / sr).sin()).collect();
        let s = compute(&x, sr);
        assert_eq!(s.rows, 96);
        let col = s.columns / 2;
        let brightest = (0..s.rows).max_by_key(|r| s.data[r * s.columns + col]).unwrap();
        let expected = ((1000f32 / 60.0).ln() / (16000f32 / 60.0).ln() * 96.0) as usize;
        assert!((95 - brightest).abs_diff(expected) <= 2, "row {brightest}, expected near {}", 95 - expected);
        assert!(s.data[col] < 80, "top row should be dark");
    }
}
