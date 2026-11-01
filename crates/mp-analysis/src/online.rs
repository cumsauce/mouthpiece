//! Finds songs online and downloads them, using yt-dlp. Sites change often and yt-dlp keeps up, so
//! it updates itself separately from the app: at most daily, and again if something stops working.

use crate::tools::{self, hide_console, Tool};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const SEARCH_RESULTS: usize = 12;
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const NEEDED: [Tool; 3] = [Tool::Ffmpeg, Tool::Deno, Tool::YtDlp];

#[derive(Clone, Debug, PartialEq)]
pub struct SongResult {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<f64>,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DownloadKind {
    #[default]
    Audio,
    Video,
}

/// What a query means: a link to go straight to, or words to search for.
fn target(query: &str) -> String {
    let query = query.trim();
    if query.starts_with("http://") || query.starts_with("https://") {
        query.to_string()
    } else {
        format!("ytsearch{SEARCH_RESULTS}:{query}")
    }
}

/// Reads yt-dlp's JSON: a list of `entries` for a search, or a single video for a link.
pub fn parse_results(json: &str) -> Vec<SongResult> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let one = |v: &serde_json::Value| {
        let id = v["id"].as_str()?.to_string();
        let url = v["webpage_url"].as_str().or(v["url"].as_str())?.to_string();
        Some(SongResult {
            id,
            url,
            title: v["title"].as_str().unwrap_or("Untitled").to_string(),
            channel: v["channel"].as_str().or(v["uploader"].as_str()).unwrap_or("").to_string(),
            duration: v["duration"].as_f64(),
        })
    };
    match value["entries"].as_array() {
        Some(entries) => entries.iter().filter_map(one).collect(),
        None => one(&value).into_iter().collect(),
    }
}

/// Says what went wrong in plain words, from what yt-dlp last said.
fn explain(log: &str) -> String {
    let lower = log.to_lowercase();
    if lower.contains("sign in") || lower.contains("not a bot") {
        "YouTube wants a signed-in browser for this one. Try another result.".to_string()
    } else if lower.contains("private video") || lower.contains("video unavailable") || lower.contains("not available") {
        "That one isn't available. Try another result.".to_string()
    } else if lower.contains("getaddrinfo") || lower.contains("unable to download webpage") || lower.contains("timed out") {
        "Couldn't reach the site. Check your connection.".to_string()
    } else {
        log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("yt-dlp failed").trim_start_matches("ERROR: ").to_string()
    }
}

fn percent_in(line: &str) -> Option<f32> {
    let end = line.find('%')?;
    let start = line[..end].rfind(|c: char| !c.is_ascii_digit() && c != '.').map_or(0, |i| i + 1);
    line[start..end].parse().ok()
}

/// Runs yt-dlp to the end, passing it the runtime and ffmpeg it needs, and returns what it printed.
fn run(args: &[String], cancel: &AtomicBool, on_line: &mut dyn FnMut(&str)) -> Result<String, String> {
    let program = tools::locate(Tool::YtDlp).ok_or("yt-dlp isn't installed")?;
    let mut command = Command::new(program);
    if let Some(deno) = tools::locate(Tool::Deno) {
        command.arg("--js-runtimes").arg(format!("deno:{}", deno.display()));
    }
    if let Some(ffmpeg) = tools::locate(Tool::Ffmpeg) {
        command.arg("--ffmpeg-location").arg(ffmpeg);
    }
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|e| format!("yt-dlp wouldn't start: {e}"))?;
    let mut stderr = child.stderr.take();
    let log = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(s) = &mut stderr {
            let _ = s.read_to_string(&mut text);
        }
        text
    });
    let (lines, incoming) = std::sync::mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = lines.send(line);
            }
        });
    }
    let mut output = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".to_string());
        }
        match incoming.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                on_line(&line);
                output.push_str(&line);
                output.push('\n');
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(output)
    } else {
        Err(explain(&log.join().unwrap_or_default()))
    }
}

