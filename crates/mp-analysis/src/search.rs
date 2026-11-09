//! Search over every transcribed word and phoneme.
//!
//! Query syntax:
//! - `hello`: words, ranked exact, sounds-the-same, prefix, contains, then near-misses
//! - `hello world`: consecutive takes of a phrase
//! - `/k ae t/`: a phoneme sequence, anywhere, including inside other words
//! - `~cat`: words that rhyme

use crate::{g2p, normalize_word, Transcript};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum HitKind {
    Exact,
    Homophone,
    Phrase,
    Prefix,
    Contains,
    Fuzzy,
    Rhyme,
    /// Part of a word, found by its sounds.
    Phonemes,
}

impl HitKind {
    pub fn label(&self) -> &'static str {
        match self {
            HitKind::Exact => "exact",
            HitKind::Homophone => "sounds same",
            HitKind::Phrase => "phrase",
            HitKind::Prefix => "starts with",
            HitKind::Contains => "contains",
            HitKind::Fuzzy => "close",
            HitKind::Rhyme => "rhyme",
            HitKind::Phonemes => "sound",
        }
    }
}

/// A findable range of audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The key the transcript was added under.
    pub source: u64,
    pub start: f64,
    pub end: f64,
    /// What was found, such as "hello".
    pub text: String,
    /// The word it's inside, when a sound was found within a word.
    pub context: Option<String>,
    pub kind: HitKind,
    pub confidence: f32,
    pub pitch_hz: Option<f32>,
    /// Lets the UI find this word in its transcript again.
    pub word_index: usize,
}

/// A sound to find, with what would surround it.
#[derive(Clone, Debug, Default)]
pub struct SoundWanted {
    pub symbols: Vec<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub pitch_hz: Option<f32>,
    pub length: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SoundCandidate {
    pub hit: Hit,
    pub same_before: bool,
    pub same_after: bool,
}

/// A run of sounds in the recordings that matches part of a word being made.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundRun {
    pub hit: Hit,
    /// Where in the wanted sounds it starts, and how many it covers.
    pub at: usize,
    pub len: usize,
    /// The sounds just outside it where it was said. `None` is a pause.
    pub before: Option<String>,
    pub after: Option<String>,
    /// Whether it begins a word, and ends one, where it was said.
    pub starts_word: bool,
    pub ends_word: bool,
    /// How many gaps between words it runs across.
    pub crosses: usize,
    /// Its shortest sound in seconds. Very short ones are usually where alignment went wrong.
    pub shortest: f64,
}

/// Words closer than this run into each other, so sounds can be taken across them.
const CONNECTED: f64 = 0.03;

struct Entry {
    source: u64,
    word_index: usize,
    normalized: String,
}

/// A prebuilt index across every transcript in the project.
#[derive(Default)]
pub struct WordIndex {
    transcripts: HashMap<u64, Transcript>,
    entries: Vec<Entry>,
    by_word: HashMap<String, Vec<usize>>,
}

impl WordIndex {
    pub fn set_transcript(&mut self, source: u64, transcript: Transcript) {
        self.transcripts.insert(source, transcript);
        self.rebuild();
    }

    pub fn remove_transcript(&mut self, source: u64) {
        self.transcripts.remove(&source);
        self.rebuild();
    }

    pub fn transcript(&self, source: u64) -> Option<&Transcript> {
        self.transcripts.get(&source)
    }

    pub fn word_count(&self) -> usize {
        self.entries.len()
    }

    fn rebuild(&mut self) {
        self.entries.clear();
        self.by_word.clear();
        let mut sources: Vec<u64> = self.transcripts.keys().copied().collect();
        sources.sort_unstable();
        for source in sources {
            for (word_index, word) in self.transcripts[&source].words.iter().enumerate() {
                let normalized = normalize_word(&word.text);
                if normalized.is_empty() {
                    continue;
                }
                self.by_word.entry(normalized.clone()).or_default().push(self.entries.len());
                self.entries.push(Entry { source, word_index, normalized });
            }
        }
    }

