//! The Sentence Builder's planner: from typed text to a list of audio pieces.
//!
//! It prefers runs of consecutive words from the original recording because they keep their natural
//! flow, then single takes, then stitches words nobody ever said out of phonemes.

use crate::search::{Hit, HitKind, WordIndex};
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
}

impl SentencePart {
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
                parts.push(SentencePart { text: phrase, source: PartSource::Takes, options: hits, selected: 0, made: None, use_made: false });
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
            parts.push(SentencePart { text: word.clone(), source: PartSource::Takes, options: takes, selected: 0, made: None, use_made: false });
        } else {
            let pieces = stitch(word, index);
            let source = if pieces.is_empty() { PartSource::Missing } else { PartSource::Stitched };
            parts.push(SentencePart { text: word.clone(), source, options: pieces, selected: 0, made: None, use_made: false });
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

/// Builds a word out of the longest phoneme runs available. Returns empty if any sound is missing.
pub fn stitch(word: &str, index: &WordIndex) -> Vec<Hit> {
    let symbols = g2p::phonemes(word);
    let mut pieces = Vec::new();
    let mut at = 0;
    while at < symbols.len() {
        let mut best: Option<(usize, Hit)> = None;
        for len in (1..=symbols.len() - at).rev() {
            let hits = index.find_phonemes(&symbols[at..at + len]);
            if let Some(hit) = hits.into_iter().max_by(|a, b| a.confidence.total_cmp(&b.confidence)) {
                best = Some((len, hit));
                break;
            }
        }
        let Some((len, hit)) = best else { return Vec::new() };
        pieces.push(hit);
        at += len;
    }
    pieces
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
