//! Offline time-stretch and pitch-shift for clip renders.

use crate::{resample::resample, Stereo};
use signalsmith_stretch::Stretch;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Algorithm {
    /// Independent time and pitch.
    Elastic { semitones: f32, preserve_formants: bool },
    /// Tape-style resampling: pitch follows speed.
    Varispeed,
}

/// Renders `range` of `src` stretched by `ratio` (2.0 is twice as long).
///
/// Up to `CONTEXT_S` of the surrounding source is run through the stretcher and then cropped,
/// so a slice starts with the stretcher already settled, without a smeared or quiet first attack.
pub fn render(src: &Stereo, range: std::ops::Range<usize>, ratio: f64, algorithm: Algorithm, sample_rate: u32) -> Stereo {
    const CONTEXT_S: f64 = 0.1;
    let frames = src.frames();
    let start = range.start.min(frames);
    let end = range.end.min(frames);
    if start >= end {
        return Stereo::default();
    }
    let out_frames = ((end - start) as f64 * ratio).round() as usize;
    match algorithm {
        Algorithm::Varispeed => Stereo {
            l: fit(resample(&src.l[start..end], ratio), out_frames),
            r: fit(resample(&src.r[start..end], ratio), out_frames),
        },
        Algorithm::Elastic { semitones, preserve_formants } => {
            let context = (CONTEXT_S * sample_rate as f64) as usize;
            let (pad_start, pad_end) = (start.min(context), (frames - end).min(context));
            let (from, to) = (start - pad_start, end + pad_end);
            let input_frames = to - from;
            let mut interleaved = Vec::with_capacity(input_frames * 2);
            for i in from..to {
                interleaved.push(src.l[i]);
                interleaved.push(src.r[i]);
            }
            let total_out = (input_frames as f64 * ratio).round() as usize;
            let mut output = vec![0f32; total_out * 2];

            let mut stretch = Stretch::preset_default(2, sample_rate);
            stretch.set_transpose_factor_semitones(semitones, None);
            if preserve_formants && semitones != 0.0 {
                stretch.set_formant_factor(1.0, true);
            }
            stretch.exact(&interleaved, &mut output);

            let skip = (pad_start as f64 * ratio).round() as usize;
            let mut out = Stereo::silence(out_frames);
            for i in 0..out_frames {
                let j = (skip + i) * 2;
                if j + 1 < output.len() {
                    out.l[i] = output[j];
                    out.r[i] = output[j + 1];
                }
            }
            out
        }
    }
}

fn fit(mut v: Vec<f32>, frames: usize) -> Vec<f32> {
    v.resize(frames, 0.0);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::median_pitch;

    const SR: u32 = 48000;

    fn tone(hz: f32, secs: f32) -> Stereo {
        let n = (SR as f32 * secs) as usize;
        let l: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32 * 2.0 * std::f32::consts::PI;
                0.4 * (hz * t).sin() + 0.2 * (2.0 * hz * t).sin()
            })
            .collect();
        Stereo { r: l.clone(), l }
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn elastic_stretch_changes_length_but_not_pitch() {
        let src = tone(220.0, 1.0);
        let out = render(&src, 0..src.frames(), 2.0, Algorithm::Elastic { semitones: 0.0, preserve_formants: false }, SR);
        assert_eq!(out.frames(), 96000);
        let hz = median_pitch(&out.l, SR as f32).unwrap();
        assert!((hz - 220.0).abs() < 3.0, "{hz}");
        assert!(rms(&out.l[10000..80000]) > 0.5 * rms(&src.l));
    }

    #[test]
    fn elastic_pitch_shift_keeps_length() {
        let src = tone(220.0, 1.0);
        let out = render(&src, 0..src.frames(), 1.0, Algorithm::Elastic { semitones: 12.0, preserve_formants: false }, SR);
        assert_eq!(out.frames(), 48000);
        let hz = median_pitch(&out.l, SR as f32).unwrap();
        assert!((hz - 440.0).abs() < 6.0, "{hz}");
    }

    #[test]
    fn varispeed_links_pitch_and_time() {
        let src = tone(220.0, 1.0);
        let out = render(&src, 0..src.frames(), 2.0, Algorithm::Varispeed, SR);
        assert_eq!(out.frames(), 96000);
        let hz = median_pitch(&out.l, SR as f32).unwrap();
        assert!((hz - 110.0).abs() < 2.0, "{hz}");
    }

    #[test]
    fn slice_render_has_no_startup_dropout() {
        let src = tone(220.0, 2.0);
        let out = render(&src, 48000..60000, 1.5, Algorithm::Elastic { semitones: 0.0, preserve_formants: false }, SR);
        assert_eq!(out.frames(), 18000);
        assert!(rms(&out.l[..2400]) > 0.5 * rms(&src.l), "start level {}", rms(&out.l[..2400]));
    }
}
