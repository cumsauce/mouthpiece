//! Programs the app relies on but doesn't ship: ffmpeg, yt-dlp, Deno and uv. They're downloaded on
//! first need into the app's own folder, so that nobody has to install anything themselves.

use crate::transcribe::app_data_dir;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Comes with ffprobe, which yt-dlp also needs.
    Ffmpeg,
    YtDlp,
    /// The JavaScript runtime yt-dlp needs for YouTube.
    Deno,
    /// Sets up the Python that stem splitting runs in.
    Uv,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::YtDlp => "yt-dlp",
            Tool::Deno => "Deno",
            Tool::Uv => "uv",
        }
    }
}

pub fn tools_dir() -> PathBuf {
    app_data_dir().join("tools")
}

/// Call once at startup, so that the rest of the app can find what's been installed here.
pub fn init() {
    mp_model::video::set_tools_dir(tools_dir());
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Finds a program, including the usual places a GUI app's PATH leaves out.
pub fn which(name: &str) -> Option<PathBuf> {
    let binary = exe(name);
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from);
    let from_env = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    let in_home = home.iter().flat_map(|h| [h.join(".local/bin"), h.join(".cargo/bin")]).collect::<Vec<_>>();
    from_env.into_iter().chain(usual).chain(in_home).map(|dir| dir.join(&binary)).find(|p| p.is_file())
}

/// Where a tool is. yt-dlp and Deno always use the app's own copies, because this app keeps them up to
/// date, and an old system copy is the commonest reason YouTube downloads stop working.
pub fn locate(tool: Tool) -> Option<PathBuf> {
    let own = |name: &str| Some(tools_dir().join(exe(name))).filter(|p| p.is_file());
    match tool {
        Tool::YtDlp => std::env::var_os("MOUTHPIECE_YTDLP").map(PathBuf::from).or_else(|| own("yt-dlp")),
        Tool::Deno => own("deno"),
        Tool::Uv => own("uv").or_else(|| which("uv")),
        Tool::Ffmpeg => {
            let ffmpeg = mp_model::video::find_ffmpeg();
            // A test's stand-in has no ffprobe beside it, and doesn't need one.
            let complete = |p: &PathBuf| std::env::var_os("MOUTHPIECE_FFMPEG").is_some() || p.with_file_name(exe("ffprobe")).is_file();
            ffmpeg.filter(complete).or_else(|| own("ffmpeg").filter(complete))
        }
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" }
}

fn triple() -> String {
    let system = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(windows) {
        "pc-windows-msvc"
    } else {
        "unknown-linux-gnu"
    };
    format!("{}-{system}", arch())
}

fn download(url: &str, to: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let response = ureq::get(url).call().map_err(|e| format!("Couldn't download {url}: {e}"))?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; 1 << 16];
    let mut written = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(to);
            return Err("cancelled".to_string());
        }
        let n = reader.read(&mut buffer).map_err(|e| format!("Couldn't download {url}: {e}"))?;
        if n == 0 {
            return Ok(());
        }
        file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        written += n as u64;
        if let Some(total) = total {
            on_progress(written as f32 / total as f32);
        }
    }
}

fn make_runnable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Takes the named programs out of a zip, wherever they sit inside it.
fn unzip(archive: &Path, names: &[String], into: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Bad download: {e}"))?;
    let mut found = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = Path::new(entry.name()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if entry.is_file() && names.contains(&name) {
            place(&mut entry, &into.join(&name))?;
            found += 1;
        }
    }
    if found < names.len() {
        return Err(format!("The download didn't contain {}", names.join(" and ")));
    }
    Ok(())
}

/// Writes a file beside its destination first, so that an interrupted install leaves nothing half done.
fn place(source: &mut dyn Read, target: &Path) -> Result<(), String> {
    let partial = target.with_extension("partial");
    let mut out = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    std::io::copy(source, &mut out).map_err(|e| e.to_string())?;
    drop(out);
    make_runnable(&partial)?;
    std::fs::rename(&partial, target).map_err(|e| e.to_string())
}