fn update_marker() -> PathBuf {
    tools::tools_dir().join("yt-dlp-updated")
}

/// Updates yt-dlp. Unless `now`, only if it hasn't been done for a while. A copy that's only stale keeps
/// working, so failing to update isn't an error unless it was asked for.
pub fn update(now: bool, cancel: &AtomicBool) -> Result<(), String> {
    // A stand-in used by tests isn't this app's to update.
    if std::env::var_os("MOUTHPIECE_YTDLP").is_some() {
        return Ok(());
    }
    let last = std::fs::metadata(update_marker()).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    if !now && last.elapsed().is_ok_and(|age| age < UPDATE_EVERY) {
        return Ok(());
    }
    // Nightly is what yt-dlp recommends for regular use, since it gets fixes for site changes first.
    let result = run(&["--update-to".to_string(), "nightly".to_string()], cancel, &mut |_| {});
    if result.is_ok() {
        let _ = std::fs::write(update_marker(), "");
    } else if let Err(error) = &result {
        eprintln!("could not update yt-dlp: {error}");
    }
    result.map(|_| ())
}

/// Gets everything ready: installs what's missing, and freshens yt-dlp.
pub fn prepare(cancel: &AtomicBool, on_progress: &mut dyn FnMut(&str, f32)) -> Result<(), String> {
    // A stand-in used by tests brings what it needs.
    if std::env::var_os("MOUTHPIECE_YTDLP").is_none() {
        tools::ensure(&NEEDED, cancel, on_progress)?;
    }
    let _ = update(false, cancel);
    if cancel.load(Ordering::Relaxed) { Err("cancelled".to_string()) } else { Ok(()) }
}

/// Tries once, and if it fails, updates yt-dlp and tries again, as a site change is the usual cause.
fn with_retry<T>(cancel: &AtomicBool, mut attempt: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    static RETRIED: AtomicBool = AtomicBool::new(false);
    match attempt() {
        Err(error) if error != "cancelled" && !RETRIED.swap(true, Ordering::Relaxed) && update(true, cancel).is_ok() => attempt(),
        other => other,
    }
}

pub fn search(query: &str, cancel: &AtomicBool) -> Result<Vec<SongResult>, String> {
    let args = vec!["--flat-playlist".to_string(), "--no-playlist".to_string(), "--no-warnings".to_string(), "-J".to_string(), target(query)];
    with_retry(cancel, || run(&args, cancel, &mut |_| {}).map(|json| parse_results(&json)))
}

/// A file already downloaded from this video, which saves doing it again.
pub fn existing_download(dir: &Path, id: &str, kind: DownloadKind) -> Option<PathBuf> {
    let extension = match kind {
        DownloadKind::Audio => "wav",
        DownloadKind::Video => "mp4",
    };
    let tag = format!("[{id}]");
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        // An interrupted download leaves a ".temp" file, which isn't a finished song.
        let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        p.extension().is_some_and(|e| e == extension) && name.contains(&tag) && !name.contains(".temp.")
    })
}

fn download_args(url: &str, dir: &Path, kind: DownloadKind) -> Vec<String> {
    let mut args: Vec<String> = ["--no-playlist", "--no-warnings", "--windows-filenames", "--newline", "--progress", "--no-simulate"].iter().map(|a| a.to_string()).collect();
    args.extend(["--progress-template".to_string(), "download:mp-progress %(progress._percent_str)s".to_string()]);
    args.extend(["--print".to_string(), "after_move:filepath".to_string()]);
    args.extend(["-o".to_string(), dir.join("%(title).120B [%(id)s].%(ext)s").display().to_string()]);
    match kind {
        // Audio is kept lossless from here on, since it'll be separated and cut.
        DownloadKind::Audio => args.extend(["-f", "bestaudio/best", "-x", "--audio-format", "wav"].iter().map(|a| a.to_string())),
        DownloadKind::Video => args.extend(["-f", "bv*+ba/b", "-S", "res:1080,vcodec:h264,acodec:m4a", "--merge-output-format", "mp4"].iter().map(|a| a.to_string())),
    }
    // A video id can start with a dash, which mustn't be read as an option.
    args.push("--".to_string());
    args.push(url.to_string());
    args
}

