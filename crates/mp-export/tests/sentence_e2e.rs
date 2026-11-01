//! The whole product in one test: real speech is transcribed, a new sentence is built from its
//! words, the result is rendered to MP3, and that MP3 is transcribed to check what it says.
//!
//! Needs the macOS `say` command and the base.en speech model, so it's opt-in:
//! `cargo test -p mp-export --test sentence_e2e -- --ignored --nocapture`

use mp_analysis::decode::decode_file;
use mp_analysis::search::WordIndex;
use mp_analysis::sentence::{plan, PartSource};
use mp_analysis::transcribe::{transcribe, TranscribeRequest, MODELS};
use mp_analysis::{normalize_word, Transcript};
use mp_engine::{GraphCompiler, RenderCache, SourceBank};
use mp_export::{export, ExportSettings, Format};
use mp_model::{Project, SourceId, TrackKind};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const SR: u32 = 48000;
const SPEECH: &str = "Hello world. This is a test of the sentence mixer. Never gonna give you up, the quick brown fox \
                      jumps over the lazy dog. We can say anything we want today. People love music and dancing all night long.";

fn run_whisper(path: &std::path::Path) -> Transcript {
    let audio = decode_file(path, SR).unwrap();
    let model = *MODELS.iter().find(|m| m.id == "base.en").unwrap();
    assert!(model.is_installed(), "install the base.en model in the app first");
    let request = TranscribeRequest { mono: Arc::new(audio.mono()), sample_rate: SR as f32, model, language: Some("en".into()) };
    transcribe(&request, |_| {}, Arc::new(AtomicBool::new(false))).unwrap()
}

fn words_of(t: &Transcript) -> Vec<String> {
    t.words.iter().map(|w| normalize_word(&w.text)).collect()
}

/// Fraction of `wanted` found in order in `heard`.
fn in_order_score(wanted: &[String], heard: &[String]) -> f32 {
    let mut lcs = vec![vec![0usize; heard.len() + 1]; wanted.len() + 1];
    for (i, w) in wanted.iter().enumerate() {
        for (j, h) in heard.iter().enumerate() {
            lcs[i + 1][j + 1] = if w == h { lcs[i][j] + 1 } else { lcs[i][j + 1].max(lcs[i + 1][j]) };
        }
    }
    lcs[wanted.len()][heard.len()] as f32 / wanted.len() as f32
}

fn build_and_hear(sentence: &str, index: &WordIndex, audio: Arc<mp_dsp::Stereo>, duration: f64, dir: &std::path::Path, file: &str) -> Vec<String> {
    let mut project = Project::default();
    let source = project.add_source("v".into(), "v".into(), duration, TrackKind::Vocal);
    let track = project.add_track("Sentence", TrackKind::Vocal);
    let mut bank = SourceBank::default();
    bank.buffers.insert(source, audio);
    let mut cursor = 0.3;
    for part in plan(sentence, index) {
        assert_ne!(part.source, PartSource::Missing, "{}", part.text);
        for hit in part.pieces() {
            let clip = project.add_slice(track, source, cursor, hit.start, hit.end, None).unwrap();
            cursor = project.clip(clip).unwrap().end();
        }
        cursor += 0.12;
    }
    let graph = GraphCompiler::default().compile(&project, &bank, &RenderCache::new(), SR);
    let path = dir.join(file);
    let settings = ExportSettings { format: Format::Mp3 { kbps: 320 }, start: 0, end: ((cursor + 0.5) * SR as f64) as u64, tail_seconds: 0.0, title: "t".into() };
    assert!(export(graph, SR, &settings, &path, |_| true).unwrap());
    words_of(&run_whisper(&path))
}

#[test]
#[ignore]
fn a_built_sentence_says_what_was_typed() {
    let dir = std::env::temp_dir().join(format!("mouthpiece-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let aiff = dir.join("speech.aiff");
    assert!(Command::new("say").args(["-v", "Samantha", "-o"]).arg(&aiff).arg(SPEECH).status().unwrap().success());

    let transcript = run_whisper(&aiff);
    let wanted: Vec<String> = SPEECH.split_whitespace().map(normalize_word).collect();
    let recognition = in_order_score(&wanted, &words_of(&transcript));
    println!("recognition: {recognition:.2}");
    for w in &transcript.words {
        let ph: Vec<&str> = w.phonemes.iter().map(|p| p.symbol.as_str()).collect();
        println!("{:7.3} {:7.3} {:10} {}", w.start, w.end, w.text, ph.join(" "));
    }
    assert!(recognition > 0.9);

    let mut index = WordIndex::default();
    index.set_transcript(SourceId(1).0, transcript);
    let audio = Arc::new(decode_file(&aiff, SR).unwrap());
    let duration = audio.frames() as f64 / SR as f64;

    // Single words chosen so nothing here comes from a phrase, so every boundary gets tested.
    let mut total = 0.0;
    let sentences = [
        "the lazy dog love music all night today",
        "dog love sentence",
        "people say give test today",
        "brown music jumps over world",
        "we want quick dancing",
    ];
    for (i, sentence) in sentences.iter().enumerate() {
        let heard = build_and_hear(sentence, &index, audio.clone(), duration, &dir, &format!("s{i}.mp3"));
        let wanted: Vec<String> = sentence.split_whitespace().map(String::from).collect();
        let score = in_order_score(&wanted, &heard);
        println!("typed: {sentence}\nheard: {}\nscore: {score:.2}\n", heard.join(" "));
        total += score;
    }
    let average = total / sentences.len() as f32;
    println!("average: {average:.2}");
    std::fs::remove_dir_all(&dir).ok();
    assert!(average >= 0.8, "built sentences must say what was typed, got {average:.2}");
}
