//! Uses a song's real lyrics to correct what Whisper heard. Whisper knows when words happen better than it
//! knows which words they are, especially in singing, and lyrics are the opposite. So the two are lined up:
//! the words come from the lyrics, the timing from the recording, and the aligner then fits them to the audio.

use crate::transcribe::{finish_transcript, RawSegment, RawWord};
use crate::{normalize_word, Transcript};
use serde::Deserialize;

/// Below this share of its words found in the lyrics, a recording isn't singing these lyrics.
const LEAST_AGREEMENT: f32 = 0.3;
/// The quickest believable word, for deciding if there's room for missing words.
const QUICKEST_WORD: f64 = 0.08;
const USUAL_WORD: f64 = 0.35;
const PHRASE_GAP: f64 = 1.0;
/// Words that only sound alike still count as the same word when lining up.
const ALIKE: f32 = 0.6;

#[derive(Clone, Debug, PartialEq)]
pub struct LyricLine {
    /// When the line starts in some recording of the song, if the lyrics are timed.
    pub time: Option<f64>,
    pub words: Vec<String>,
}

/// Reads plain or timed (LRC) lyrics. Section labels such as `[Chorus]` aren't sung, so they go.
pub fn parse(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut time = None;
        while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            time = time.or_else(|| parse_time(tag.0));
            rest = tag.1.trim_start();
        }
        let words: Vec<String> = rest.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).filter(|w| !normalize_word(w).is_empty()).collect();
        if !words.is_empty() {
            lines.push(LyricLine { time, words });
        }
    }
    lines
}

fn parse_time(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    Some(minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?)
}

fn similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = diagonal + usize::from(ca != cb);
            diagonal = row[j + 1];
            row[j + 1] = cost.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    1.0 - row[b.len()] as f32 / a.len().max(b.len()).max(1) as f32
}

/// Lines two word lists up, returning pairs of (heard, lyric) word positions in order.
fn line_up(heard: &[String], lyric: &[String]) -> Vec<(usize, usize)> {
    const GAP: f32 = -0.4;
    const SAME: f32 = 2.0;
    let (n, m) = (heard.len(), lyric.len());
    // Going from the end, so that the path can be read forwards. Either list may start or end with
    // words the other lacks, such as a verse that isn't in the recording.
    let mut from = vec![0u8; (n + 1) * (m + 1)];
    let mut next = vec![0f32; m + 1];
    let mut row = vec![0f32; m + 1];
    for i in (0..n).rev() {
        row[m] = 0.0;
        for j in (0..m).rev() {
            let s = similarity(&heard[i], &lyric[j]);
            let pair = if s >= ALIKE { next[j + 1] + SAME * s } else { f32::MIN };
            let (skip_heard, skip_lyric) = (next[j] + GAP, row[j + 1] + GAP);
            let best = pair.max(skip_heard).max(skip_lyric).max(0.0);
            row[j] = best;
            from[i * (m + 1) + j] = if best == 0.0 {
                0
            } else if best == pair {
                1
            } else if best == skip_heard {
                2
            } else {
                3
            };
        }
        std::mem::swap(&mut next, &mut row);
    }
    // The best place to start, since a start that isn't the very beginning costs nothing.
    let (mut i, mut j, mut best) = (0, 0, 0f32);
    let mut scores = vec![0f32; 0];
    scores.resize((n + 1) * (m + 1), 0.0);
    for a in (0..n).rev() {
        for b in (0..m).rev() {
            let s = similarity(&heard[a], &lyric[b]);
            let v = match from[a * (m + 1) + b] {
                1 => scores[(a + 1) * (m + 1) + b + 1] + SAME * s,
                2 => scores[(a + 1) * (m + 1) + b] + GAP,
                3 => scores[a * (m + 1) + b + 1] + GAP,
                _ => 0.0,
            };
            scores[a * (m + 1) + b] = v;
            if v >= best {
                best = v;
                i = a;
                j = b;
            }
        }
    }
    let mut pairs = Vec::new();
    while i < n && j < m {
        match from[i * (m + 1) + j] {
            1 => {
                pairs.push((i, j));
                i += 1;
                j += 1;
            }
            2 => i += 1,
            3 => j += 1,
            _ => break,
        }
    }
    pairs
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LyricsReport {
    /// Words the recording and lyrics agreed on.
    pub confirmed: usize,
    /// Words changed to what the lyrics say.
    pub replaced: usize,
    /// Words Whisper missed altogether.
    pub added: usize,
    /// Lyrics that couldn't be placed, usually for not being in this recording.
    pub left_out: usize,
}

/// One word as it'll be aligned.
#[derive(Clone, Debug, PartialEq)]
struct Planned {
    text: String,
    anchor: f64,
    confidence: f32,
}

/// A word as Whisper placed it.
#[derive(Clone, Debug, PartialEq)]
pub struct HeardWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
}

fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

/// Works out the corrected words from what was heard and the lyrics, without touching audio.
pub fn plan(heard: &[HeardWord], lines: &[LyricLine]) -> Result<(Vec<RawSegment>, LyricsReport), String> {
    let lyric: Vec<(String, usize)> = lines.iter().enumerate().flat_map(|(l, line)| line.words.iter().map(move |w| (w.clone(), l))).collect();
    let heard_norm: Vec<String> = heard.iter().map(|w| normalize_word(&w.text)).collect();
    let lyric_norm: Vec<String> = lyric.iter().map(|(w, _)| normalize_word(w)).collect();
    let pairs = line_up(&heard_norm, &lyric_norm);
    let agreeing = pairs.iter().filter(|(h, l)| heard_norm[*h] == lyric_norm[*l]).count();
    if heard.is_empty() || (agreeing as f32) < heard.len() as f32 * LEAST_AGREEMENT {
        return Err("These lyrics don't seem to be what's sung here, so nothing was changed.".to_string());
    }

    // Timed lyrics are often from a slightly different recording, so measure how far off they run.
    let mut offsets = Vec::new();
    for (h, l) in &pairs {
        let line = lyric[*l].1;
        if lines[line].words.len() >= 2 && lyric.iter().position(|(_, x)| *x == line) == Some(*l) {
            if let Some(time) = lines[line].time {
                offsets.push(time - heard[*h].start);
            }
        }
    }
    const LEAST_TIMED_LINES: usize = 3;
    let offset = if offsets.len() >= LEAST_TIMED_LINES { median(&mut offsets) } else { None };

    let mut report = LyricsReport::default();
    let mut planned: Vec<Planned> = Vec::new();
    let anchor = |w: &HeardWord| (w.start + w.end) * 0.5;
    let total_end = heard.last().map_or(0.0, |w| w.end);
    // Walks the stretches between words that agree. `previous` is the last pair, and `next` the coming one.
    let boundaries: Vec<Option<(usize, usize)>> = std::iter::once(None).chain(pairs.iter().copied().map(Some)).chain(std::iter::once(None)).collect();
    for window in boundaries.windows(2) {
        let (previous, next) = (window[0], window[1]);
        let heard_run = previous.map_or(0, |p| p.0 + 1)..next.map_or(heard.len(), |n| n.0);
        let lyric_run = previous.map_or(0, |p| p.1 + 1)..next.map_or(lyric.len(), |n| n.1);
        let earliest = previous.map_or(0.0, |p| heard[p.0].end);
        let latest = next.map_or(total_end, |n| heard[n.0].start);
        fill_gap(heard, &lyric, lines, heard_run, lyric_run, (earliest, latest), offset, (previous.is_some(), next.is_some()), &mut planned, &mut report);
        if let Some((h, l)) = next {
            let replaced = heard_norm[h] != lyric_norm[l];
            report.confirmed += usize::from(!replaced);
            report.replaced += usize::from(replaced);
            planned.push(Planned { text: lyric[l].0.clone(), anchor: anchor(&heard[h]), confidence: heard[h].confidence.max(if replaced { 0.0 } else { 0.9 }) });
        }
    }
    planned.sort_by(|a, b| a.anchor.total_cmp(&b.anchor));

    let mut segments: Vec<RawSegment> = Vec::new();
    for word in planned {
        let raw = RawWord { text: word.text, anchor: word.anchor, confidence: word.confidence };
        match segments.last_mut() {
            Some(segment) if word.anchor - segment.end < PHRASE_GAP => {
                segment.end = word.anchor;
                segment.words.push(raw);
            }
            _ => segments.push(RawSegment { start: word.anchor, end: word.anchor, words: vec![raw] }),
        }
    }
    Ok((segments, report))
}

