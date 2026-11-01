//! Renders the arrangement's edit list to a video with ffmpeg. Each segment becomes its own short
//! file with an exact number of frames, then they're joined with the rendered mix.

use mp_model::video::{ffmpeg_path, SegmentPicture, VideoSegment};
use mp_model::SourceId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

const ENCODE: [&str; 8] = ["-an", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-y"];

fn frame_filter(picture: Option<&SegmentPicture>, s: &VideoSettings) -> String {
    let (w, h, fps) = (s.width, s.height, s.fps);
    let fit = format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p");
    match picture {
        Some(p) => {
            let reverse = if p.reversed { "reverse," } else { "" };
            // tpad holds the last frame when a segment runs on past its clip, across a short gap.
            format!("{reverse}setpts=(PTS-STARTPTS)*{:.6},fps={fps},{fit},tpad=stop_mode=clone:stop_duration=2", p.stretch)
        }
        None => fit,
    }
}

/// The ffmpeg arguments that render one segment.
pub fn segment_args(segment: &VideoSegment, path: Option<&Path>, s: &VideoSettings, out: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec!["-v".into(), "error".into()];
    match (&segment.picture, path) {
        (Some(p), Some(path)) => {
            // A clip shorter than a frame still needs a frame of input to show.
            let length = (p.src_end - p.src_start).max(1.0 / s.fps as f64);
            args.extend(["-ss".into(), format!("{:.6}", p.src_start), "-t".into(), format!("{length:.6}"), "-i".into(), path.display().to_string()]);
            args.extend(["-vf".into(), frame_filter(Some(p), s)]);
        }
        _ => {
            args.extend(["-f".into(), "lavfi".into(), "-i".into(), format!("color=c=black:s={}x{}:r={}", s.width, s.height, s.fps)]);
            args.extend(["-vf".into(), frame_filter(None, s)]);
        }
    }
    args.extend(["-frames:v".into(), segment.frames.to_string(), "-r".into(), s.fps.to_string()]);
    args.extend(ENCODE.iter().map(|a| a.to_string()));
    args.push(out.display().to_string());
    args
}

fn run(ffmpeg: &Path, args: &[String]) -> Result<(), String> {
    let output = Command::new(ffmpeg).args(args).stdin(Stdio::null()).output().map_err(|e| format!("could not run ffmpeg: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        Err(format!("ffmpeg failed: {}", text.lines().last().unwrap_or("no details")))
    }
}

/// Renders the edit list with `audio` as its sound, writing `out` only if it all works. `on_progress`
/// receives 0..=1, and returning false cancels. Returns Ok(false) if cancelled.
pub fn export_video(
    segments: &[VideoSegment],
    sources: &HashMap<SourceId, PathBuf>,
    audio: &Path,
    settings: &VideoSettings,
    out: &Path,
    mut on_progress: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let ffmpeg = ffmpeg_path().ok_or("ffmpeg isn't installed")?;
    let dir = std::env::temp_dir().join(format!("mouthpiece-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut list = String::new();
        for (i, segment) in segments.iter().enumerate() {
            if !on_progress(i as f32 / segments.len().max(1) as f32 * 0.95) {
                return Ok(false);
            }
            let file = dir.join(format!("seg{i:05}.mp4"));
            let path = segment.picture.as_ref().and_then(|p| sources.get(&p.source)).map(PathBuf::as_path);
            run(&ffmpeg, &segment_args(segment, path, settings, &file))?;
            list.push_str(&format!("file '{}'\n", file.display()));
        }
        let list_path = dir.join("list.txt");
        std::fs::write(&list_path, list).map_err(|e| e.to_string())?;
        let tmp = dir.join("joined.mp4");
        let args: Vec<String> = ["-v", "error", "-f", "concat", "-safe", "0", "-i"]
            .iter()
            .map(|a| a.to_string())
            .chain([list_path.display().to_string(), "-i".into(), audio.display().to_string()])
            .chain(["-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "256k", "-shortest", "-y"].iter().map(|a| a.to_string()))
            .chain([tmp.display().to_string()])
            .collect();
        run(&ffmpeg, &args)?;
        // A rename can't cross disks, and the temp folder is often on a different one.
        std::fs::copy(&tmp, out).map_err(|e| e.to_string())?;
        on_progress(1.0);
        Ok(true)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: VideoSettings = VideoSettings { width: 1280, height: 720, fps: 30 };

    fn picture(reversed: bool) -> SegmentPicture {
        SegmentPicture { source: SourceId(1), src_start: 10.0, src_end: 11.0, stretch: 2.0, reversed }
    }

    #[test]
    fn a_clip_segment_seeks_to_its_source_range_for_exact_frames() {
        let segment = VideoSegment { frames: 60, picture: Some(picture(false)) };
        let args = segment_args(&segment, Some(Path::new("a b.mp4")), &S, Path::new("out.mp4")).join(" ");
        assert!(args.contains("-ss 10.000000 -t 1.000000 -i a b.mp4"), "{args}");
        assert!(args.contains("-frames:v 60"), "{args}");
        assert!(args.contains("setpts=(PTS-STARTPTS)*2.000000"), "{args}");
        assert!(!args.contains("reverse"), "{args}");
    }

    #[test]
    fn a_reversed_clip_reverses_its_picture() {
        let segment = VideoSegment { frames: 30, picture: Some(picture(true)) };
        let args = segment_args(&segment, Some(Path::new("a.mp4")), &S, Path::new("out.mp4")).join(" ");
        assert!(args.contains("reverse,setpts"), "{args}");
    }

    #[test]
    fn a_gap_is_black_at_the_output_size() {
        let segment = VideoSegment { frames: 12, picture: None };
        let args = segment_args(&segment, None, &S, Path::new("out.mp4")).join(" ");
        assert!(args.contains("color=c=black:s=1280x720:r=30"), "{args}");
        assert!(args.contains("-frames:v 12"), "{args}");
    }
}