    fn word_hit(&self, entry: &Entry, kind: HitKind) -> Hit {
        let w = &self.transcripts[&entry.source].words[entry.word_index];
        Hit {
            source: entry.source,
            start: w.start,
            end: w.end,
            text: w.text.clone(),
            context: None,
            kind,
            confidence: w.confidence,
            pitch_hz: w.pitch_hz,
            word_index: entry.word_index,
        }
    }

    /// Every take of exactly this word, or a word that sounds the same.
    pub fn takes_of(&self, word: &str) -> Vec<Hit> {
        let word = normalize_word(word);
        let mut hits: Vec<Hit> = self
            .by_word
            .get(&word)
            .into_iter()
            .flatten()
            .map(|&i| self.word_hit(&self.entries[i], HitKind::Exact))
            .collect();
        let phones = g2p::pronunciations(&word);
        for (other, entries) in &self.by_word {
            if *other != word && phones.iter().any(|p| g2p::pronunciations(other).contains(p)) {
                hits.extend(entries.iter().map(|&i| self.word_hit(&self.entries[i], HitKind::Homophone)));
            }
        }
        hits.sort_by(|a, b| rank(a).total_cmp(&rank(b)));
        hits
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let mut hits = if let Some(inner) = q.strip_prefix('/') {
            let symbols: Vec<String> = inner
                .trim_end_matches('/')
                .split_whitespace()
                .map(str::to_lowercase)
                .collect();
            self.find_phonemes(&symbols)
        } else if let Some(word) = q.strip_prefix('~') {
            self.find_rhymes(word)
        } else if q.split_whitespace().count() > 1 {
            self.find_phrase(q)
        } else {
            self.find_word(q)
        };
        hits.sort_by(|a, b| rank(a).total_cmp(&rank(b)));
        hits.truncate(limit);
        hits
    }

    fn find_word(&self, q: &str) -> Vec<Hit> {
        let q = normalize_word(q);
        if q.is_empty() {
            return Vec::new();
        }
        let mut hits = self.takes_of(&q);
        for entry in &self.entries {
            let w = &entry.normalized;
            if *w == q {
                continue;
            }
            let kind = if w.starts_with(&q) {
                HitKind::Prefix
            } else if q.len() >= 2 && w.contains(&q) {
                HitKind::Contains
            } else if q.len() >= 4 && edit_distance(w, &q) <= 1 + q.len() / 6 {
                HitKind::Fuzzy
            } else {
                continue;
            };
            let hit = self.word_hit(entry, kind);
            if !hits.iter().any(|h| h.source == hit.source && h.word_index == hit.word_index) {
                hits.push(hit);
            }
        }
        hits
    }

    /// Consecutive words that spell out a phrase.
    pub fn find_phrase(&self, q: &str) -> Vec<Hit> {
        let wanted: Vec<String> = q.split_whitespace().map(normalize_word).filter(|w| !w.is_empty()).collect();
        let mut hits = Vec::new();
        let Some(first) = wanted.first() else { return hits };
        for &i in self.by_word.get(first).into_iter().flatten() {
            let entry = &self.entries[i];
            let words = &self.transcripts[&entry.source].words;
            let Some(window) = words.get(entry.word_index..entry.word_index + wanted.len()) else { continue };
            if window.iter().zip(&wanted).all(|(w, t)| normalize_word(&w.text) == *t) {
                let last = &window[window.len() - 1];
                let confidence = window.iter().map(|w| w.confidence).fold(1.0, f32::min);
                hits.push(Hit {
                    source: entry.source,
                    start: window[0].start,
                    end: last.end,
                    text: wanted.join(" "),
                    context: None,
                    kind: HitKind::Phrase,
                    confidence,
                    pitch_hz: window[0].pitch_hz,
                    word_index: entry.word_index,
                });
            }
        }
        hits
    }