/// Decides what happens between two words that agree: lyrics that replace what was heard, lyrics that were
/// missed, or things that were heard and aren't in the lyrics, such as ad-libs.
#[allow(clippy::too_many_arguments)]
fn fill_gap(
    heard: &[HeardWord],
    lyric: &[(String, usize)],
    lines: &[LyricLine],
    heard_run: std::ops::Range<usize>,
    lyric_run: std::ops::Range<usize>,
    (earliest, latest): (f64, f64),
    offset: Option<f64>,
    (has_previous, has_next): (bool, bool),
    planned: &mut Vec<Planned>,
    report: &mut LyricsReport,
) {
    const MOST_HEARD_PER_LYRIC: usize = 3;
    let keep_heard = |planned: &mut Vec<Planned>| {
        for w in &heard[heard_run.clone()] {
            planned.push(Planned { text: w.text.clone(), anchor: (w.start + w.end) * 0.5, confidence: w.confidence });
        }
    };
    if lyric_run.is_empty() {
        keep_heard(planned);
        return;
    }
    let count = lyric_run.len();
    if !heard_run.is_empty() {
        // Both have words here. Only swap them if the amounts are believably the same words.
        let (from, to) = (heard[heard_run.start].start, heard[heard_run.end - 1].end);
        let room = (latest - earliest).max(to - from);
        if heard_run.len() > count * MOST_HEARD_PER_LYRIC || (count as f64) * QUICKEST_WORD > room {
            keep_heard(planned);
            report.left_out += count;
            return;
        }
        let spread = |i: usize| from + (to - from) * (i as f64 + 0.5) / count as f64;
        for (i, l) in lyric_run.enumerate() {
            planned.push(Planned { text: lyric[l].0.clone(), anchor: spread(i), confidence: 0.6 });
        }
        report.replaced += count;
        return;
    }
    // Words that were missed. Line by line, since each line may have its own time.
    let mut start = lyric_run.start;
    let mut clear_from = earliest;
    while start < lyric_run.end {
        let line = lyric[start].1;
        let end = (start..lyric_run.end).find(|i| lyric[*i].1 != line).unwrap_or(lyric_run.end);
        let words = end - start;
        let timed = offset.and_then(|o| lines[line].time.map(|t| t - o)).filter(|t| *t >= clear_from && *t < latest);
        let span = match timed {
            Some(t) => Some((t, (t + words as f64 * USUAL_WORD).min(latest))),
            // Without a time, the words must fill the gap, or they may not belong in it. This only
            // works between words that agree, as at either end there's no telling where they'd go.
            None if has_previous && has_next => {
                let gap = latest - clear_from;
                ((words as f64) * QUICKEST_WORD <= gap && gap <= (words as f64) * USUAL_WORD * 3.0).then_some((clear_from, latest))
            }
            None => None,
        };
        match span.filter(|(a, b)| (words as f64) * QUICKEST_WORD <= b - a) {
            Some((a, b)) => {
                let pace = (b - a) / words as f64;
                for (i, l) in (start..end).enumerate() {
                    planned.push(Planned { text: lyric[l].0.clone(), anchor: a + pace * (i as f64 + 0.5), confidence: 0.5 });
                }
                report.added += words;
                clear_from = b;
            }
            None => report.left_out += words,
        }
        start = end;
    }
}

/// Corrects a transcript with lyrics and fits the result to the audio. Keeps what it replaced, so that
/// it can be undone.
pub fn apply(transcript: &Transcript, lines: &[LyricLine], mono: &[f32], sample_rate: f32) -> Result<(Transcript, LyricsReport), String> {
    // Always from what was actually heard, so that trying other lyrics doesn't build on the last ones.
    let original = transcript.before_lyrics.as_ref().unwrap_or(&transcript.words);
    let heard: Vec<HeardWord> = original.iter().map(|w| HeardWord { text: w.text.clone(), start: w.start, end: w.end, confidence: w.confidence }).collect();
    let (segments, report) = plan(&heard, lines)?;
    let mut corrected = finish_transcript(segments, mono, sample_rate, &transcript.model);
    corrected.before_lyrics = Some(original.clone());
    Ok((corrected, report))
}

