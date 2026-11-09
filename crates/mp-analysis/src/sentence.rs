//! The Sentence Builder's planner: from typed text to a list of audio pieces.
//!
//! It prefers runs of consecutive words from the original recording because they keep their natural
//! flow, then single takes, then stitches words nobody ever said out of phonemes.

use crate::search::{Hit, HitKind, SoundRun, WordIndex};
use crate::{g2p, normalize_word};

/// How a part of the sentence will be sourced.
#[derive(Clone, Debug, PartialEq)]
pub enum PartSource {
    /// A word or run of words as originally spoken. `options` holds alternate takes.
    Takes,
    /// Made from pieces of other words. `options` holds the pieces, all of which are used, in order.
    Stitched,
    /// Nothing in the recordings can produce this.
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SentencePart {
    /// The words this part covers, as typed.
    pub text: String,
    pub source: PartSource,
    pub options: Vec<Hit>,
    /// Which take is chosen, for `PartSource::Takes`.
    pub selected: usize,
    /// A word the user made with this name, whose id the caller fills in. It's preferred over takes
    /// and stitching, because someone who made a word has already said what it should sound like.
    pub made: Option<u64>,
    pub use_made: bool,
    /// Other ways to stitch the word, for `PartSource::Stitched`. `options` is the one chosen.
    pub stitches: Vec<Vec<Hit>>,
}

impl SentencePart {
    pub fn new(text: String, source: PartSource, options: Vec<Hit>) -> Self {
        Self { text, source, options, selected: 0, made: None, use_made: false, stitches: Vec::new() }
    }

    /// How many versions there are to choose between: takes, or ways to stitch it.
    pub fn choices(&self) -> usize {
        match self.source {
            PartSource::Takes => self.options.len(),
            PartSource::Stitched => self.stitches.len(),
            PartSource::Missing => 0,
        }
    }

    /// Chooses a take, or a way to stitch it.
    pub fn choose(&mut self, i: usize) {
        self.selected = i.min(self.choices().saturating_sub(1));
        if self.source == PartSource::Stitched {
            if let Some(pieces) = self.stitches.get(self.selected) {
                self.options = pieces.clone();
            }
        }
    }

    /// Audio pieces to place on the timeline, in order.
    pub fn pieces(&self) -> Vec<&Hit> {
        if self.use_made {
            return Vec::new();
        }
        match self.source {
            PartSource::Takes => self.options.get(self.selected).into_iter().collect(),
            PartSource::Stitched => self.options.iter().collect(),
            PartSource::Missing => Vec::new(),
        }
    }
}

/// A stretch of words that differs between two versions of a line.
#[derive(Clone, Debug, PartialEq)]
pub struct WordChange {
    pub old: std::ops::Range<usize>,
    pub new: std::ops::Range<usize>,
}

/// What changed between two lines of words. Words kept in order aren't in the result, so that an
/// edit only touches the words that really changed.
pub fn diff_words(old: &[String], new: &[String]) -> Vec<WordChange> {
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changes: Vec<WordChange> = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
            continue;
        }
        let (old_from, new_from) = (i, j);
        while (i < old.len() || j < new.len()) && !(i < old.len() && j < new.len() && old[i] == new[j]) {
            if j >= new.len() || (i < old.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
                i += 1;
            } else {
                j += 1;
            }
        }
        changes.push(WordChange { old: old_from..i, new: new_from..j });
    }
    changes
}