    /// Like `find_phrase`, but only runs of words said without a pause. A clip that goes silent in the
    /// middle of a typed line isn't what anyone wants, so those are better as separate words.
    pub fn find_connected_phrase(&self, q: &str) -> Vec<Hit> {
        const LONGEST_GAP: f64 = 0.15;
        let mut hits = self.find_phrase(q);
        let length = q.split_whitespace().count();
        hits.retain(|h| {
            let words = &self.transcripts[&h.source].words;
            words[h.word_index..h.word_index + length].windows(2).all(|w| w[1].start - w[0].end <= LONGEST_GAP)
        });
        hits
    }

    /// A phoneme sequence anywhere, inside a word.
    pub fn find_phonemes(&self, symbols: &[String]) -> Vec<Hit> {
        let mut hits = Vec::new();
        if symbols.is_empty() {
            return hits;
        }
        for entry in &self.entries {
            let word = &self.transcripts[&entry.source].words[entry.word_index];
            for window_start in 0..word.phonemes.len().saturating_sub(symbols.len() - 1) {
                let window = &word.phonemes[window_start..window_start + symbols.len()];
                if window.iter().zip(symbols).all(|(p, s)| p.symbol == *s) {
                    hits.push(Hit {
                        source: entry.source,
                        start: window[0].start,
                        end: window[window.len() - 1].end,
                        text: symbols.join(" "),
                        context: Some(word.text.clone()),
                        kind: HitKind::Phonemes,
                        // Whole-word matches sound cleanest because they have natural edges.
                        confidence: word.confidence * if word.phonemes.len() == symbols.len() { 1.0 } else { 0.8 },
                        pitch_hz: word.pitch_hz,
                        word_index: entry.word_index,
                    });
                }
            }
        }
        hits
    }

    /// Every run of up to `longest` sounds that matches part of `wanted`, including runs across words
    /// said without a pause, with what surrounds each one.
    pub fn sound_runs(&self, wanted: &[String], longest: usize) -> Vec<SoundRun> {
        let mut out = Vec::new();
        let mut sources: Vec<u64> = self.transcripts.keys().copied().collect();
        sources.sort_unstable();
        for source in sources {
            let words = &self.transcripts[&source].words;
            // Every sound in order, and whether each runs straight into the next.
            let mut stream: Vec<(usize, usize)> = Vec::new();
            let mut joined: Vec<bool> = Vec::new();
            for (w, word) in words.iter().enumerate() {
                for p in 0..word.phonemes.len() {
                    if let Some(last) = joined.last_mut() {
                        *last = p > 0 || words.get(w.wrapping_sub(1)).is_some_and(|before| word.start - before.end <= CONNECTED);
                    }
                    stream.push((w, p));
                    joined.push(false);
                }
            }
            let symbol = |k: usize| words[stream[k].0].phonemes[stream[k].1].symbol.as_str();
            for k in 0..stream.len() {
                for at in 0..wanted.len() {
                    let mut len = 0;
                    while len < longest && at + len < wanted.len() && k + len < stream.len() && symbol(k + len) == wanted[at + len] && (len == 0 || joined[k + len - 1]) {
                        len += 1;
                        let last = k + len - 1;
                        let ((first_word, first_phoneme), (last_word, last_phoneme)) = (stream[k], stream[last]);
                        let (first, end) = (&words[first_word], &words[last_word]);
                        let shortest = (k..=last).map(|s| {
                            let p = &words[stream[s].0].phonemes[stream[s].1];
                            p.end - p.start
                        });
                        let text: Vec<&str> = (first_word..=last_word).map(|w| words[w].text.as_str()).collect();
                        out.push(SoundRun {
                            hit: Hit {
                                source,
                                start: first.phonemes[first_phoneme].start,
                                end: end.phonemes[last_phoneme].end,
                                text: wanted[at..at + len].join(" "),
                                context: Some(text.join(" ")),
                                kind: HitKind::Phonemes,
                                confidence: (first_word..=last_word).map(|w| words[w].confidence).fold(1.0, f32::min),
                                pitch_hz: first.pitch_hz.or(end.pitch_hz),
                                word_index: first_word,
                            },
                            at,
                            len,
                            before: (k > 0 && joined[k - 1]).then(|| symbol(k - 1).to_string()),
                            after: joined[last].then(|| symbol(last + 1).to_string()),
                            starts_word: first_phoneme == 0,
                            ends_word: last_phoneme + 1 == end.phonemes.len(),
                            crosses: last_word - first_word,
                            shortest: shortest.fold(f64::INFINITY, f64::min),
                        });
                    }
                }
            }
        }
        out
    }

