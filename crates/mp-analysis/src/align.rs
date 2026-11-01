//! Refines word boundaries against the audio, and aligns phonemes inside each word.
//!
//! The aligner is a dynamic-programming search over how to divide a phrase's frames among its known
//! phonemes. Each phoneme class has a hand-built expectation of how it sounds (vowels are loud and
//! tonal, fricatives are noisy, stops have a closure and a burst). It's approximate compared with a
//! trained acoustic model, but it runs quickly, needs no extra model, and places boundaries well
//! enough to cut words apart and to find and audition sounds.

use crate::g2p::{class_of, PhonemeClass};
use crate::Phoneme;

const HOP_S: f64 = 0.005;
const WINDOW_S: f64 = 0.02;
const MIN_FRAMES: usize = 2;

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    /// Level in dB relative to the word's loudest frame, scaled to 0 (silent) .. 1 (loudest).
    level: f32,
    /// Zero-crossing rate in 0..=1. High for noise, low for voiced sound.
    zcr: f32,
}

fn features(x: &[f32], sr: f32, hop_s: f64) -> Vec<Frame> {
    let hop = ((hop_s * sr as f64) as usize).max(1);
    let win = ((WINDOW_S * sr as f64) as usize).max(2);
    if x.len() < win {
        return Vec::new();
    }
    let mut frames: Vec<(f32, f32)> = Vec::with_capacity(x.len() / hop);
    let mut at = 0;
    while at + win <= x.len() {
        let w = &x[at..at + win];
        let energy = w.iter().map(|v| v * v).sum::<f32>() / win as f32;
        let crossings = w.windows(2).filter(|p| (p[0] >= 0.0) != (p[1] >= 0.0)).count();
        frames.push((10.0 * energy.max(1e-10).log10(), crossings as f32 / win as f32));
        at += hop;
    }
    let top = frames.iter().map(|f| f.0).fold(f32::MIN, f32::max);
    const RANGE_DB: f32 = 40.0;
    frames
        .into_iter()
        .map(|(db, zcr)| Frame { level: ((db - top + RANGE_DB) / RANGE_DB).clamp(0.0, 1.0), zcr: (zcr * 4.0).min(1.0) })
        .collect()
}

/// How well a frame fits a class, as a log-likelihood-like score.
fn emission(class: PhonemeClass, f: Frame) -> f32 {
    use PhonemeClass::*;
    let gauss = |x: f32, mean: f32, sd: f32| -((x - mean) / sd).powi(2) * 0.5;
    match class {
        Vowel => gauss(f.level, 0.9, 0.2) + gauss(f.zcr, 0.15, 0.2),
        Glide => gauss(f.level, 0.75, 0.2) + gauss(f.zcr, 0.15, 0.2),
        Nasal => gauss(f.level, 0.65, 0.2) + gauss(f.zcr, 0.08, 0.15),
        VoicedFricative => gauss(f.level, 0.55, 0.25) + gauss(f.zcr, 0.4, 0.25),
        UnvoicedFricative => gauss(f.level, 0.45, 0.3) + gauss(f.zcr, 0.75, 0.25),
        VoicedStop => gauss(f.level, 0.35, 0.3) + gauss(f.zcr, 0.15, 0.3),
        UnvoicedStop => gauss(f.level, 0.2, 0.3) + gauss(f.zcr, 0.4, 0.4),
        Affricate => gauss(f.level, 0.45, 0.3) + gauss(f.zcr, 0.6, 0.3),
    }
}

fn silence_emission(f: Frame) -> f32 {
    -((f.level - 0.02) / 0.12).powi(2) * 0.5
}

/// Typical duration in seconds, used as a prior so that no phoneme collapses or swallows a word.
fn typical_duration(class: PhonemeClass) -> f64 {
    use PhonemeClass::*;
    match class {
        Vowel => 0.11,
        Glide => 0.06,
        Nasal => 0.07,
        VoicedFricative => 0.07,
        UnvoicedFricative => 0.1,
        VoicedStop => 0.06,
        UnvoicedStop => 0.08,
        Affricate => 0.1,
    }
}

