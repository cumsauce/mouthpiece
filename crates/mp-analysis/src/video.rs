//! Reading video files with ffmpeg: their audio, and a small copy of the picture for previewing.

use mp_dsp::Stereo;
use mp_model::video::ffmpeg_path;
use std::path::Path;
use std::process::{Command, Stdio};

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "mkv", "webm", "avi"];

pub fn is_video_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Preview frames are kept small and in memory, so that scrubbing and cutting never wait on the disk.
pub const PROXY_WIDTH: usize = 320;
pub const PROXY_HEIGHT: usize = 180;
pub const PROXY_FPS: f64 = 10.0;

/// A low-resolution copy of a video, as tightly packed RGB frames.
pub struct VideoProxy {
    pub frames: Vec<u8>,
}

impl VideoProxy {
    const FRAME_BYTES: usize = PROXY_WIDTH * PROXY_HEIGHT * 3;

    pub fn frame_count(&self) -> usize {
        self.frames.len() / Self::FRAME_BYTES
    }

    /// The index of the frame at a time in the source, staying inside the video.
    pub fn frame_index(&self, seconds: f64) -> Option<usize> {
        let count = self.frame_count();
        (count > 0).then(|| ((seconds.max(0.0) * PROXY_FPS) as usize).min(count - 1))
    }

    pub fn frame(&self, index: usize) -> Option<&[u8]> {
        self.frames.get(index * Self::FRAME_BYTES..(index + 1) * Self::FRAME_BYTES)
    }
}

/// Decodes the small preview copy. Blocks, so call off the UI thread. Fails if there's no picture.
pub fn extract_proxy(path: &Path) -> Result<VideoProxy, String> {
    let ffmpeg = ffmpeg_path().ok_or("ffmpeg isn't installed")?;
    let filter = format!(
        "fps={PROXY_FPS},scale={PROXY_WIDTH}:{PROXY_HEIGHT}:force_original_aspect_ratio=decrease,pad={PROXY_WIDTH}:{PROXY_HEIGHT}:(ow-iw)/2:(oh-ih)/2"
    );
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-an", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run ffmpeg: {e}"))?;
    if output.stdout.len() < VideoProxy::FRAME_BYTES {
        return Err("this file has no picture".to_string());
    }
    Ok(VideoProxy { frames: output.stdout })
}

/// Decodes a video's sound with ffmpeg, which reads more formats than the built-in decoders.
pub fn extract_audio(path: &Path, sample_rate: u32) -> Result<Stereo, String> {
    let ffmpeg = ffmpeg_path().ok_or("ffmpeg isn't installed")?;
    let output = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "2", "-ar", &sample_rate.to_string(), "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run ffmpeg: {e}"))?;
    if output.stdout.is_empty() {
        return Err("this video has no sound to cut".to_string());
    }
    let mut out = Stereo::default();
    for frame in output.stdout.chunks_exact(8) {
        out.l.push(f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]));
        out.r.push(f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_lookup_stays_in_bounds() {
        let proxy = VideoProxy { frames: vec![0; VideoProxy::FRAME_BYTES * 20] };
        assert_eq!(proxy.frame_count(), 20);
        assert_eq!(proxy.frame_index(1.0), Some(10));
        assert_eq!(proxy.frame_index(500.0), Some(19));
        assert_eq!(proxy.frame_index(-1.0), Some(0));
        assert_eq!(proxy.frame(19).map(<[u8]>::len), Some(VideoProxy::FRAME_BYTES));
        assert!(proxy.frame(20).is_none());
    }

    #[test]
    fn recognises_video_files() {
        assert!(is_video_path(Path::new("a/b.MOV")));
        assert!(!is_video_path(Path::new("a/b.wav")));
    }
}