    /// Every occurrence of a sound, best fit first, for filling a gap in a word. A sound depends on
    /// what's around it, so ones with the same neighbours rank first, then close pitch and length.
    pub fn sound_candidates(&self, wanted: &SoundWanted) -> Vec<SoundCandidate> {
        let mut out: Vec<(f32, SoundCandidate)> = self
            .find_phonemes(&wanted.symbols)
            .into_iter()
            .map(|hit| {
                let word = &self.transcripts[&hit.source].words[hit.word_index];
                let at = word.phonemes.iter().position(|p| p.start == hit.start).unwrap_or(0);
                let after_index = at + wanted.symbols.len();
                let same_before = wanted.before.is_some() && at > 0 && word.phonemes[at - 1].symbol == *wanted.before.as_deref().unwrap_or("");
                let same_after = wanted.after.is_some() && word.phonemes.get(after_index).is_some_and(|p| p.symbol == *wanted.after.as_deref().unwrap_or(""));
                let mut score = hit.confidence * 0.3 + same_before as u8 as f32 * 0.5 + same_after as u8 as f32 * 0.5;
                if let (Some(a), Some(b)) = (wanted.pitch_hz, hit.pitch_hz) {
                    score -= (12.0 * (b / a).log2()).abs() / 12.0 * 0.5;
                }
                if let Some(length) = wanted.length {
                    score -= ((hit.end - hit.start).max(1e-3) / length.max(1e-3)).ln().abs() as f32 * 0.3;
                }
                (score, SoundCandidate { hit, same_before, same_after })
            })
            .collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.into_iter().map(|(_, c)| c).collect()
    }

    fn find_rhymes(&self, word: &str) -> Vec<Hit> {
        let phones = g2p::phonemes(word);
        let target = g2p::rhyme_part(&phones);
        if target.is_empty() {
            return Vec::new();
        }
        let wanted = normalize_word(word);
        self.entries
            .iter()
            .filter(|e| e.normalized != wanted)
            .filter(|e| {
                let w = &self.transcripts[&e.source].words[e.word_index];
                let symbols: Vec<String> = w.phonemes.iter().map(|p| p.symbol.clone()).collect();
                g2p::rhyme_part(&symbols) == target
            })
            .map(|e| self.word_hit(e, HitKind::Rhyme))
            .collect()
    }
}