/// Downloads a song into `dir`, and returns its file.
pub fn download(result: &SongResult, kind: DownloadKind, dir: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f32)) -> Result<PathBuf, String> {
    if let Some(done) = existing_download(dir, &result.id, kind) {
        return Ok(done);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let args = download_args(&result.url, dir, kind);
    let output = with_retry(cancel, || {
        run(&args, cancel, &mut |line| {
            if line.starts_with("mp-progress") {
                if let Some(percent) = percent_in(line) {
                    on_progress(percent / 100.0);
                }
            }
        })
    })?;
    output
        .lines()
        .rev()
        .find(|l| !l.starts_with("mp-progress") && !l.trim().is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| "yt-dlp didn't say where it saved the file".to_string())
}

/// A downloaded file's name without the video id that keeps it unique on disk.
pub fn display_name(file_stem: &str) -> &str {
    const SHORTEST_ID: usize = 6;
    match file_stem.strip_suffix(']').and_then(|s| s.rsplit_once(" [")) {
        Some((name, id)) if id.len() >= SHORTEST_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') => name,
        _ => file_stem,
    }
}

/// Where downloads go by default: a folder someone can find, since projects keep using these files.
pub fn default_download_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map_or_else(std::env::temp_dir, PathBuf::from);
    home.join("Music").join("Mouthpiece")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_lists_its_entries_and_a_link_gives_one() {
        let search = r#"{"entries":[{"id":"a","title":"One","channel":"C","duration":61.0,"url":"https://x/a"},{"id":"b","title":"Two","uploader":"U","url":"https://x/b"},{"title":"no id"}]}"#;
        let results = parse_results(search);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].duration, Some(61.0));
        assert_eq!(results[1].channel, "U");
        let link = parse_results(r#"{"id":"c","title":"Three","webpage_url":"https://x/c"}"#);
        assert_eq!(link[0].url, "https://x/c");
        assert!(parse_results("not json").is_empty());
    }

    #[test]
    fn video_ids_are_left_out_of_names() {
        assert_eq!(display_name("Big Buck Bunny trailer [yUQM7H4Swgw]"), "Big Buck Bunny trailer");
        assert_eq!(display_name("Song [live]"), "Song [live]");
        assert_eq!(display_name("Plain"), "Plain");
    }

    #[test]
    fn links_go_straight_there_and_words_are_searched() {
        assert_eq!(target(" https://youtu.be/x "), "https://youtu.be/x");
        assert_eq!(target("never gonna"), format!("ytsearch{SEARCH_RESULTS}:never gonna"));
    }

    #[test]
    fn errors_are_explained_in_plain_words() {
        assert!(explain("ERROR: Sign in to confirm you're not a bot").contains("signed-in"));
        assert_eq!(explain("something\nERROR: odd failure\n"), "odd failure");
    }

    #[test]
    fn progress_is_read_with_or_without_padding() {
        assert_eq!(percent_in("mp-progress  42.5%"), Some(42.5));
        assert_eq!(percent_in("mp-progress 100%"), Some(100.0));
        assert_eq!(percent_in("mp-progress"), None);
    }

    #[test]
    fn a_previous_download_is_found_by_its_id_and_kind() {
        let dir = std::env::temp_dir().join(format!("mp-online-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Half [abc].temp.wav"), b"x").unwrap();
        assert!(existing_download(&dir, "abc", DownloadKind::Audio).is_none(), "an interrupted download doesn't count");
        std::fs::write(dir.join("Song [abc].wav"), b"x").unwrap();
        assert!(existing_download(&dir, "abc", DownloadKind::Audio).is_some());
        assert!(existing_download(&dir, "abc", DownloadKind::Video).is_none());
        assert!(existing_download(&dir, "other", DownloadKind::Audio).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
