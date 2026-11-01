//! The picture side of a project. Clips already say which part of a source plays when, so the
//! arrangement is an edit list: a sentence mix cuts its video by itself.

use crate::{Clip, Project, Seconds, SourceId};
use std::path::PathBuf;
use std::sync::OnceLock;

/// How long the last picture holds across a gap, so the short gaps between words don't flash black.
pub const HOLD_SECONDS: Seconds = 0.5;

/// Finds ffmpeg, which handles everything to do with video. An app opened from the Finder doesn't
/// get the shell's PATH, so the usual install locations are checked as well.
pub fn ffmpeg_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(find_ffmpeg).clone()
}

/// Looks again, for when someone may have just installed it.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MOUTHPIECE_FFMPEG") {
        return Some(PathBuf::from(path));
    }
    let from_env = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from);
    let binary = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    from_env.into_iter().chain(usual).map(|dir| dir.join(binary)).find(|p| p.is_file())
}

impl Project {
    pub fn has_video(&self) -> bool {
        self.sources.iter().any(|s| s.has_video)
    }

    /// The clip whose picture shows at a time: the topmost audible video clip playing then.
    pub fn picture_clip(&self, t: Seconds) -> Option<&Clip> {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        self.tracks
            .iter()
            .filter(|track| !track.mute && (!any_solo || track.solo))
            .find_map(|track| {
                track.clips.iter().rev().find(|c| {
                    !c.muted && c.start <= t && t < c.end() && self.clip_heard_at(track.id, c, t) && self.source(c.source).is_some_and(|s| s.has_video)
                })
            })
    }

    /// Like `picture_clip`, but keeps showing the last clip across a short gap.
    pub fn held_picture_clip(&self, t: Seconds) -> Option<&Clip> {
        self.picture_clip(t).or_else(|| {
            let any_solo = self.tracks.iter().any(|t| t.solo);
            self.tracks
                .iter()
                .filter(|track| !track.mute && (!any_solo || track.solo))
                .flat_map(|track| track.clips.iter())
                .filter(|c| !c.muted && c.end() <= t && t - c.end() < HOLD_SECONDS && self.source(c.source).is_some_and(|s| s.has_video))
                .max_by(|a, b| a.end().total_cmp(&b.end()))
        })
    }

    /// Cuts `start..end` into runs of frames that show the same thing. Lengths are whole frames, so
    /// joined segments can't drift against the audio however many cuts there are.
    pub fn video_edit_list(&self, start: Seconds, end: Seconds, fps: u32) -> Vec<VideoSegment> {
        let fps_f = fps as f64;
        let first = (start * fps_f).round() as u64;
        let last = (end * fps_f).round() as u64;
        let mut segments: Vec<VideoSegment> = Vec::new();
        let mut current: Option<crate::ClipId> = None;
        let mut run_start = first;
        for frame in first..last {
            let t = (frame as f64 + 0.5) / fps_f;
            let shown = self.held_picture_clip(t).map(|c| c.id);
            if shown != current || frame == first {
                if frame > first {
                    segments.push(self.make_segment(current, run_start, frame, fps_f));
                }
                current = shown;
                run_start = frame;
            }
        }
        if last > first {
            segments.push(self.make_segment(current, run_start, last, fps_f));
        }
        segments
    }

    fn make_segment(&self, clip: Option<crate::ClipId>, from: u64, to: u64, fps: f64) -> VideoSegment {
        let picture = clip.and_then(|id| self.clip(id)).map(|c| {
            // A held run goes past the clip's end, so the times stop at the clip. ffmpeg holds the last frame.
            let t0 = (from as f64 / fps).max(c.start);
            let t1 = (to as f64 / fps).min(c.end()).max(t0);
            let (a, b) = (c.source_time_at(t0), c.source_time_at(t1));
            SegmentPicture { source: c.source, src_start: a.min(b), src_end: a.max(b), stretch: c.stretch, reversed: c.reversed }
        });
        VideoSegment { frames: to - from, picture }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoSegment {
    pub frames: u64,
    /// `None` is black.
    pub picture: Option<SegmentPicture>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentPicture {
    pub source: SourceId,
    pub src_start: Seconds,
    pub src_end: Seconds,
    pub stretch: f64,
    pub reversed: bool,
}

#[cfg(test)]
mod tests {
    use crate::{Project, SourceId, TrackId, TrackKind};

    fn project() -> (Project, TrackId, SourceId) {
        let mut p = Project::default();
        let t = p.add_track("Sentence", TrackKind::Vocal);
        let s = p.add_source("v.mp4".into(), "v".into(), 60.0, TrackKind::Vocal);
        p.sources[0].has_video = true;
        (p, t, s)
    }

    #[test]
    fn the_edit_list_follows_the_clips() {
        let (mut p, t, s) = project();
        p.add_clip(t, s, 0.0, 10.0, 11.0).unwrap();
        p.add_clip(t, s, 1.0, 30.0, 32.0).unwrap();
        let edl = p.video_edit_list(0.0, 3.0, 30);
        assert_eq!(edl.iter().map(|e| e.frames).collect::<Vec<_>>(), [30, 60]);
        assert_eq!(edl[1].picture.as_ref().unwrap().src_start, 30.0);
        assert_eq!(edl[1].picture.as_ref().unwrap().src_end, 32.0);
    }

    #[test]
    fn frames_always_add_up_to_the_full_length() {
        let (mut p, t, s) = project();
        for i in 0..40 {
            let at = i as f64 * 0.137;
            p.add_clip(t, s, at, at, at + 0.09).unwrap();
        }
        let edl = p.video_edit_list(0.0, 10.0, 30);
        assert_eq!(edl.iter().map(|e| e.frames).sum::<u64>(), 300);
    }

    #[test]
    fn short_gaps_hold_and_long_gaps_go_black() {
        let (mut p, t, s) = project();
        p.add_clip(t, s, 0.0, 0.0, 1.0).unwrap();
        p.add_clip(t, s, 1.2, 5.0, 6.0).unwrap();
        p.add_clip(t, s, 4.0, 8.0, 9.0).unwrap();
        let edl = p.video_edit_list(0.0, 5.0, 10);
        let lengths: Vec<(u64, bool)> = edl.iter().map(|e| (e.frames, e.picture.is_some())).collect();
        // 1.2 s of first clip and hold, then 1.5 s including a 0.5 s hold, 1.3 s black, 1 s last clip.
        assert_eq!(lengths, [(12, true), (15, true), (13, false), (10, true)]);
    }

    #[test]
    fn muted_tracks_and_audio_sources_show_no_picture() {
        let (mut p, t, s) = project();
        p.add_clip(t, s, 0.0, 0.0, 1.0).unwrap();
        p.track_mut(t).unwrap().mute = true;
        assert!(p.picture_clip(0.5).is_none());
        p.track_mut(t).unwrap().mute = false;
        p.sources[0].has_video = false;
        assert!(p.picture_clip(0.5).is_none());
    }
}
