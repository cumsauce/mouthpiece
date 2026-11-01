//! Pure edit operations. All are non-destructive: they only change clip metadata.

use crate::*;

/// Fades applied to clips cut from a longer recording so the cut points don't click.
pub const SPLICE_FADE: Seconds = 0.004;

impl Project {
    /// Adds a track in its usual place: vocals on top, then drums, bass and everything else.
    /// Each kind has its own colour, so a song's stems always look the same.
    pub fn add_track(&mut self, name: impl Into<String>, kind: TrackKind) -> TrackId {
        let id = TrackId(self.alloc_id());
        let same_kind = self.tracks.iter().filter(|t| t.kind == kind).count() as u8;
        let color = match kind {
            TrackKind::Vocal => [0, 5][same_kind as usize % 2],
            TrackKind::Drums => [1, 6][same_kind as usize % 2],
            TrackKind::Bass => 2,
            TrackKind::Music => [3, 4, 7][same_kind as usize % 3],
        };
        let position = self.tracks.iter().rposition(|t| t.kind.order() <= kind.order()).map_or(0, |i| i + 1);
        self.tracks.insert(position, Track {
            id,
            name: name.into(),
            kind,
            color,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            height: 84.0,
            clips: Vec::new(),
            devices: Vec::new(),
        });
        id
    }

    pub fn remove_track(&mut self, id: TrackId) {
        self.tracks.retain(|t| t.id != id);
    }

    pub fn add_source(&mut self, path: PathBuf, name: String, duration: Seconds, kind: TrackKind) -> SourceId {
        let id = SourceId(self.alloc_id());
        self.sources.push(Source { id, path, name, duration, bpm: None, kind });
        id
    }

    /// Adds a clip that plays `src_in..src_out` of a source and returns its id.
    pub fn add_clip(
        &mut self,
        track: TrackId,
        source: SourceId,
        start: Seconds,
        src_in: Seconds,
        src_out: Seconds,
    ) -> Option<ClipId> {
        let id = ClipId(self.alloc_id());
        let clip = Clip::new(id, source, start.max(0.0), src_in, src_out);
        self.track_mut(track)?.clips.push(clip);
        Some(id)
    }

    /// Adds a clip cut from a longer recording, with anti-click fades and a label.
    pub fn add_slice(
        &mut self,
        track: TrackId,
        source: SourceId,
        start: Seconds,
        src_in: Seconds,
        src_out: Seconds,
        label: Option<String>,
    ) -> Option<ClipId> {
        let id = self.add_clip(track, source, start, src_in, src_out)?;
        let clip = self.clip_mut(id)?;
        clip.fade_in = SPLICE_FADE;
        clip.fade_out = SPLICE_FADE;
        clip.label = label;
        Some(id)
    }

    pub fn remove_clip(&mut self, id: ClipId) -> Option<Clip> {
        for t in &mut self.tracks {
            if let Some(i) = t.clips.iter().position(|c| c.id == id) {
                return Some(t.clips.remove(i));
            }
        }
        None
    }

    /// Splits a clip at arrangement time `at`. The left part keeps the original id; returns the
    /// new right part's id, or `None` if `at` isn't inside the clip.
    pub fn split_clip(&mut self, id: ClipId, at: Seconds) -> Option<ClipId> {
        let clip = self.clip(id)?;
        if at <= clip.start + MIN_CLIP_LEN || at >= clip.end() - MIN_CLIP_LEN {
            return None;
        }
        let new_id = ClipId(self.alloc_id());
        let track = self.track_of_clip(id)?;
        let clip = self.clip_mut(id)?;
        let cut = clip.source_time_at(at);
        let mut right = clip.clone();
        right.id = new_id;
        right.start = at;
        if clip.reversed {
            // Time runs backwards through the source: the left part plays the upper range.
            clip.src_in = cut;
            right.src_out = cut;
        } else {
            clip.src_out = cut;
            right.src_in = cut;
        }
        clip.fade_out = 0.0;
        right.fade_in = 0.0;
        right.fade_out = right.fade_out.min(right.length());
        clip.fade_in = clip.fade_in.min(clip.length());
        self.track_mut(track)?.clips.push(right);
        Some(new_id)
    }

