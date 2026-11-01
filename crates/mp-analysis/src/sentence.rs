//! The Sentence Builder's planner: from typed text to a list of audio pieces.
//!
//! It prefers runs of consecutive words from the original recording because they keep their natural
//! flow, then single takes, then stitches words nobody ever said out of phonemes.

use crate::search::{Hit, WordIndex};
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
}

impl SentencePart {
    /// Audio pieces to place on the timeline, in order.
    pub fn pieces(&self) -> Vec<&Hit> {
        match self.source {
            PartSource::Takes => self.options.get(self.selected).into_iter().collect(),
            PartSource::Stitched => self.options.iter().collect(),
            PartSource::Missing => Vec::new(),
        }
    }
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
            let mut hits = index.find_phrase(&phrase);
            if !hits.is_empty() {
                hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
                parts.push(SentencePart { text: phrase, source: PartSource::Takes, options: hits, selected: 0 });
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
            parts.push(SentencePart { text: word.clone(), source: PartSource::Takes, options: takes, selected: 0 });
        } else {
            let pieces = stitch(word, index);
            let source = if pieces.is_empty() { PartSource::Missing } else { PartSource::Stitched };
            parts.push(SentencePart { text: word.clone(), source, options: pieces, selected: 0 });
        }
        i += 1;
    }
    parts
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
    use crate::search::tests::index;

    #[test]
    fn prefers_whole_phrases_over_single_words() {
        let idx = index("never gonna give you up you know");
        let parts = plan("gonna give you know", &idx);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].text, "gonna give you");
        assert_eq!(parts[1].text, "know");
        assert_eq!(parts[0].source, PartSource::Takes);
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
    fn empty_input_plans_nothing() {
        assert!(plan("  ,, ", &index("hello")).is_empty());
    }
}