/// Lyrics for a song from the search.
#[derive(Clone, Debug, PartialEq)]
pub struct LyricsMatch {
    pub track: String,
    pub artist: String,
    pub duration: Option<f64>,
    /// Lines with times when available, otherwise plain.
    pub text: String,
    pub timed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibEntry {
    track_name: Option<String>,
    artist_name: Option<String>,
    duration: Option<f64>,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

/// Reads LRCLIB's answer, closest in length to the recording first, since that's usually the same version.
pub fn parse_search(json: &str, length: f64) -> Result<Vec<LyricsMatch>, String> {
    let entries: Vec<LrclibEntry> = serde_json::from_str(json).map_err(|e| format!("Unexpected answer from the lyrics search: {e}"))?;
    let mut matches: Vec<LyricsMatch> = entries
        .into_iter()
        .filter_map(|e| {
            let timed = e.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty());
            let text = if timed { e.synced_lyrics } else { e.plain_lyrics };
            let text = text.filter(|t| !t.trim().is_empty())?;
            Some(LyricsMatch { track: e.track_name.unwrap_or_default(), artist: e.artist_name.unwrap_or_default(), duration: e.duration, text, timed })
        })
        .collect();
    matches.sort_by(|a, b| {
        let off = |m: &LyricsMatch| m.duration.map_or(f64::MAX, |d| (d - length).abs());
        off(a).total_cmp(&off(b))
    });
    Ok(matches)
}

/// Searches LRCLIB, a free and open lyrics database. Only the words of the query leave this computer.
pub fn search(query: &str, length: f64) -> Result<Vec<LyricsMatch>, String> {
    // It's a free service that's often briefly busy, so one failure doesn't mean much.
    const ATTEMPTS: u32 = 4;
    const WAIT: std::time::Duration = std::time::Duration::from_millis(600);
    let attempt = || {
        ureq::get("https://lrclib.net/api/search")
            .query("q", query)
            .header("User-Agent", "Mouthpiece (https://github.com/cumsauce/mouthpiece)")
            .call()
            .map_err(|e| e.to_string())
            .and_then(|mut response| response.body_mut().read_to_string().map_err(|e| e.to_string()))
    };
    let mut last = attempt();
    for round in 1..ATTEMPTS {
        if last.is_ok() {
            break;
        }
        std::thread::sleep(WAIT * round);
        last = attempt();
    }
    match last {
        Ok(body) => parse_search(&body, length),
        Err(error) => {
            eprintln!("lyrics search failed: {error}");
            Err("The lyrics search is busy or can't be reached. Try again in a moment, or paste the lyrics in.".to_string())
        }
    }
}

/// A likely search for a recording, from its name. Bits like "(Official Video)" only get in the way.
pub fn guess_query(name: &str, parent_folder: Option<&str>) -> String {
    // A stem is called "Vocals", so its folder has the name of the song.
    let base = parent_folder.and_then(|f| f.strip_suffix(" stems")).unwrap_or(name);
    let mut out = String::new();
    let mut depth = 0;
    for c in base.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth as i32 - 1).max(0) as usize,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().filter(|w| *w != "-").collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words heard one after another, at a steady pace, with `None` for a silent stretch.
    fn heard(words: &[&str]) -> Vec<HeardWord> {
        words
            .iter()
            .enumerate()
            .filter(|(_, w)| !w.is_empty())
            .map(|(i, w)| HeardWord { text: w.to_string(), start: i as f64 * 0.4, end: i as f64 * 0.4 + 0.3, confidence: 0.5 })
            .collect()
    }

    fn lyric(text: &str) -> Vec<LyricLine> {
        parse(text)
    }

    fn words_of(segments: &[RawSegment]) -> Vec<String> {
        segments.iter().flat_map(|s| &s.words).map(|w| w.text.clone()).collect()
    }