    /// Moves a clip's start edge to `to`, keeping the audio under the rest of the clip in place.
    pub fn trim_start(&mut self, id: ClipId, to: Seconds) {
        let Some(clip) = self.clip_mut(id) else { return };
        let to = to.clamp(
            clip.start - clip.available_before() * clip.stretch,
            clip.end() - MIN_CLIP_LEN,
        ).max(0.0);
        let delta = (to - clip.start) / clip.stretch;
        if clip.reversed {
            clip.src_out -= delta;
        } else {
            clip.src_in += delta;
        }
        clip.start = to;
        clip.clamp_fades();
    }

    /// Moves a clip's end edge to `to`. `source_len` limits how far it can be extended.
    pub fn trim_end(&mut self, id: ClipId, to: Seconds) {
        let Some(source_len) = self
            .clip(id)
            .and_then(|c| self.source(c.source))
            .map(|s| s.duration)
        else {
            return;
        };
        let Some(clip) = self.clip_mut(id) else { return };
        let available = if clip.reversed { clip.src_in } else { source_len - clip.src_out };
        let to = to.clamp(clip.start + MIN_CLIP_LEN, clip.end() + available * clip.stretch);
        let delta = (to - clip.end()) / clip.stretch;
        if clip.reversed {
            clip.src_in -= delta;
        } else {
            clip.src_out += delta;
        }
        clip.clamp_fades();
    }

    /// Changes the stretch ratio, anchored at the clip start.
    pub fn set_stretch(&mut self, id: ClipId, stretch: f64) {
        if let Some(clip) = self.clip_mut(id) {
            clip.stretch = stretch.clamp(0.1, 16.0);
            clip.clamp_fades();
        }
    }

    pub fn duplicate_clip(&mut self, id: ClipId, start: Seconds) -> Option<ClipId> {
        let track = self.track_of_clip(id)?;
        self.duplicate_clip_to(id, track, start)
    }

    pub fn duplicate_clip_to(&mut self, id: ClipId, track: TrackId, start: Seconds) -> Option<ClipId> {
        let new_id = ClipId(self.alloc_id());
        let mut clip = self.clip(id)?.clone();
        clip.id = new_id;
        clip.start = start.max(0.0);
        self.track_mut(track)?.clips.push(clip);
        Some(new_id)
    }

    pub fn move_clip_to_track(&mut self, id: ClipId, track: TrackId) {
        if self.track_of_clip(id) == Some(track) || self.track(track).is_none() {
            return;
        }
        if let Some(clip) = self.remove_clip(id) {
            if let Some(t) = self.track_mut(track) {
                t.clips.push(clip);
            }
        }
    }

    /// Cuts a clip at each `(source_start, source_end, label)` range, producing one clip per range
    /// in the original positions. Ranges are in source time. Returns the new clips in time order.
    pub fn slice_clip_to_ranges(&mut self, id: ClipId, ranges: &[(Seconds, Seconds, String)]) -> Vec<ClipId> {
        let Some(clip) = self.clip(id).cloned() else { return Vec::new() };
        let Some(track) = self.track_of_clip(id) else { return Vec::new() };
        let mut out = Vec::new();
        for (a, b, label) in ranges {
            let a = a.max(clip.src_in);
            let b = b.min(clip.src_out);
            if b - a < MIN_CLIP_LEN {
                continue;
            }
            let new_id = ClipId(self.alloc_id());
            let mut part = clip.clone();
            part.id = new_id;
            part.src_in = a;
            part.src_out = b;
            part.start = if clip.reversed { clip.time_of_source(b) } else { clip.time_of_source(a) };
            part.fade_in = SPLICE_FADE;
            part.fade_out = SPLICE_FADE;
            part.label = Some(label.clone());
            if let Some(t) = self.track_mut(track) {
                t.clips.push(part);
            }
            out.push(new_id);
        }
        if !out.is_empty() {
            self.remove_clip(id);
            out.sort_by(|a, b| {
                let (a, b) = (self.clip(*a).map_or(0.0, |c| c.start), self.clip(*b).map_or(0.0, |c| c.start));
                a.total_cmp(&b)
            });
        }
        out
    }

    /// Nearest grid line to `t`, where the grid is `division` beats wide.
    pub fn snap_to_grid(&self, t: Seconds, division: f64) -> Seconds {
        let step = self.beat_seconds() * division;
        ((t - self.grid_offset) / step).round() * step + self.grid_offset
    }

    /// Snaps clip starts to the grid.
    pub fn quantize(&mut self, ids: &[ClipId], division: f64) {
        for id in ids {
            if let Some(start) = self.clip(*id).map(|c| c.start) {
                let snapped = self.snap_to_grid(start, division).max(0.0);
                if let Some(c) = self.clip_mut(*id) {
                    c.start = snapped;
                }
            }
        }
    }

