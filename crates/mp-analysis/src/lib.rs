//! Everything that turns raw audio into knowledge: decoding, waveforms, tempo, key, words and
//! phonemes, plus search over them. All of it runs on background threads.

pub mod align;
pub mod decode;
pub mod g2p;
pub mod key;
pub mod peaks;
pub mod search;
pub mod sentence;
pub mod lyrics;
pub mod online;
pub mod spectrogram;
pub mod stems;
pub mod tools;
pub mod tempo;
pub mod transcribe;
pub mod video;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phoneme {
    /// Lower-case ARPAbet without stress, such as "ae".
    pub symbol: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// As transcribed, with punctuation stripped.
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// Transcription confidence in 0..=1.
    pub confidence: f32,
    pub phonemes: Vec<Phoneme>,
    /// Median pitch, if the word is clearly pitched.
    pub pitch_hz: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub words: Vec<Word>,
    pub model: String,
    /// The words as heard, if lyrics have corrected them. See `lyrics.rs`.
    #[serde(default)]
    pub before_lyrics: Option<Vec<Word>>,
}

/// Lower-cases and strips everything but letters, digits and inner apostrophes.
pub fn normalize_word(text: &str) -> String {
    let lower = text.to_lowercase().replace('’', "'");
    lower
        .trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect()
}