fn ffmpeg_urls() -> Vec<String> {
    if cfg!(windows) {
        return vec!["https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip".to_string()];
    }
    let system = if cfg!(target_os = "macos") { "macos" } else { "linux" };
    let cpu = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    ["ffmpeg", "ffprobe"].iter().map(|tool| format!("https://ffmpeg.martin-riedl.de/redirect/latest/{system}/{cpu}/release/{tool}.zip")).collect()
}

fn yt_dlp_url() -> &'static str {
    if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else if cfg!(windows) {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    }
}

fn install(tool: Tool, cancel: &AtomicBool, on_progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let dir = tools_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let scratch = dir.join(format!("{}.download", tool.label()));
    let result = (|| match tool {
        Tool::YtDlp => {
            download(yt_dlp_url(), &scratch, cancel, on_progress)?;
            let mut file = std::fs::File::open(&scratch).map_err(|e| e.to_string())?;
            place(&mut file, &dir.join(exe("yt-dlp")))
        }
        Tool::Deno => {
            download(&format!("https://github.com/denoland/deno/releases/latest/download/deno-{}.zip", triple()), &scratch, cancel, on_progress)?;
            unzip(&scratch, &[exe("deno")], &dir)
        }
        Tool::Uv if cfg!(windows) => {
            download(&format!("https://github.com/astral-sh/uv/releases/latest/download/uv-{}.zip", triple()), &scratch, cancel, on_progress)?;
            unzip(&scratch, &[exe("uv")], &dir)
        }
        Tool::Uv => {
            download(&format!("https://github.com/astral-sh/uv/releases/latest/download/uv-{}.tar.gz", triple()), &scratch, cancel, on_progress)?;
            let decoder = flate2::read::GzDecoder::new(std::fs::File::open(&scratch).map_err(|e| e.to_string())?);
            let mut archive = tar::Archive::new(decoder);
            for entry in archive.entries().map_err(|e| e.to_string())? {
                let mut entry = entry.map_err(|e| e.to_string())?;
                if entry.path().ok().and_then(|p| p.file_name().map(|n| n == "uv")).unwrap_or(false) {
                    return place(&mut entry, &dir.join("uv"));
                }
            }
            Err("The download didn't contain uv".to_string())
        }
        Tool::Ffmpeg => {
            let urls = ffmpeg_urls();
            let count = urls.len() as f32;
            for (i, url) in urls.iter().enumerate() {
                download(url, &scratch, cancel, &mut |p| on_progress((i as f32 + p) / count))?;
                let names = if urls.len() == 1 {
                    vec![exe("ffmpeg"), exe("ffprobe")]
                } else {
                    vec![exe(if i == 0 { "ffmpeg" } else { "ffprobe" })]
                };
                unzip(&scratch, &names, &dir)?;
            }
            mp_model::video::forget_ffmpeg();
            Ok(())
        }
    })();
    let _ = std::fs::remove_file(&scratch);
    result
}

/// Installs whichever of these aren't here yet. `on_progress` receives what's happening and 0..=1.
pub fn ensure(tools: &[Tool], cancel: &AtomicBool, on_progress: &mut dyn FnMut(&str, f32)) -> Result<(), String> {
    for tool in tools {
        if locate(*tool).is_none() {
            let label = format!("Getting {}", tool.label());
            on_progress(&label, 0.0);
            install(*tool, cancel, &mut |p| on_progress(&label, p))?;
        }
    }
    Ok(())
}

pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unzip_finds_programs_inside_folders_and_notices_missing_ones() {
        let dir = std::env::temp_dir().join(format!("mp-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("a.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("ffmpeg-build/bin/ffmpeg", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"program").unwrap();
        zip.finish().unwrap();
        unzip(&archive, &["ffmpeg".to_string()], &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("ffmpeg")).unwrap(), b"program");
        assert!(unzip(&archive, &["ffprobe".to_string()], &dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