    #[test]
    fn reads_plain_and_timed_lyrics_without_section_labels() {
        let lines = parse("[Chorus]\n[00:12.50] Hello, there world\n\nplain line\n[01:02.00]\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time, Some(12.5));
        assert_eq!(lines[0].words, ["Hello", "there", "world"]);
        assert_eq!(lines[1].time, None);
    }

    #[test]
    fn misheard_words_take_the_lyrics_and_keep_their_timing() {
        let got = heard(&["the", "kitty", "sat", "on", "the", "mat", "today"]);
        let (segments, report) = plan(&got, &lyric("the kitten sat on the mat today")).unwrap();
        assert_eq!(words_of(&segments), ["the", "kitten", "sat", "on", "the", "mat", "today"]);
        assert_eq!(report.replaced, 1);
        assert_eq!(report.confirmed, 6);
        let anchor = segments[0].words[1].anchor;
        assert!((anchor - 0.55).abs() < 0.01, "{anchor}");
    }

    #[test]
    fn words_that_sound_nothing_alike_are_still_corrected_between_good_ones() {
        let got = heard(&["we", "will", "bananas", "you", "tonight", "for", "sure"]);
        let (segments, _) = plan(&got, &lyric("we will rock you tonight for sure")).unwrap();
        assert_eq!(words_of(&segments), ["we", "will", "rock", "you", "tonight", "for", "sure"]);
    }

    #[test]
    fn ad_libs_that_are_not_in_the_lyrics_stay() {
        let got = heard(&["hello", "my", "yeah", "friend", "how", "are", "you"]);
        let (segments, report) = plan(&got, &lyric("hello my friend how are you")).unwrap();
        assert!(words_of(&segments).contains(&"yeah".to_string()));
        assert_eq!(report.confirmed, 6);
    }

    #[test]
    fn missed_words_are_added_where_there_is_room() {
        // A gap the length of three words, between words that agree.
        let got = heard(&["open", "your", "", "", "", "eyes", "and", "see"]);
        let (segments, report) = plan(&got, &lyric("open your big bright blue eyes and see")).unwrap();
        assert_eq!(words_of(&segments), ["open", "your", "big", "bright", "blue", "eyes", "and", "see"]);
        assert_eq!(report.added, 3);
    }

    #[test]
    fn lyrics_are_left_out_when_there_is_no_room_for_them() {
        let got = heard(&["open", "your", "eyes", "and", "see", "it", "all"]);
        let (segments, report) = plan(&got, &lyric("open your\none two three four five six seven\neyes and see it all")).unwrap();
        assert_eq!(words_of(&segments), ["open", "your", "eyes", "and", "see", "it", "all"]);
        assert_eq!(report.left_out, 7);
    }

    #[test]
    fn a_verse_that_was_missed_is_placed_from_its_timing() {
        let mut got = heard(&["one", "two", "three", "four"]);
        got.extend(["five", "six", "seven", "eight"].iter().enumerate().map(|(i, w)| HeardWord { text: w.to_string(), start: 20.0 + i as f64 * 0.4, end: 20.3 + i as f64 * 0.4, confidence: 0.5 }));
        got.extend(["nine", "ten"].iter().enumerate().map(|(i, w)| HeardWord { text: w.to_string(), start: 30.0 + i as f64 * 0.4, end: 30.3 + i as f64 * 0.4, confidence: 0.5 }));
        // These lyrics run 2 seconds later than this recording.
        let text = "[00:02.00] one two three four\n[00:12.00] missing words go here\n[00:22.00] five six seven eight\n[00:32.00] nine ten";
        let (segments, report) = plan(&got, &lyric(text)).unwrap();
        assert_eq!(report.added, 4);
        let all: Vec<&RawWord> = segments.iter().flat_map(|s| &s.words).collect();
        let missing = all.iter().find(|w| w.text == "missing").unwrap();
        assert!((missing.anchor - 10.0).abs() < 0.5, "offset wasn't corrected: {}", missing.anchor);
    }

    #[test]
    fn a_verse_that_was_missed_is_left_out_without_timing() {
        let mut got = heard(&["one", "two", "three", "four"]);
        got.extend(["five", "six", "seven"].iter().enumerate().map(|(i, w)| HeardWord { text: w.to_string(), start: 60.0 + i as f64 * 0.4, end: 60.3 + i as f64 * 0.4, confidence: 0.5 }));
        let (segments, report) = plan(&got, &lyric("one two three four\nmissing words\nfive six seven")).unwrap();
        assert_eq!(report.left_out, 2, "a minute of gap could be anything");
        assert_eq!(words_of(&segments).len(), 7);
    }

    #[test]
    fn the_wrong_song_changes_nothing() {
        let got = heard(&["completely", "different", "words", "from", "another", "song", "entirely"]);
        assert!(plan(&got, &lyric("nothing like the thing that was actually sung at all here")).is_err());
    }

    #[test]
    fn a_chorus_sung_twice_lines_up_with_both() {
        let got = heard(&["sing", "it", "loud", "verse", "words", "here", "sing", "it", "loud"]);
        let (_, report) = plan(&got, &lyric("sing it loud\nverse words here\nsing it loud")).unwrap();
        assert_eq!(report.confirmed, 9);
    }

    #[test]
    fn search_results_come_closest_in_length_first_and_skip_empty_ones() {
        let json = r#"[
            {"trackName":"Far","artistName":"A","duration":400.0,"plainLyrics":"la la","syncedLyrics":null},
            {"trackName":"Near","artistName":"A","duration":181.0,"plainLyrics":"la","syncedLyrics":"[00:01.00] la"},
            {"trackName":"Empty","artistName":"A","duration":180.0,"plainLyrics":"","syncedLyrics":null}
        ]"#;
        let found = parse_search(json, 180.0).unwrap();
        assert_eq!(found.iter().map(|m| m.track.as_str()).collect::<Vec<_>>(), ["Near", "Far"]);
        assert!(found[0].timed);
        assert!(!found[1].timed);
    }

    #[test]
    fn a_good_search_is_guessed_from_names() {
        assert_eq!(guess_query("Artist - Song (Official Video) [abc123]", None), "Artist Song");
        assert_eq!(guess_query("Vocals", Some("Artist - Song [abc123] stems")), "Artist Song");
        assert_eq!(guess_query("Plain", Some("Some folder")), "Plain");
    }
}
