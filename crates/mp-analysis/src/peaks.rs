//! Multi-resolution waveform data for fast drawing at any zoom.

const BASE_BIN: usize = 64;
const LEVEL_FACTOR: usize = 4;

/// One waveform column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bin {
    pub min: f32,
    pub max: f32,
    /// 0 is dull, 1 is bright: how much high-frequency content there is. Used to tint waveforms.
    pub brightness: f32,
}

struct Level {
    frames_per_bin: usize,
    bins: Vec<Bin>,
}

pub struct Peaks {
    levels: Vec<Level>,
    pub frames: usize,
}

impl Peaks {
    pub fn build(mono: &[f32]) -> Self {
        let base: Vec<Bin> = mono
            .chunks(BASE_BIN)
            .map(|chunk| {
                let (mut lo, mut hi, mut total, mut diff) = (f32::MAX, f32::MIN, 0.0, 0.0);
                let mut prev = chunk[0];
                for &x in chunk {
                    lo = lo.min(x);
                    hi = hi.max(x);
                    total += x.abs();
                    diff += (x - prev).abs();
                    prev = x;
                }
                Bin { min: lo, max: hi, brightness: (diff / total.max(1e-6)).min(1.0) }
            })
            .collect();
        let mut levels = vec![Level { frames_per_bin: BASE_BIN, bins: base }];
        while levels.last().is_some_and(|l| l.bins.len() > 64) {
            let prev = levels.last().expect("levels is never empty");
            let bins = prev.bins.chunks(LEVEL_FACTOR).map(merge).collect();
            let frames_per_bin = prev.frames_per_bin * LEVEL_FACTOR;
            levels.push(Level { frames_per_bin, bins });
        }
        Self { levels, frames: mono.len() }
    }

    /// Summary of frames `a..b`, from the coarsest level that still resolves that span.
    pub fn query(&self, a: f64, b: f64) -> Bin {
        let span = (b - a).abs().max(1.0);
        let level = self
            .levels
            .iter()
            .rev()
            .find(|l| (l.frames_per_bin as f64) <= span)
            .unwrap_or(&self.levels[0]);
        let n = level.frames_per_bin as f64;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let first = (lo / n).floor().max(0.0) as usize;
        let last = ((hi / n).ceil() as usize).max(first + 1).min(level.bins.len());
        if first >= last {
            return Bin::default();
        }
        merge(&level.bins[first..last])
    }
}

fn merge(bins: &[Bin]) -> Bin {
    let (mut lo, mut hi, mut bright, mut weight) = (f32::MAX, f32::MIN, 0.0, 0.0);
    for b in bins {
        lo = lo.min(b.min);
        hi = hi.max(b.max);
        // Weighted by level, so silence doesn't dull the colour of loud neighbours.
        let w = (b.max - b.min).max(1e-5);
        bright += b.brightness * w;
        weight += w;
    }
    Bin { min: lo, max: hi, brightness: bright / weight }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_finds_a_spike_at_any_zoom() {
        let mut x = vec![0f32; 100_000];
        x[50_000] = 1.0;
        let p = Peaks::build(&x);
        assert_eq!(p.query(49_990.0, 50_010.0).max, 1.0);
        assert_eq!(p.query(0.0, 100_000.0).max, 1.0);
        assert_eq!(p.query(0.0, 10_000.0).max, 0.0);
        assert_eq!(p.query(200_000.0, 200_100.0).max, 0.0);
    }

    #[test]
    fn brightness_separates_noise_from_a_low_tone() {
        let low: Vec<f32> = (0..64_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..64_000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect();
        let (a, b) = (Peaks::build(&low).query(0.0, 64_000.0), Peaks::build(&noise).query(0.0, 64_000.0));
        assert!(a.brightness < 0.1 && b.brightness > 0.5, "{} {}", a.brightness, b.brightness);
    }
}
