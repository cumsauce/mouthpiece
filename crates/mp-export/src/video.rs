//! Renders the picture. Each stretch of the edit list is decoded by ffmpeg at the output size, every
//! frame goes through the GPU effects, and the results go to one ffmpeg encoder along with the mix.

use mp_analysis::peaks::Peaks;
use mp_model::video::{ffmpeg_path, SegmentPicture, VideoSegment};
use mp_model::{Project, SourceId};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

/// What's needed to work out effects that follow a track's level.
pub struct LevelSource {
    pub peaks: HashMap<SourceId, Arc<Peaks>>,
    pub sample_rate: f64,
}

/// The ffmpeg arguments that decode one segment to raw frames on stdout.
pub fn decode_args(p: &SegmentPicture, path: &Path, s: &VideoSettings, frames: u64) -> Vec<String> {
    let (w, h, fps) = (s.width, s.height, s.fps);
    let reverse = if p.reversed { "reverse," } else { "" };
    // tpad holds the last frame when a segment runs on past its clip, across a short gap.
    let filter = format!(
        "{reverse}setpts=(PTS-STARTPTS)*{:.6},fps={fps},scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,tpad=stop_mode=clone:stop_duration=2",
        p.stretch
    );
    // A clip shorter than a frame still needs a frame of input to show.
    let length = (p.src_end - p.src_start).max(1.0 / fps as f64);
    let mut args: Vec<String> = ["-v", "error", "-ss"].iter().map(|a| a.to_string()).collect();
    args.extend([format!("{:.6}", p.src_start), "-t".into(), format!("{length:.6}"), "-i".into(), path.display().to_string()]);
    args.extend(["-an".into(), "-vf".into(), filter, "-frames:v".into(), frames.to_string()]);
    args.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "-"].iter().map(|a| a.to_string()));
    args
}