/// Plans a typed sentence against everything in the index.
pub fn plan(text: &str, index: &WordIndex) -> Vec<SentencePart> {
    let words: Vec<String> = text.split_whitespace().map(normalize_word).filter(|w| !w.is_empty()).collect();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < words.len() {
        // Longest phrase first. Six words is long for a sentence mix, and keeps this quick.
        let mut matched = false;
        for len in (2..=(words.len() - i).min(6)).rev() {
            let phrase = words[i..i + len].join(" ");
            let mut hits = index.find_connected_phrase(&phrase);
            if !hits.is_empty() {
                hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
                parts.push(SentencePart::new(phrase, PartSource::Takes, hits));
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        let word = &words[i];
        let takes = index.takes_of(word);
        if !takes.is_empty() {
            parts.push(SentencePart::new(word.clone(), PartSource::Takes, takes));
        } else {
            let stitches = stitches(word, index);
            let source = if stitches.is_empty() { PartSource::Missing } else { PartSource::Stitched };
            let mut part = SentencePart::new(word.clone(), source, Vec::new());
            part.stitches = stitches;
            part.choose(0);
            parts.push(part);
        }
        i += 1;
    }
    choose_takes(&mut parts);
    parts
}

/// A first guess at takes: each word close in pitch to the one before, so a line sounds less stitched.
/// It trades off with confidence, and a true match always wins over a word that only sounds the same.
fn choose_takes(parts: &mut [SentencePart]) {
    let mut previous: Option<f32> = None;
    for part in parts.iter_mut() {
        if part.source == PartSource::Takes && part.options.len() > 1 {
            let exact = part.options.iter().any(|h| h.kind != HitKind::Homophone);
            let cost = |h: &Hit| {
                let leap = match (previous, h.pitch_hz) {
                    (Some(a), Some(b)) => (12.0 * (b / a).log2()).abs() / 12.0,
                    _ => 0.0,
                };
                let wrong_word = if exact && h.kind == HitKind::Homophone { 10.0 } else { 0.0 };
                leap + (1.0 - h.confidence) + wrong_word
            };
            part.selected = (0..part.options.len()).min_by(|a, b| cost(&part.options[*a]).total_cmp(&cost(&part.options[*b]))).unwrap_or(0);
        }
        if let Some(pitch) = part.pieces().last().and_then(|h| h.pitch_hz) {
            previous = Some(pitch);
        }
    }
}

/// Ways to build a word out of sounds from other words, best first. Empty if any sound is missing.
///
/// This is unit selection, as in concatenative speech synthesis. A sound is coloured by the ones
/// around it, so a piece fits best where it was said with the same neighbours, or at the start or end
/// of a word when it starts or ends this one. Every piece is a join, and joins are least audible at a
/// stop or a hiss, where the sound is quiet or noisy anyway, and most audible inside a vowel. A pitch
/// jump between pieces is heard too. The cheapest path through all of that is the first version, and
/// the others are found by making the pieces already used dearer.
pub fn stitches(word: &str, index: &WordIndex) -> Vec<Vec<Hit>> {
    const VERSIONS: usize = 5;
    let mut found: Vec<(f32, Vec<Hit>)> = Vec::new();
    // Variants that differed only in stress are the same once stress is gone.
    let mut pronunciations: Vec<Vec<String>> = Vec::new();
    for p in g2p::pronunciations(word) {
        if !p.is_empty() && !pronunciations.contains(&p) {
            pronunciations.push(p);
        }
    }
    for symbols in pronunciations {
        let runs = best_runs(index.sound_runs(&symbols, symbols.len()), &symbols);
        let mut used = vec![0u32; runs.len()];
        for _ in 0..VERSIONS {
            let Some((cost, path)) = cheapest_path(&runs, &symbols, &used) else { break };
            for &r in &path {
                used[r] += 1;
            }
            let pieces: Vec<Hit> = path.iter().map(|&r| runs[r].0.hit.clone()).collect();
            if !found.iter().any(|(_, f)| *f == pieces) {
                found.push((cost, pieces));
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.truncate(VERSIONS);
    found.into_iter().map(|(_, pieces)| pieces).collect()
}

/// How loud and open a sound is. Joins go best where it's low.
fn sonority(symbol: &str) -> f32 {
    use g2p::PhonemeClass::*;
    match g2p::class_of(symbol) {
        UnvoicedStop | VoicedStop | Affricate => 0.0,
        UnvoicedFricative => 1.0,
        VoicedFricative => 2.0,
        Nasal => 3.0,
        Glide => 4.0,
        Vowel => 5.0,
    }
}

/// What it costs for a piece to have `got` beside it where `want` should be. `None` is a pause, or the
/// edge of the word being made.
fn edge_cost(want: Option<&String>, got: Option<&String>, at_word_edge: bool) -> f32 {
    match (want, got) {
        (None, None) => 0.0,
        // Words run together in speech, so the edge of a word is nearly as clean as a pause.
        (None, Some(_)) if at_word_edge => 0.15,
        (None, Some(_)) => 0.6,
        (Some(w), Some(g)) if w == g => 0.0,
        (Some(w), Some(g)) if g2p::class_of(w) == g2p::class_of(g) => 0.25,
        (Some(_), None) => 0.35,
        (Some(_), Some(_)) => 0.5,
    }
}

/// What a piece costs on its own, lower being better.
fn run_cost(run: &SoundRun, symbols: &[String]) -> f32 {
    const JOIN: f32 = 1.0;
    let before = run.at.checked_sub(1).map(|i| &symbols[i]);
    let after = symbols.get(run.at + run.len);
    let mut cost = JOIN + edge_cost(before, run.before.as_ref(), run.starts_word) + edge_cost(after, run.after.as_ref(), run.ends_word);
    cost += (1.0 - run.hit.confidence) * 0.6 + run.crosses as f32 * 0.25;
    // Alignment that collapsed a sound to almost nothing has probably put its edges in the wrong place.
    if run.shortest < 0.025 {
        cost += 0.6;
    }
    cost
}

/// The best few runs for each stretch of the word, each with its cost. Keeping only a few keeps this
/// quick enough to run on every key press.
fn best_runs(runs: Vec<SoundRun>, symbols: &[String]) -> Vec<(SoundRun, f32)> {
    const KEEP: usize = 12;
    let mut costed: Vec<(SoundRun, f32)> = runs.into_iter().map(|r| {
        let cost = run_cost(&r, symbols);
        (r, cost)
    }).collect();
    costed.sort_by(|a, b| (a.0.at, a.0.len).cmp(&(b.0.at, b.0.len)).then(a.1.total_cmp(&b.1)));
    let mut kept: Vec<(SoundRun, f32)> = Vec::new();
    for run in costed {
        let same = kept.iter().rev().take_while(|k| (k.0.at, k.0.len) == (run.0.at, run.0.len)).count();
        if same < KEEP {
            kept.push(run);
        }
    }
    kept
}

/// What joining `b` after `a` costs: where the cut falls, and how far the pitch jumps.
fn join_cost(a: &SoundRun, b: &SoundRun, symbols: &[String]) -> f32 {
    let cut = sonority(&symbols[b.at - 1]).min(sonority(&symbols[b.at])) * 0.1;
    let leap = match (a.hit.pitch_hz, b.hit.pitch_hz) {
        (Some(x), Some(y)) => (12.0 * (y / x).log2()).abs() / 12.0 * 0.6,
        _ => 0.0,
    };
    cut + leap
}

/// The cheapest way through the word, as indices into `runs`. `used` makes runs already in another
/// version dearer, so that the next version differs.
fn cheapest_path(runs: &[(SoundRun, f32)], symbols: &[String], used: &[u32]) -> Option<(f32, Vec<usize>)> {
    const REUSE: f32 = 0.8;
    let n = symbols.len();
    let mut best: Vec<Option<(f32, Option<usize>)>> = vec![None; runs.len()];
    let mut ending_at: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
    // Runs are sorted by where they start, so everything a run can follow has been worked out first.
    for (k, (run, cost)) in runs.iter().enumerate() {
        let own = cost + used[k] as f32 * REUSE;
        best[k] = if run.at == 0 {
            Some((own, None))
        } else {
            ending_at[run.at]
                .iter()
                .filter_map(|&m| best[m].map(|(so_far, _)| (so_far + join_cost(&runs[m].0, run, symbols) + own, Some(m))))
                .min_by(|a, b| a.0.total_cmp(&b.0))
        };
        if best[k].is_some() {
            ending_at[run.at + run.len].push(k);
        }
    }
    let end = *ending_at[n].iter().min_by(|a, b| best[**a].expect("reachable").0.total_cmp(&best[**b].expect("reachable").0))?;
    let mut path = vec![end];
    while let Some(Some(previous)) = best[*path.last().expect("not empty")].map(|b| b.1) {
        path.push(previous);
    }
    path.reverse();
    // The cost without the reuse penalty, so versions from different pronunciations compare fairly.
    let real = path.iter().map(|&k| runs[k].1).sum::<f32>() + path.windows(2).map(|w| join_cost(&runs[w[0]].0, &runs[w[1]].0, symbols)).sum::<f32>();
    Some((real, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::tests::{index, word};
    use crate::Transcript;

    /// Words with no gaps between them, as in connected speech.
    fn connected_index(text: &str) -> WordIndex {
        let words = text.split_whitespace().enumerate().map(|(i, w)| word(w, i as f64 * 0.3, 0.9)).collect();
        let mut idx = WordIndex::default();
        idx.set_transcript(1, Transcript { words, model: "test".into(), before_lyrics: None });
        idx
    }

    #[test]
    fn prefers_whole_phrases_over_single_words() {
        let idx = connected_index("never gonna give you up you know");
        let parts = plan("gonna give you know", &idx);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].text, "gonna give you");
        assert_eq!(parts[1].text, "know");
        assert_eq!(parts[0].source, PartSource::Takes);
    }

    #[test]
    fn phrases_do_not_span_a_pause() {
        // In the test index, words are 0.3 s long and start 1 s apart, so each pair has a long gap.
        let idx = index("never gonna");
        let parts = plan("never gonna", &idx);
        assert_eq!(parts.len(), 2, "{parts:?}");
    }

    #[test]
    fn single_words_offer_every_take() {
        let idx = index("go stop go stop go");
        let parts = plan("go", &idx);
        assert_eq!(parts[0].options.len(), 3);
        assert_eq!(parts[0].pieces().len(), 1);
    }

    #[test]
    fn stitches_unsaid_words_from_phonemes() {
        // "cat" is never said, but its sounds are all there.
        let idx = index("kid hat");
        let parts = plan("cat", &idx);
        assert_eq!(parts[0].source, PartSource::Stitched);
        let pieces = parts[0].pieces();
        let spelled: Vec<&str> = pieces.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(spelled, ["k", "ae t"]);
    }

    fn spelled(pieces: &[Hit]) -> Vec<&str> {
        pieces.iter().map(|h| h.text.as_str()).collect()
    }

    #[test]
    fn stitching_cuts_at_a_stop_rather_than_into_a_vowel() {
        // "mat" is "m" + "ae t" or "m ae" + "t". Cutting before the "t" hides the join in its closure.
        let idx = index("mad pat");
        let parts = plan("mat", &idx);
        assert_eq!(spelled(&parts[0].options), ["m ae", "t"]);
    }

    #[test]
    fn stitching_takes_sounds_across_words_said_together() {
        let idx = connected_index("black at");
        let parts = plan("cat", &idx);
        assert_eq!(spelled(&parts[0].options), ["k ae t"]);
        assert_eq!(parts[0].options[0].context.as_deref(), Some("black at"));
    }

    #[test]
    fn stitching_never_runs_across_a_pause() {
        // In the test index, words start a second apart, so "black" and "at" aren't joined.
        let idx = index("black at");
        let parts = plan("cat", &idx);
        assert_eq!(spelled(&parts[0].options), ["k", "ae t"]);
    }

    #[test]
    fn a_stitched_word_offers_other_versions() {
        let idx = index("mad pat mud kit");
        let mut part = plan("mat", &idx).remove(0);
        assert!(part.choices() >= 2, "{:?}", part.stitches);
        let first = part.options.clone();
        part.choose(1);
        assert_ne!(part.options, first);
        assert_eq!(part.pieces().len(), part.options.len());
    }

    #[test]
    fn impossible_words_are_marked_missing() {
        let idx = index("aaa");
        let parts = plan("zebra", &idx);
        assert_eq!(parts[0].source, PartSource::Missing);
        assert!(parts[0].pieces().is_empty());
    }

    #[test]
    fn takes_stay_close_in_pitch_to_the_word_before() {
        let mut words = vec![
            word("hello", 0.0, 0.9),
            word("and", 0.5, 0.9), // keeps "hello world" from being a phrase
            word("world", 1.0, 0.9),
            word("world", 2.0, 0.9),
        ];
        words[0].pitch_hz = Some(220.0);
        words[2].pitch_hz = Some(440.0); // an octave away from "hello"
        words[3].pitch_hz = Some(233.0); // a semitone away
        let mut idx = WordIndex::default();
        idx.set_transcript(1, Transcript { words, model: "t".into(), before_lyrics: None });
        let parts = plan("hello world", &idx);
        let world = parts.iter().find(|p| p.text == "world").unwrap();
        assert_eq!(world.pieces()[0].start, 2.0);
    }

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_diff_finds_only_the_words_that_changed() {
        let change = diff_words(&words("the lazy dog sleeps"), &words("the crazy dog sleeps"));
        assert_eq!(change, [WordChange { old: 1..2, new: 1..2 }]);
        let change = diff_words(&words("the lazy dog"), &words("the dog"));
        assert_eq!(change, [WordChange { old: 1..2, new: 1..1 }]);
        let change = diff_words(&words("the dog"), &words("the big red dog"));
        assert_eq!(change, [WordChange { old: 1..1, new: 1..3 }]);
        assert!(diff_words(&words("same words"), &words("same words")).is_empty());
        let change = diff_words(&words("one two three four"), &words("uno two three cuatro"));
        assert_eq!(change, [WordChange { old: 0..1, new: 0..1 }, WordChange { old: 3..4, new: 3..4 }]);
    }

    #[test]
    fn empty_input_plans_nothing() {
        assert!(plan("  ,, ", &index("hello")).is_empty());
    }
}