/// A word to place, with where recognition thinks it is.
#[derive(Clone, Debug)]
pub struct PhraseWord {
    pub symbols: Vec<String>,
    /// A time somewhere near the middle of the word, from recognition.
    pub anchor: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlignedWord {
    pub start: f64,
    pub end: f64,
    pub phonemes: Vec<Phoneme>,
}

struct State {
    /// `None` is optional silence between words.
    class: Option<PhonemeClass>,
    word: usize,
    first_of_word: bool,
    expected: f64,
    max_frames: usize,
}

/// Longest time a sound can be held. Singers hold vowels far longer than speakers do.
fn max_seconds(class: PhonemeClass) -> f64 {
    use PhonemeClass::*;
    match class {
        Vowel => 4.0,
        Nasal | Glide | VoicedFricative | UnvoicedFricative => 1.5,
        VoicedStop | UnvoicedStop | Affricate => 0.4,
    }
}

/// Aligns a phrase's words and phonemes over its audio in one search, with optional silence between
/// words. `x` starts at time `phrase_start`. Sound classes pull boundaries onto real acoustic
/// changes, while durations and recognition's anchors keep the search from wandering.
pub fn align_phrase(x: &[f32], sr: f32, phrase_start: f64, words: &[PhraseWord]) -> Vec<AlignedWord> {
    const ANCHOR_SD: f64 = 0.2;
    const ANCHOR_WEIGHT: f64 = 6.0;
    // Long phrases use a coarser frame rate, which keeps the search quick.
    let duration = x.len() as f64 / sr as f64;
    let hop_s = (duration / 4000.0).max(HOP_S);
    let frames = features(x, sr, hop_s);
    let needed: usize = words.iter().map(|w| w.symbols.len()).sum::<usize>() * MIN_FRAMES;
    if frames.len() < needed.max(1) {
        return proportional_words(words, phrase_start, phrase_start + duration);
    }
    let t = frames.len();

    // Prefix sums of each class's fit give any segment's score in constant time.
    const SILENCE: usize = PhonemeClass::ALL.len();
    let emission_sum: Vec<Vec<f32>> = PhonemeClass::ALL
        .iter()
        .map(|c| Some(*c))
        .chain([None])
        .map(|class| {
            let mut sum = vec![0f32; t + 1];
            for i in 0..t {
                let e = class.map_or_else(|| silence_emission(frames[i]), |c| emission(c, frames[i]));
                sum[i + 1] = sum[i] + e;
            }
            sum
        })
        .collect();

    // Scale expected durations to how fast this phrase is spoken or sung.
    let sounding = frames.iter().filter(|f| f.level > 0.2).count() as f64 * hop_s;
    let typical: f64 = words.iter().flat_map(|w| &w.symbols).map(|s| typical_duration(class_of(s))).sum();
    let pace = (sounding / typical.max(0.01)).clamp(0.4, 12.0);

    let mut states = vec![State { class: None, word: usize::MAX, first_of_word: false, expected: 0.0, max_frames: t }];
    let mut word_lengths = Vec::with_capacity(words.len());
    for (w, word) in words.iter().enumerate() {
        word_lengths.push(word.symbols.iter().map(|s| typical_duration(class_of(s)) * pace).sum::<f64>());
        for (i, symbol) in word.symbols.iter().enumerate() {
            let class = class_of(symbol);
            states.push(State {
                class: Some(class),
                word: w,
                first_of_word: i == 0,
                expected: (typical_duration(class) * pace / hop_s).max(2.0),
                max_frames: (max_seconds(class) / hop_s) as usize,
            });
        }
        states.push(State { class: None, word: w, first_of_word: false, expected: 0.0, max_frames: t });
    }

    let mut previous = vec![f32::NEG_INFINITY; t + 1];
    previous[0] = 0.0;
    let mut back = vec![vec![0u32; t + 1]; states.len()];
    let class_index = |c: Option<PhonemeClass>| c.map_or(SILENCE, |c| PhonemeClass::ALL.iter().position(|x| *x == c).unwrap_or(0));
    for (s, state) in states.iter().enumerate() {
        let mut current = vec![f32::NEG_INFINITY; t + 1];
        let sum = &emission_sum[class_index(state.class)];
        let optional = state.class.is_none();
        let min_frames = if optional { 1 } else { MIN_FRAMES };
        let anchor = (state.first_of_word).then(|| {
            let word = state.word;
            words[word].anchor - word_lengths[word] * 0.5
        });
        for end in 0..=t {
            if optional && previous[end] > current[end] {
                current[end] = previous[end];
                back[s][end] = end as u32;
            }
            let longest = state.max_frames.min(end);
            for d in min_frames..=longest {
                let start = end - d;
                if previous[start] == f32::NEG_INFINITY {
                    continue;
                }
                let mut score = previous[start] + (sum[end] - sum[start]);
                if !optional {
                    let r = (d as f64 / state.expected).ln();
                    score -= (r * r * 3.0) as f32;
                }
                if let Some(a) = anchor {
                    let at = phrase_start + start as f64 * hop_s;
                    let z = (at - a) / ANCHOR_SD;
                    score -= (ANCHOR_WEIGHT * z * z * 0.5) as f32;
                }
                if score > current[end] {
                    current[end] = score;
                    back[s][end] = start as u32;
                }
            }
        }
        previous = current;
    }
    if previous[t] == f32::NEG_INFINITY {
        return proportional_words(words, phrase_start, phrase_start + duration);
    }

    // Walk back through the best path to find where each state starts and ends.
    let mut spans = vec![(0usize, 0usize); states.len()];
    let mut end = t;
    for s in (0..states.len()).rev() {
        let start = back[s][end] as usize;
        spans[s] = (start, end);
        end = start;
    }

    // A frame describes the audio around its centre, half a window after its start.
    let time_of = |frame: usize| phrase_start + frame as f64 * hop_s + (WINDOW_S - hop_s) * 0.5;
    let mut out: Vec<AlignedWord> = Vec::with_capacity(words.len());
    let mut phonemes_left = words.iter().flat_map(|w| w.symbols.iter());
    for (s, state) in states.iter().enumerate() {
        if state.class.is_none() {
            continue;
        }
        let (a, b) = spans[s];
        let phoneme = Phoneme {
            symbol: phonemes_left.next().cloned().unwrap_or_default(),
            start: time_of(a).max(phrase_start),
            end: time_of(b),
        };
        if state.first_of_word {
            out.push(AlignedWord { start: phoneme.start, end: phoneme.end, phonemes: vec![phoneme] });
        } else if let Some(word) = out.last_mut() {
            word.end = phoneme.end;
            word.phonemes.push(phoneme);
        }
    }
    out
}

fn proportional_words(words: &[PhraseWord], start: f64, end: f64) -> Vec<AlignedWord> {
    let weight = |w: &PhraseWord| w.symbols.iter().map(|s| typical_duration(class_of(s))).sum::<f64>();
    let total: f64 = words.iter().map(weight).sum::<f64>().max(1e-6);
    let mut t = start;
    words
        .iter()
        .map(|w| {
            let share = (end - start) * weight(w) / total;
            let word_total = weight(w).max(1e-6);
            let mut p = t;
            let phonemes = w
                .symbols
                .iter()
                .map(|s| {
                    let d = share * typical_duration(class_of(s)) / word_total;
                    let phoneme = Phoneme { symbol: s.clone(), start: p, end: p + d };
                    p += d;
                    phoneme
                })
                .collect();
            let word = AlignedWord { start: t, end: t + share, phonemes };
            t += share;
            word
        })
        .collect()
}

/// Finds the quietest moment near `around` in a mono signal, in seconds. Phrases are split here, so
/// no word ends up on the wrong side because recognition's segment boundary was late.
pub fn quietest_point(mono: &[f32], sr: f32, around: f64, reach: f64, earliest: f64) -> f64 {
    let win = ((0.02 * sr as f64) as usize).max(1);
    let hop = win / 2;
    let a = (((around - reach).max(earliest)) * sr as f64) as usize;
    let b = (((around + reach) * sr as f64) as usize).min(mono.len().saturating_sub(win));
    let mut best = (f32::MAX, around.max(earliest));
    let mut at = a;
    while at < b {
        let energy: f32 = mono[at..at + win].iter().map(|v| v * v).sum();
        if energy < best.0 {
            best = (energy, (at + win / 2) as f64 / sr as f64);
        }
        at += hop.max(1);
    }
    best.1
}

/// Moves raw transcription timings onto the audio. Whisper's word times are often tens of
/// milliseconds off and clip consonants, so each edge is found from the loudness contour near it.
///
/// `words` holds (start, end) in order. Words won't be moved over one another.
pub fn refine_boundaries(mono: &[f32], sr: f32, words: &mut [(f64, f64)]) {
    const LOOK_BEFORE: f64 = 0.12;
    const LOOK_AFTER: f64 = 0.08;
    let hop = ((0.002 * sr as f64) as usize).max(1);
    let win = hop * 3;
    let energy_at = |t: f64| -> f32 {
        let a = ((t.max(0.0) * sr as f64) as usize).min(mono.len());
        let b = (a + win).min(mono.len());
        if a >= b {
            return 0.0;
        }
        mono[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32
    };
    let step = hop as f64 / sr as f64;
    for i in 0..words.len() {
        let (start, end) = words[i];
        let prev_end = if i > 0 { words[i - 1].1 } else { 0.0 };
        let next_start = words.get(i + 1).map_or(f64::MAX, |w| w.0);
        let mut peak = 0f32;
        let mut t = start;
        while t < end {
            peak = peak.max(energy_at(t));
            t += step;
        }
        if peak <= 0.0 {
            continue;
        }
        // About -30 dB under the word's peak separates speech from room noise.
        let floor = peak * 0.001;

        // Start: from a little after the given time, walk back until the sound drops to the floor.
        let earliest = (start - LOOK_BEFORE).max(prev_end);
        let mut new_start = (start + LOOK_AFTER).min((start + end) * 0.5);
        while new_start > earliest && energy_at(new_start - step) > floor {
            new_start -= step;
        }
        // If the given time was already in silence, walk forward to where the sound starts.
        while new_start < end - step && energy_at(new_start) <= floor {
            new_start += step;
        }

        let latest = (end + LOOK_BEFORE).min(next_start);
        let mut new_end = (end - LOOK_AFTER).max((start + end) * 0.5).max(new_start + step);
        while new_end < latest && energy_at(new_end) > floor {
            new_end += step;
        }
        while new_end > new_start + step && energy_at(new_end - step) <= floor {
            new_end -= step;
        }
        words[i] = (new_start.max(earliest), new_end.min(latest));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn noise(n: usize, amp: f32) -> Vec<f32> {
        let mut seed = 11u32;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    fn tone(n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 200.0 / SR).sin()).collect()
    }

    fn sym(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn word(symbols: &[&str], anchor: f64) -> PhraseWord {
        PhraseWord { symbols: sym(symbols), anchor }
    }

    #[test]
    fn fricative_then_vowel_splits_where_the_sound_changes() {
        // "sa": 150 ms of noise, then 150 ms of a voiced tone.
        let mut x = noise(7200, 0.2);
        x.extend(tone(7200, 0.8));
        let w = align_phrase(&x, SR, 1.0, &[word(&["s", "aa"], 1.15)]);
        let p = &w[0].phonemes;
        assert_eq!(p.len(), 2);
        assert!((p[0].end - 1.15).abs() < 0.03, "boundary at {}", p[0].end);
    }

    #[test]
    fn phonemes_are_contiguous_and_in_order() {
        let mut x = tone(4800, 0.1);
        x.extend(noise(4800, 0.3));
        x.extend(tone(9600, 0.9));
        x.extend(noise(2400, 0.1));
        let w = align_phrase(&x, SR, 0.0, &[word(&["m", "s", "ae", "t"], 0.2)]);
        for pair in w[0].phonemes.windows(2) {
            assert!((pair[0].end - pair[1].start).abs() < 1e-9);
            assert!(pair[0].end > pair[0].start);
        }
    }

    #[test]
    fn very_short_words_fall_back_cleanly() {
        let x = tone(200, 0.5);
        let w = align_phrase(&x, SR, 2.0, &[word(&["k", "ae", "t"], 2.0)]);
        assert_eq!(w[0].phonemes.len(), 3);
        assert!((w[0].end - (2.0 + 200.0 / 48000.0)).abs() < 1e-6);
    }

    #[test]
    fn silence_between_words_stays_out_of_both_words() {
        // Voiced word, 400 ms of silence, voiced word. Anchors are off by 100 ms.
        let mut x = tone(14400, 0.8);
        x.extend(vec![0.0; 19200]);
        x.extend(tone(14400, 0.8));
        let words = [word(&["aa"], 0.25), word(&["aa"], 0.95)];
        let w = align_phrase(&x, SR, 0.0, &words);
        assert!((w[0].end - 0.3).abs() < 0.03, "first ends at {}", w[0].end);
        assert!((w[1].start - 0.7).abs() < 0.03, "second starts at {}", w[1].start);
    }

    #[test]
    fn long_sung_notes_are_allowed() {
        let x = tone(48000 * 3, 0.8);
        let w = align_phrase(&x, SR, 0.0, &[word(&["aa"], 1.5)]);
        assert!(w[0].end - w[0].start > 2.5, "{:?}", w[0]);
    }

    #[test]
    fn quietest_point_finds_the_gap() {
        let mut x = tone(48000, 0.8);
        x.extend(vec![0.0; 4800]);
        x.extend(tone(48000, 0.8));
        let t = quietest_point(&x, SR, 1.3, 0.5, 0.0);
        assert!((1.0..=1.1).contains(&t), "{t}");
    }

    #[test]
    fn boundaries_snap_to_the_sound() {
        // Word audio from 1.0 to 1.5 s, with timings 50 ms late at the start and 50 ms early at the end.
        let mut x = vec![0f32; 48000];
        x.extend(tone(24000, 0.5));
        x.extend(vec![0f32; 48000]);
        let mut words = [(1.05, 1.45)];
        refine_boundaries(&x, SR, &mut words);
        assert!((words[0].0 - 1.0).abs() < 0.01, "start {}", words[0].0);
        assert!((words[0].1 - 1.5).abs() < 0.01, "end {}", words[0].1);
    }

    #[test]
    fn boundaries_do_not_cross_neighbours() {
        let x = tone(96000, 0.5);
        let mut words = [(0.5, 0.8), (0.82, 1.2)];
        refine_boundaries(&x, SR, &mut words);
        assert!(words[0].1 <= words[1].0 + 1e-9, "{words:?}");
    }
}