/// The ffmpeg arguments that encode raw frames from stdin, with `audio` as the sound.
pub fn encode_args(s: &VideoSettings, audio: &Path, out: &Path) -> Vec<String> {
    let mut args: Vec<String> = ["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"].iter().map(|a| a.to_string()).collect();
    args.extend([format!("{}x{}", s.width, s.height), "-r".into(), s.fps.to_string(), "-i".into(), "-".into(), "-i".into(), audio.display().to_string()]);
    args.extend(["-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "256k", "-shortest"].iter().map(|a| a.to_string()));
    args.push(out.display().to_string());
    args
}

/// Renders `start..end` of the project, with `audio` as its sound, and writes `out` only if it all
/// works. `on_progress` receives 0..=1, and returning false cancels. Returns Ok(false) if cancelled.
pub fn export_video(
    project: &Project,
    range: (f64, f64),
    levels: &LevelSource,
    audio: &Path,
    settings: &VideoSettings,
    out: &Path,
    mut on_progress: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let ffmpeg = ffmpeg_path().ok_or("ffmpeg isn't installed")?;
    let segments: Vec<VideoSegment> = project.video_edit_list(range.0, range.1, settings.fps);
    let sources: HashMap<SourceId, PathBuf> = project.sources.iter().map(|s| (s.id, s.picture_path.clone().unwrap_or_else(|| s.path.clone()))).collect();
    let total: u64 = segments.iter().map(|s| s.frames).sum();

    let mut renderer = if project.has_picture_effects() {
        Some(mp_vfx::Renderer::headless(settings.width, settings.height).ok_or("no usable GPU for the picture effects")?)
    } else {
        None
    };
    let peaks = |source: SourceId| levels.peaks.get(&source).map(Arc::as_ref);
    let level = |track, t| mp_vfx::track_level(project, track, t, &peaks, levels.sample_rate);

    let tmp = out.with_extension("rendering.mp4");
    let mut encoder = Command::new(&ffmpeg)
        .args(encode_args(settings, audio, &tmp))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not run ffmpeg: {e}"))?;
    let mut sink = encoder.stdin.take().ok_or("could not talk to ffmpeg")?;

    let frame_bytes = (settings.width * settings.height * 4) as usize;
    let aspect = settings.width as f32 / settings.height as f32;
    let first_frame = (range.0 * settings.fps as f64).round() as u64;
    let mut done = 0u64;
    let mut cancelled = false;
    let mut buffer = vec![0u8; frame_bytes];
    let mut failure = None;

    'segments: for segment in &segments {
        let mut decoder = match segment.picture.as_ref().and_then(|p| sources.get(&p.source).map(|path| (p, path))) {
            Some((picture, path)) => Some(
                Command::new(&ffmpeg)
                    .args(decode_args(picture, path, settings, segment.frames))
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("could not run ffmpeg: {e}"))?,
            ),
            None => None,
        };
        let mut pipe = decoder.as_mut().and_then(|d| d.stdout.take());
        // Black until a decoder gives a frame, and if one comes up short, the last frame holds.
        buffer.fill(0);
        for pixel in buffer.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        for _ in 0..segment.frames {
            if let Some(stream) = &mut pipe {
                if stream.read_exact(&mut buffer).is_err() {
                    pipe = None;
                }
            }
            let t = (first_frame + done) as f64 / settings.fps as f64;
            let written = match &mut renderer {
                Some(gpu) => {
                    let passes = mp_vfx::resolve_frame(project, t, &level);
                    if passes.is_empty() {
                        sink.write_all(&buffer)
                    } else {
                        gpu.upload(&buffer, settings.width, settings.height);
                        gpu.render(&passes, t as f32, aspect);
                        sink.write_all(&gpu.read_output())
                    }
                }
                None => sink.write_all(&buffer),
            };
            if let Err(e) = written {
                failure = Some(format!("ffmpeg stopped while encoding: {e}"));
                break 'segments;
            }
            done += 1;
            if done % 8 == 0 && !on_progress(done as f32 / total.max(1) as f32) {
                cancelled = true;
                break 'segments;
            }
        }
        drop(pipe);
        if let Some(mut d) = decoder {
            let _ = d.kill();
            let _ = d.wait();
        }
    }
    drop(sink);
    let status = encoder.wait().map_err(|e| e.to_string())?;
    if cancelled || failure.is_some() || !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return match failure {
            Some(e) => Err(e),
            None if cancelled => Ok(false),
            None => Err("ffmpeg couldn't encode the video".to_string()),
        };
    }
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    on_progress(1.0);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: VideoSettings = VideoSettings { width: 1280, height: 720, fps: 30 };

    fn picture(reversed: bool) -> SegmentPicture {
        SegmentPicture { source: SourceId(1), src_start: 10.0, src_end: 11.0, stretch: 2.0, reversed }
    }

    #[test]
    fn decoding_seeks_to_the_source_range_for_exact_frames() {
        let args = decode_args(&picture(false), Path::new("a b.mp4"), &S, 60).join(" ");
        assert!(args.contains("-ss 10.000000 -t 1.000000 -i a b.mp4"), "{args}");
        assert!(args.contains("-frames:v 60"), "{args}");
        assert!(args.contains("setpts=(PTS-STARTPTS)*2.000000"), "{args}");
        assert!(args.contains("-pix_fmt rgba"), "{args}");
        assert!(!args.contains("reverse"), "{args}");
    }

    #[test]
    fn a_reversed_clip_reverses_its_picture() {
        let args = decode_args(&picture(true), Path::new("a.mp4"), &S, 30).join(" ");
        assert!(args.contains("reverse,setpts"), "{args}");
    }

    #[test]
    fn encoding_takes_frames_from_stdin_and_the_mix_as_sound() {
        let args = encode_args(&S, Path::new("mix.wav"), Path::new("out.mp4")).join(" ");
        assert!(args.contains("-s 1280x720 -r 30 -i - -i mix.wav"), "{args}");
        assert!(args.ends_with("out.mp4"), "{args}");
    }
}