    /// Butts clips up against each other in time order, keeping the first in place.
    pub fn close_gaps(&mut self, ids: &[ClipId], gap: Seconds) {
        let mut order: Vec<ClipId> = ids.to_vec();
        order.sort_by(|a, b| {
            let (a, b) = (self.clip(*a).map_or(0.0, |c| c.start), self.clip(*b).map_or(0.0, |c| c.start));
            a.total_cmp(&b)
        });
        let mut cursor = None;
        for id in order {
            if let Some(c) = self.clip_mut(id) {
                if let Some(t) = cursor {
                    c.start = t;
                }
                cursor = Some(c.end() + gap);
            }
        }
    }

    /// Sets a clip's stretch so that it follows the project tempo.
    pub fn fit_clip_to_tempo(&mut self, id: ClipId) {
        let bpm = self.bpm;
        let Some(source_bpm) = self.clip(id).and_then(|c| self.source(c.source)).and_then(|s| s.bpm) else {
            return;
        };
        self.set_stretch(id, source_bpm / bpm);
    }

    pub fn add_device(&mut self, track: Option<TrackId>, kind: DeviceKind) -> DeviceId {
        let id = DeviceId(self.alloc_id());
        let device = DeviceInstance { id, kind, enabled: true, params: BTreeMap::new() };
        match track.and_then(|t| self.track_mut(t)) {
            Some(t) => t.devices.push(device),
            None => self.master.devices.push(device),
        }
        id
    }

    pub fn device_chain_mut(&mut self, track: Option<TrackId>) -> Option<&mut Vec<DeviceInstance>> {
        match track {
            Some(t) => self.track_mut(t).map(|t| &mut t.devices),
            None => Some(&mut self.master.devices),
        }
    }

    pub fn device_chain(&self, track: Option<TrackId>) -> Option<&Vec<DeviceInstance>> {
        match track {
            Some(t) => self.track(t).map(|t| &t.devices),
            None => Some(&self.master.devices),
        }
    }
}

impl Clip {
    fn available_before(&self) -> Seconds {
        // Reversed clips extend into later source; the caller clamps against the source length
        // for `trim_end`, and a start trim on a reversed clip can only reach what remains above.
        if self.reversed { 0.0 } else { self.src_in }
    }

    fn clamp_fades(&mut self) {
        let len = self.length();
        self.fade_in = self.fade_in.min(len);
        self.fade_out = self.fade_out.min(len - self.fade_in);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, TrackId, SourceId, ClipId) {
        let mut p = Project::default();
        let t = p.add_track("Vox", TrackKind::Vocal);
        let s = p.add_source("v.wav".into(), "v".into(), 20.0, TrackKind::Vocal);
        let c = p.add_clip(t, s, 10.0, 2.0, 12.0).unwrap();
        (p, t, s, c)
    }

    #[test]
    fn split_preserves_audio_mapping() {
        let (mut p, _, _, c) = project();
        let r = p.split_clip(c, 13.0).unwrap();
        let (left, right) = (p.clip(c).unwrap(), p.clip(r).unwrap());
        assert_eq!((left.src_in, left.src_out, left.start), (2.0, 5.0, 10.0));
        assert_eq!((right.src_in, right.src_out, right.start), (5.0, 12.0, 13.0));
    }

    #[test]
    fn split_reversed_clip_keeps_each_moment_of_audio_in_place() {
        let (mut p, _, _, c) = project();
        p.clip_mut(c).unwrap().reversed = true;
        let heard_before = p.clip(c).unwrap().source_time_at(11.0);
        let heard_after = p.clip(c).unwrap().source_time_at(15.0);
        let r = p.split_clip(c, 13.0).unwrap();
        let (left, right) = (p.clip(c).unwrap(), p.clip(r).unwrap());
        assert!((left.source_time_at(11.0) - heard_before).abs() < 1e-9);
        assert!((right.source_time_at(15.0) - heard_after).abs() < 1e-9);
    }

    #[test]
    fn split_stretched_clip_uses_source_time() {
        let (mut p, _, _, c) = project();
        p.set_stretch(c, 2.0);
        let r = p.split_clip(c, 14.0).unwrap();
        assert_eq!(p.clip(c).unwrap().src_out, 4.0);
        assert_eq!(p.clip(r).unwrap().src_in, 4.0);
        assert_eq!(p.clip(r).unwrap().start, 14.0);
    }