/// Sort key: lower is better. Match quality first, then confidence.
fn rank(h: &Hit) -> f32 {
    let tier = match h.kind {
        HitKind::Exact | HitKind::Phrase => 0.0,
        HitKind::Homophone => 1.0,
        HitKind::Phonemes | HitKind::Rhyme => 1.0,
        HitKind::Prefix => 2.0,
        HitKind::Contains => 3.0,
        HitKind::Fuzzy => 4.0,
    };
    tier - h.confidence * 0.9
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let next = (prev + cost).min(row[j] + 1).min(row[j + 1] + 1);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{Phoneme, Word};

    pub(crate) fn word(text: &str, start: f64, confidence: f32) -> Word {
        let symbols = g2p::phonemes(text);
        let d = 0.3 / symbols.len().max(1) as f64;
        Word {
            text: text.to_string(),
            start,
            end: start + 0.3,
            confidence,
            phonemes: symbols
                .iter()
                .enumerate()
                .map(|(i, s)| Phoneme { symbol: s.clone(), start: start + i as f64 * d, end: start + (i + 1) as f64 * d })
                .collect(),
            pitch_hz: None,
        }
    }

    pub(crate) fn index(text: &str) -> WordIndex {
        let words = text.split_whitespace().enumerate().map(|(i, w)| word(w, i as f64, 0.9)).collect();
        let mut idx = WordIndex::default();
        idx.set_transcript(1, Transcript { words, model: "test".into(), before_lyrics: None });
        idx
    }

    #[test]
    fn exact_outranks_partial() {
        let idx = index("cathedral cat scat cat");
        let hits = idx.search("cat", 10);
        assert_eq!(hits[0].kind, HitKind::Exact);
        assert_eq!(hits[1].kind, HitKind::Exact);
        assert_eq!(hits.len(), 4);
    }

    #[test]
    fn homophones_are_found() {
        let idx = index("there is a sign");
        let hits = idx.search("their", 10);
        assert_eq!(hits[0].text, "there");
        assert_eq!(hits[0].kind, HitKind::Homophone);
    }

    #[test]
    fn punctuation_and_case_do_not_matter() {
        let idx = index("Hello, WORLD!");
        assert_eq!(idx.search("hello", 10).len(), 1);
        assert_eq!(idx.search("WORLD", 10).len(), 1);
    }

    #[test]
    fn near_misses_are_found_but_rank_last() {
        let idx = index("tonight tonite");
        let hits = idx.search("tonight", 10);
        assert_eq!(hits[0].text, "tonight");
        assert_eq!(hits[1].text, "tonite");
    }

    #[test]
    fn phoneme_search_finds_sounds_inside_words() {
        let idx = index("scatter fish");
        let hits = idx.search("/k ae t/", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].context.as_deref(), Some("scatter"));
        assert!(hits[0].start > 0.0 && hits[0].end < 0.3);
        assert_eq!(idx.search("/sh/", 10)[0].context.as_deref(), Some("fish"));
    }

    #[test]
    fn sounds_with_matching_neighbours_rank_first() {
        // Wanting /ae/ after /k/: "cat" has that, "hat" doesn't.
        let idx = index("hat cat");
        let wanted = SoundWanted { symbols: vec!["ae".into()], before: Some("k".into()), ..Default::default() };
        let found = idx.sound_candidates(&wanted);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].hit.context.as_deref(), Some("cat"));
        assert!(found[0].same_before && !found[1].same_before);
    }

    #[test]
    fn sounds_close_in_pitch_rank_first() {
        let mut a = word("hat", 0.0, 0.9);
        let mut b = word("mat", 1.0, 0.9);
        a.pitch_hz = Some(440.0);
        b.pitch_hz = Some(222.0);
        let mut idx = WordIndex::default();
        idx.set_transcript(1, Transcript { words: vec![a, b], model: "t".into(), before_lyrics: None });
        let wanted = SoundWanted { symbols: vec!["ae".into()], pitch_hz: Some(220.0), ..Default::default() };
        assert_eq!(idx.sound_candidates(&wanted)[0].hit.context.as_deref(), Some("mat"));
    }

    #[test]
    fn phrase_search_requires_consecutive_words() {
        let idx = index("never gonna give you up gonna never");
        let hits = idx.search("gonna give", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].start, hits[0].end), (1.0, 2.3));
        assert!(idx.search("give never", 10).is_empty());
    }

    #[test]
    fn rhyme_search() {
        let idx = index("hat dog mat cat");
        let hits = idx.search("~cat", 10);
        let mut texts: Vec<&str> = hits.iter().map(|h| h.text.as_str()).collect();
        texts.sort_unstable();
        assert_eq!(texts, ["hat", "mat"]);
    }

    #[test]
    fn empty_and_junk_queries_are_safe() {
        let idx = index("hello");
        assert!(idx.search("", 10).is_empty());
        assert!(idx.search("/", 10).is_empty());
        assert!(idx.search("~", 10).is_empty());
        assert!(idx.search("!!!", 10).is_empty());
    }

    #[test]
    fn higher_confidence_ranks_first() {
        let mut idx = WordIndex::default();
        let words = vec![word("go", 0.0, 0.2), word("go", 1.0, 0.95)];
        idx.set_transcript(1, Transcript { words, model: "t".into(), before_lyrics: None });
        assert_eq!(idx.search("go", 5)[0].start, 1.0);
    }
}
