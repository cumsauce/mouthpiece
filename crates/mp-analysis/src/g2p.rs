//! Words to phonemes: CMUdict first, spelling rules for anything it doesn't know.

use std::collections::HashMap;
use std::sync::OnceLock;

static CMUDICT: &str = include_str!("../assets/cmudict.dict");

fn dictionary() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static DICT: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    DICT.get_or_init(|| {
        let mut map: HashMap<&'static str, Vec<&'static str>> = HashMap::with_capacity(140_000);
        for line in CMUDICT.lines() {
            let Some((word, pron)) = line.split_once(' ') else { continue };
            // Variant pronunciations are listed as "word(2)".
            let word = word.split('(').next().unwrap_or(word);
            // A trailing comment can follow the pronunciation.
            let pron = pron.split('#').next().unwrap_or(pron).trim();
            map.entry(word).or_default().push(pron);
        }
        map
    })
}

/// All known pronunciations as lower-case ARPAbet without stress marks.
pub fn pronunciations(word: &str) -> Vec<Vec<String>> {
    let w = word.to_lowercase();
    let found: Vec<Vec<String>> = dictionary()
        .get(w.as_str())
        .map(|list| list.iter().map(|p| parse_arpabet(p)).collect())
        .unwrap_or_default();
    if found.is_empty() {
        let guess = from_spelling(&w);
        if guess.is_empty() { Vec::new() } else { vec![guess] }
    } else {
        found
    }
}

/// The most common pronunciation.
pub fn phonemes(word: &str) -> Vec<String> {
    pronunciations(word).into_iter().next().unwrap_or_default()
}

fn parse_arpabet(p: &str) -> Vec<String> {
    p.split_whitespace()
        .map(|s| s.trim_end_matches(|c: char| c.is_ascii_digit()).to_lowercase())
        .collect()
}

/// Everything from the last vowel onward, the part of a word that rhymes. Works on phonemes.
pub fn rhyme_part(phonemes: &[String]) -> &[String] {
    let last_vowel = phonemes.iter().rposition(|p| class_of(p) == PhonemeClass::Vowel).unwrap_or(0);
    &phonemes[last_vowel..]
}

/// Rough letter-to-sound rules, for names and slang. Longest spelling match wins.
fn from_spelling(w: &str) -> Vec<String> {
    const RULES: &[(&str, &[&str])] = &[
        ("tch", &["ch"]), ("igh", &["ay"]), ("ough", &["ah", "f"]), ("tion", &["sh", "ah", "n"]),
        ("sh", &["sh"]), ("ch", &["ch"]), ("th", &["th"]), ("ph", &["f"]), ("wh", &["w"]), ("ck", &["k"]),
        ("ng", &["ng"]), ("qu", &["k", "w"]), ("ee", &["iy"]), ("ea", &["iy"]), ("oo", &["uw"]),
        ("ou", &["aw"]), ("ow", &["ow"]), ("oi", &["oy"]), ("oy", &["oy"]), ("ai", &["ey"]), ("ay", &["ey"]),
        ("au", &["ao"]), ("aw", &["ao"]), ("er", &["er"]), ("ir", &["er"]), ("ur", &["er"]), ("ar", &["aa", "r"]),
        ("or", &["ao", "r"]), ("a", &["ae"]), ("b", &["b"]), ("c", &["k"]), ("d", &["d"]), ("e", &["eh"]),
        ("f", &["f"]), ("g", &["g"]), ("h", &["hh"]), ("i", &["ih"]), ("j", &["jh"]), ("k", &["k"]), ("l", &["l"]),
        ("m", &["m"]), ("n", &["n"]), ("o", &["aa"]), ("p", &["p"]), ("r", &["r"]), ("s", &["s"]), ("t", &["t"]),
        ("u", &["ah"]), ("v", &["v"]), ("w", &["w"]), ("x", &["k", "s"]), ("y", &["iy"]), ("z", &["z"]),
    ];
    let mut out: Vec<String> = Vec::new();
    let mut rest = w;
    while !rest.is_empty() {
        // A silent final e after a consonant.
        if rest == "e" && !out.is_empty() {
            break;
        }
        let rule = RULES.iter().filter(|(s, _)| rest.starts_with(s)).max_by_key(|(s, _)| s.len());
        match rule {
            Some((s, ph)) => {
                for p in *ph {
                    if out.last().map(String::as_str) != Some(*p) {
                        out.push((*p).to_string());
                    }
                }
                rest = &rest[s.len()..];
            }
            None => {
                let mut chars = rest.chars();
                chars.next();
                rest = chars.as_str();
            }
        }
    }
    out
}

/// Acoustic class of a phoneme, which determines how the aligner expects it to sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhonemeClass {
    Vowel,
    /// w, y, r, l
    Glide,
    Nasal,
    VoicedFricative,
    UnvoicedFricative,
    VoicedStop,
    UnvoicedStop,
    Affricate,
}

impl PhonemeClass {
    pub const ALL: [PhonemeClass; 8] = [
        PhonemeClass::Vowel,
        PhonemeClass::Glide,
        PhonemeClass::Nasal,
        PhonemeClass::VoicedFricative,
        PhonemeClass::UnvoicedFricative,
        PhonemeClass::VoicedStop,
        PhonemeClass::UnvoicedStop,
        PhonemeClass::Affricate,
    ];
}

pub fn class_of(p: &str) -> PhonemeClass {
    use PhonemeClass::*;
    match p {
        "w" | "y" | "r" | "l" => Glide,
        "m" | "n" | "ng" => Nasal,
        "v" | "dh" | "z" | "zh" => VoicedFricative,
        "f" | "th" | "s" | "sh" | "hh" => UnvoicedFricative,
        "b" | "d" | "g" => VoicedStop,
        "p" | "t" | "k" => UnvoicedStop,
        "ch" | "jh" => Affricate,
        _ => Vowel,
    }
}

/// Every phoneme symbol, for validating `/.../` queries.
pub const ALL_PHONEMES: &[&str] = &[
    "aa", "ae", "ah", "ao", "aw", "ay", "b", "ch", "d", "dh", "eh", "er", "ey", "f", "g", "hh", "ih", "iy", "jh",
    "k", "l", "m", "n", "ng", "ow", "oy", "p", "r", "s", "sh", "t", "th", "uh", "uw", "v", "w", "y", "z", "zh",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn dictionary_words_and_variants() {
        assert_eq!(phonemes("cat"), s(&["k", "ae", "t"]));
        assert_eq!(phonemes("HELLO")[0], "hh");
        assert!(pronunciations("tomato").len() >= 2);
        assert_eq!(phonemes("'cause"), s(&["k", "ah", "z"]));
    }

    #[test]
    fn unknown_words_get_a_guess() {
        let guess = phonemes("zorblax");
        assert_eq!(guess.first().map(String::as_str), Some("z"));
        assert!(guess.len() >= 5);
    }

    #[test]
    fn every_dictionary_phoneme_is_known() {
        for p in phonemes("the quick brown fox jumps over thoughtful sheep") {
            assert!(ALL_PHONEMES.contains(&p.as_str()), "{p}");
        }
        for w in ["measure", "bottle", "ring", "voice", "chew", "jump", "yes"] {
            for p in phonemes(w) {
                assert!(ALL_PHONEMES.contains(&p.as_str()), "{p}");
            }
        }
    }

    #[test]
    fn rhymes_share_their_final_part() {
        assert_eq!(rhyme_part(&phonemes("cat")), rhyme_part(&phonemes("hat")));
        assert_ne!(rhyme_part(&phonemes("cat")), rhyme_part(&phonemes("cot")));
    }
}