    #[test]
    fn split_outside_clip_does_nothing() {
        let (mut p, t, _, c) = project();
        assert!(p.split_clip(c, 5.0).is_none());
        assert!(p.split_clip(c, 25.0).is_none());
        assert_eq!(p.track(t).unwrap().clips.len(), 1);
    }

    #[test]
    fn trim_keeps_audio_under_the_clip_in_place() {
        let (mut p, _, _, c) = project();
        let at_12 = p.clip(c).unwrap().source_time_at(12.0);
        p.trim_start(c, 11.0);
        p.trim_end(c, 15.0);
        let clip = p.clip(c).unwrap();
        assert_eq!((clip.start, clip.end()), (11.0, 15.0));
        assert!((clip.source_time_at(12.0) - at_12).abs() < 1e-9);
    }

    #[test]
    fn trim_cannot_extend_past_source() {
        let (mut p, _, _, c) = project();
        p.trim_start(c, 0.0);
        p.trim_end(c, 100.0);
        let clip = p.clip(c).unwrap();
        assert_eq!(clip.src_in, 0.0);
        assert_eq!(clip.src_out, 20.0);
    }

    #[test]
    fn slice_to_words_replaces_clip_and_keeps_positions() {
        let (mut p, t, _, c) = project();
        let ranges = vec![(3.0, 4.0, "hello".to_string()), (5.0, 6.0, "world".to_string()), (30.0, 31.0, "gone".to_string())];
        let parts = p.slice_clip_to_ranges(c, &ranges);
        assert_eq!(parts.len(), 2);
        assert_eq!(p.track(t).unwrap().clips.len(), 2);
        let first = p.clip(parts[0]).unwrap();
        assert_eq!((first.start, first.label.as_deref()), (11.0, Some("hello")));
        assert_eq!(p.clip(parts[1]).unwrap().start, 13.0);
    }

    #[test]
    fn quantize_and_close_gaps() {
        let (mut p, t, s, c) = project();
        p.bpm = 120.0;
        let c2 = p.add_clip(t, s, 30.26, 0.0, 1.0).unwrap();
        p.quantize(&[c2], 1.0);
        assert_eq!(p.clip(c2).unwrap().start, 30.5);
        p.close_gaps(&[c, c2], 0.0);
        assert_eq!(p.clip(c2).unwrap().start, p.clip(c).unwrap().end());
    }

    #[test]
    fn undo_redo_roundtrip() {
        let (mut p, _, _, c) = project();
        let mut h = History::default();
        let before = p.clone();
        h.push("Split", p.clone());
        p.split_clip(c, 13.0);
        let after = p.clone();
        assert_eq!(h.undo(&mut p).as_deref(), Some("Split"));
        assert_eq!(p, before);
        assert_eq!(h.redo(&mut p).as_deref(), Some("Split"));
        assert_eq!(p, after);
    }

    #[test]
    fn project_roundtrips_through_json() {
        let (mut p, t, _, _) = project();
        p.add_device(Some(t), DeviceKind::Clap { path: "x.clap".into(), plugin_id: "a.b".into(), state: vec![1, 2] });
        let back = Project::from_json(&p.to_json().unwrap()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn key_snaps_to_nearest_scale_note() {
        let c_major = MusicalKey { root: 0, minor: false };
        assert_eq!(c_major.snap_semitones(61.0).abs(), 1.0); // C# is not in C major
        assert!((c_major.snap_semitones(60.2) + 0.2).abs() < 1e-4);
    }

    #[test]
    fn tracks_are_ordered_by_kind_whatever_order_they_arrive_in() {
        let mut p = Project::default();
        p.add_track("Pad", TrackKind::Music);
        p.add_track("Bass", TrackKind::Bass);
        p.add_track("Drums", TrackKind::Drums);
        p.add_track("Keys", TrackKind::Music);
        p.add_track("Vocals", TrackKind::Vocal);
        let names: Vec<&str> = p.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Vocals", "Drums", "Bass", "Pad", "Keys"]);
    }

    #[test]
    fn stem_kinds_from_filenames() {
        assert_eq!(TrackKind::from_filename("Song - Vocals.wav"), TrackKind::Vocal);
        assert_eq!(TrackKind::from_filename("drums.flac"), TrackKind::Drums);
        assert_eq!(TrackKind::from_filename("BASS.mp3"), TrackKind::Bass);
        assert_eq!(TrackKind::from_filename("other.wav"), TrackKind::Music);
    }
}
