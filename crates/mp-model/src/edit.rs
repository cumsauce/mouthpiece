//! Pure edit operations. All are non-destructive: they only change clip metadata.

use crate::*;
use std::collections::{HashMap, HashSet};

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
            height: 76.0,
            clips: Vec::new(),
            devices: Vec::new(),
            picture: Vec::new(),
        });
        id
    }

    /// Removes a track, along with any changes on it.
    pub fn remove_track(&mut self, id: TrackId) {
        self.tracks.retain(|t| t.id != id);
        self.patches.retain(|p| p.track != id);
    }

    pub fn add_source(&mut self, path: PathBuf, name: String, duration: Seconds, kind: TrackKind) -> SourceId {
        let id = SourceId(self.alloc_id());
        self.sources.push(Source { id, path, name, duration, bpm: None, kind, has_video: false, picture_path: None, relative_path: None, relative_picture: None });
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
        clip.origin = Some((src_in, src_out));
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
        let Some(source_len) = self.clip(id).and_then(|c| self.source(c.source)).map(|s| s.duration) else {
            return;
        };
        let Some(clip) = self.clip_mut(id) else { return };
        // A reversed clip's start plays the end of its range, so it extends into later source.
        let available = if clip.reversed { source_len - clip.src_out } else { clip.src_in };
        let earliest = clip.start - available.max(0.0) * clip.stretch;
        // A clip already under the minimum length can't be shortened, but mustn't invert this range.
        let latest = (clip.end() - MIN_CLIP_LEN).max(clip.start);
        let to = to.clamp(earliest, latest).max(0.0);
        let delta = (to - clip.start) / clip.stretch;
        if clip.reversed {
            clip.src_out -= delta;
        } else {
            clip.src_in += delta;
        }
        clip.start = to;
        clip.clamp_fades();
    }

    /// Moves a clip's end edge to `to`, up to the limit of the source.
    pub fn trim_end(&mut self, id: ClipId, to: Seconds) {
        let Some(source_len) = self.clip(id).and_then(|c| self.source(c.source)).map(|s| s.duration) else {
            return;
        };
        let Some(clip) = self.clip_mut(id) else { return };
        let available = if clip.reversed { clip.src_in } else { source_len - clip.src_out };
        let earliest = (clip.start + MIN_CLIP_LEN).min(clip.end());
        let to = to.clamp(earliest, clip.end() + available.max(0.0) * clip.stretch);
        let delta = (to - clip.end()) / clip.stretch;
        if clip.reversed {
            clip.src_in -= delta;
        } else {
            clip.src_out += delta;
        }
        clip.clamp_fades();
    }

    /// Gives a clip different audio, keeping its position and everything done to it.
    pub fn swap_clip_audio(&mut self, id: ClipId, source: SourceId, src_in: Seconds, src_out: Seconds) {
        if let Some(clip) = self.clip_mut(id) {
            clip.source = source;
            clip.src_in = src_in;
            clip.src_out = src_out;
            clip.origin = Some((src_in, src_out));
            clip.clamp_fades();
        }
    }

    /// Takes a clip back to how it was first placed, keeping its position. Nothing done to a clip ever
    /// touches its recording, so this is always possible.
    pub fn restore_original(&mut self, id: ClipId) {
        let Some(duration) = self.clip(id).and_then(|c| self.source(c.source)).map(|s| s.duration) else { return };
        let Some(clip) = self.clip_mut(id) else { return };
        let (a, b) = clip.origin.unwrap_or((0.0, duration));
        clip.src_in = a;
        clip.src_out = b;
        clip.stretch = 1.0;
        clip.pitch = 0.0;
        clip.gain_db = 0.0;
        clip.pan = 0.0;
        clip.reversed = false;
        clip.muted = false;
        clip.mode = ClipMode::Elastic;
        clip.fade_in = SPLICE_FADE.min(clip.length());
        clip.fade_out = SPLICE_FADE.min(clip.length() - clip.fade_in);
        clip.fade_in_auto = false;
        clip.fade_out_auto = false;
    }

    // ----- Groups -----

    /// Makes clips move and select as one.
    pub fn group_clips(&mut self, ids: &[ClipId], word_name: Option<String>) -> GroupId {
        let group = GroupId(self.alloc_id());
        for id in ids {
            if let Some(c) = self.clip_mut(*id) {
                c.group = Some(group);
                c.word_name = word_name.clone();
            }
        }
        group
    }

    pub fn ungroup_clips(&mut self, ids: &[ClipId]) {
        let members: Vec<ClipId> = ids.iter().flat_map(|id| self.group_members(*id)).collect();
        for id in members {
            if let Some(c) = self.clip_mut(id) {
                c.group = None;
                c.word_name = None;
                c.made_word = None;
            }
        }
    }

    /// Every clip in a clip's group, or just the clip if it isn't in one. In time order.
    pub fn group_members(&self, id: ClipId) -> Vec<ClipId> {
        let Some(clip) = self.clip(id) else { return Vec::new() };
        let Some(group) = clip.group else { return vec![id] };
        let mut members: Vec<&Clip> = self.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.group == Some(group)).collect();
        members.sort_by(|a, b| a.start.total_cmp(&b.start));
        members.iter().map(|c| c.id).collect()
    }

    /// Copies clips `offset` later. Copied groups stay together, as new groups of their own.
    pub fn duplicate_clips(&mut self, ids: &[ClipId], offset: Seconds) -> Vec<ClipId> {
        let mut new_groups: HashMap<GroupId, GroupId> = HashMap::new();
        let mut copies = Vec::new();
        for id in ids {
            let Some(mut clip) = self.clip(*id).cloned() else { continue };
            let Some(track) = self.track_of_clip(*id) else { continue };
            let new_id = ClipId(self.alloc_id());
            clip.id = new_id;
            clip.start = (clip.start + offset).max(0.0);
            if let Some(group) = clip.group {
                let copy_group = match new_groups.get(&group) {
                    Some(g) => *g,
                    None => {
                        let g = GroupId(self.alloc_id());
                        new_groups.insert(group, g);
                        g
                    }
                };
                clip.group = Some(copy_group);
            }
            if let Some(t) = self.track_mut(track) {
                t.clips.push(clip);
            }
            copies.push(new_id);
        }
        copies
    }

    // ----- Crossfades -----

    /// Overlaps clips in time order by `overlap`, and crossfades them. Clips after them stay put.
    pub fn crossfade_clips(&mut self, ids: &[ClipId], overlap: Seconds) {
        let mut order: Vec<ClipId> = ids.to_vec();
        order.sort_by(|a, b| {
            let (a, b) = (self.clip(*a).map_or(0.0, |c| c.start), self.clip(*b).map_or(0.0, |c| c.start));
            a.total_cmp(&b)
        });
        let mut previous_end = None;
        for id in order {
            let Some(c) = self.clip_mut(id) else { continue };
            if let Some(end) = previous_end {
                let end: Seconds = end;
                c.start = (end - overlap.min(c.length() * 0.5)).max(0.0);
            }
            previous_end = Some(c.end());
        }
        self.update_auto_crossfades();
    }

    /// Overlapping clips on a track fade across each other, for as long as they overlap. When they
    /// part, the automatic fades go back to the short anti-click ones.
    pub fn update_auto_crossfades(&mut self) {
        for track in &mut self.tracks {
            track.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let mut fades_in = vec![None; track.clips.len()];
            let mut fades_out = vec![None; track.clips.len()];
            for i in 0..track.clips.len() {
                for j in i + 1..track.clips.len() {
                    let (a, b) = (&track.clips[i], &track.clips[j]);
                    if b.start >= a.end() {
                        // Later clips can't overlap this one, as they're sorted by start.
                        break;
                    }
                    // One clip inside another, such as a word over a whole stem, isn't a crossfade.
                    // Neither is a change over the recording it replaces.
                    if a.patch == b.patch && b.end() > a.end() && b.start > a.start {
                        let overlap = a.end() - b.start;
                        fades_out[i] = Some(overlap);
                        fades_in[j] = Some(overlap);
                    }
                }
            }
            for (i, clip) in track.clips.iter_mut().enumerate() {
                match fades_in[i] {
                    Some(f) => {
                        clip.fade_in = f;
                        clip.fade_in_auto = true;
                    }
                    None if clip.fade_in_auto => {
                        clip.fade_in = SPLICE_FADE.min(clip.length());
                        clip.fade_in_auto = false;
                    }
                    None => {}
                }
                match fades_out[i] {
                    Some(f) => {
                        clip.fade_out = f;
                        clip.fade_out_auto = true;
                    }
                    None if clip.fade_out_auto => {
                        clip.fade_out = SPLICE_FADE.min(clip.length());
                        clip.fade_out_auto = false;
                    }
                    None => {}
                }
            }
        }
    }

    // ----- Made words -----

    pub fn word(&self, id: WordId) -> Option<&MadeWord> {
        self.words.iter().find(|w| w.id == id)
    }

    pub fn word_named(&self, name: &str) -> Option<&MadeWord> {
        self.words.iter().find(|w| w.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Saves clips as a word that can be found and used again. Using an existing name updates it.
    /// The clips become a group that's linked to the word.
    pub fn save_word(&mut self, name: &str, ids: &[ClipId]) -> Option<WordId> {
        let mut parts: Vec<Clip> = ids.iter().filter_map(|id| self.clip(*id).cloned()).collect();
        let earliest = parts.iter().map(|c| c.start).reduce(f64::min)?;
        parts.sort_by(|a, b| a.start.total_cmp(&b.start));
        for part in &mut parts {
            part.start -= earliest;
            part.group = None;
            part.made_word = None;
            part.word_name = None;
        }
        let name = name.trim().to_string();
        let id = match self.word_named(&name).map(|w| w.id) {
            Some(existing) => existing,
            None => {
                let id = WordId(self.alloc_id());
                self.words.push(MadeWord { id, name: name.clone(), parts: Vec::new() });
                id
            }
        };
        if let Some(word) = self.words.iter_mut().find(|w| w.id == id) {
            word.parts = parts;
        }
        self.group_clips(ids, Some(name));
        for clip in ids {
            if let Some(c) = self.clip_mut(*clip) {
                c.made_word = Some(id);
            }
        }
        Some(id)
    }

    /// Puts copies of a saved word's clips on a track, as a group.
    pub fn place_word(&mut self, id: WordId, track: TrackId, at: Seconds) -> Vec<ClipId> {
        let Some(word) = self.word(id).cloned() else { return Vec::new() };
        let group = GroupId(self.alloc_id());
        let mut placed = Vec::new();
        for mut part in word.parts {
            let new_id = ClipId(self.alloc_id());
            part.id = new_id;
            part.start += at;
            part.group = Some(group);
            part.made_word = Some(id);
            part.word_name = Some(word.name.clone());
            if let Some(t) = self.track_mut(track) {
                t.clips.push(part);
                placed.push(new_id);
            }
        }
        placed
    }

    /// Replaces a placed copy of a saved word with a fresh one, in the same place.
    pub fn revert_to_saved(&mut self, clip: ClipId) -> Vec<ClipId> {
        let Some(word) = self.clip(clip).and_then(|c| c.made_word) else { return Vec::new() };
        let Some(track) = self.track_of_clip(clip) else { return Vec::new() };
        let members = self.group_members(clip);
        let start = members.iter().filter_map(|m| self.clip(*m)).map(|c| c.start).fold(f64::MAX, f64::min);
        for member in members {
            self.remove_clip(member);
        }
        self.place_word(word, track, start)
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
            part.origin = Some((a, b));
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

    /// Snaps clip starts to the grid. A group moves as one, by its first clip, so a made word stays intact.
    pub fn quantize(&mut self, ids: &[ClipId], division: f64) {
        let mut done: HashSet<ClipId> = HashSet::new();
        for id in ids {
            if done.contains(id) {
                continue;
            }
            let members = self.group_members(*id);
            let Some(start) = members.iter().filter_map(|m| self.clip(*m)).map(|c| c.start).reduce(f64::min) else { continue };
            let delta = self.snap_to_grid(start, division).max(0.0) - start;
            for member in members {
                if let Some(c) = self.clip_mut(member) {
                    c.start += delta;
                }
                done.insert(member);
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
    fn reversed_clip_can_extend_at_its_start() {
        let (mut p, _, _, c) = project(); // plays source 2..12 from time 10
        p.clip_mut(c).unwrap().reversed = true;
        p.trim_start(c, 7.0);
        let clip = p.clip(c).unwrap();
        assert_eq!((clip.start, clip.src_in, clip.src_out), (7.0, 2.0, 15.0));
    }

    #[test]
    fn trimming_a_very_short_clip_does_not_panic() {
        let (mut p, _, _, c) = project();
        p.clip_mut(c).unwrap().src_out = 2.001; // 1 ms long, under the minimum
        p.trim_start(c, 10.0005);
        p.trim_end(c, 10.0005);
        p.clip_mut(c).unwrap().src_in = 0.0;
        p.clip_mut(c).unwrap().src_out = 20.0; // no room left to extend
        p.set_stretch(c, 0.1);
        p.trim_end(c, 100.0);
        assert!(p.clip(c).is_some());
    }

    #[test]
    fn swapping_a_take_keeps_position_and_edits() {
        let (mut p, _, s, c) = project();
        p.clip_mut(c).unwrap().pitch = 3.0;
        p.clip_mut(c).unwrap().reversed = true;
        p.swap_clip_audio(c, s, 5.0, 5.5);
        let clip = p.clip(c).unwrap();
        assert_eq!((clip.start, clip.src_in, clip.src_out, clip.pitch, clip.reversed), (10.0, 5.0, 5.5, 3.0, true));
    }

    fn pieces(p: &mut Project) -> (TrackId, ClipId, ClipId) {
        let t = p.add_track("Sentence", TrackKind::Vocal);
        let s = p.sources[0].id;
        let a = p.add_slice(t, s, 5.0, 1.0, 1.5, Some("z iy".into())).unwrap();
        let b = p.add_slice(t, s, 5.5, 3.0, 3.5, Some("b r".into())).unwrap();
        (t, a, b)
    }

    #[test]
    fn overlapping_clips_crossfade_and_stop_when_they_part() {
        let (mut p, _, _, _) = project();
        let (_, a, b) = pieces(&mut p);
        p.clip_mut(b).unwrap().start = 5.4;
        p.update_auto_crossfades();
        assert!((p.clip(a).unwrap().fade_out - 0.1).abs() < 1e-9);
        assert!((p.clip(b).unwrap().fade_in - 0.1).abs() < 1e-9);
        p.clip_mut(b).unwrap().start = 7.0;
        p.update_auto_crossfades();
        assert_eq!(p.clip(a).unwrap().fade_out, SPLICE_FADE);
        assert_eq!(p.clip(b).unwrap().fade_in, SPLICE_FADE);
    }

    #[test]
    fn a_word_over_a_whole_stem_is_not_a_crossfade() {
        let (mut p, t, s, stem) = project(); // the stem runs 10..20
        let word = p.add_slice(t, s, 12.0, 1.0, 1.5, None).unwrap();
        p.update_auto_crossfades();
        assert_eq!(p.clip(stem).unwrap().fade_out, 0.0);
        assert_eq!(p.clip(word).unwrap().fade_in, SPLICE_FADE);
    }

    #[test]
    fn crossfading_overlaps_clips_in_order() {
        let (mut p, _, _, _) = project();
        let (_, a, b) = pieces(&mut p);
        p.crossfade_clips(&[b, a], 0.02);
        assert!((p.clip(b).unwrap().start - 5.48).abs() < 1e-9);
        assert!((p.clip(a).unwrap().fade_out - 0.02).abs() < 1e-9);
    }

    #[test]
    fn saved_words_are_copies_that_outlive_their_clips() {
        let (mut p, _, _, _) = project();
        let (t, a, b) = pieces(&mut p);
        let word = p.save_word("zebra", &[a, b]).unwrap();
        assert_eq!(p.word(word).unwrap().parts[0].start, 0.0);
        assert_eq!(p.group_members(a).len(), 2);
        p.remove_clip(a);
        p.remove_clip(b);
        let placed = p.place_word(word, t, 30.0);
        assert_eq!(placed.len(), 2);
        assert_eq!(p.clip(placed[1]).unwrap().start, 30.5);
        // Editing a placed copy leaves the saved word alone, and reverting gets it back.
        p.clip_mut(placed[1]).unwrap().start = 40.0;
        p.clip_mut(placed[1]).unwrap().pitch = 5.0;
        assert_eq!(p.word(word).unwrap().parts[1].pitch, 0.0);
        let fresh = p.revert_to_saved(placed[0]);
        assert_eq!(p.clip(fresh[1]).unwrap().start, 30.5);
        assert_eq!(p.clip(fresh[1]).unwrap().pitch, 0.0);
    }

    #[test]
    fn saving_under_the_same_name_updates_the_word() {
        let (mut p, _, _, _) = project();
        let (_, a, b) = pieces(&mut p);
        let first = p.save_word("zebra", &[a]).unwrap();
        let second = p.save_word("Zebra", &[a, b]).unwrap();
        assert_eq!(first, second);
        assert_eq!(p.words.len(), 1);
        assert_eq!(p.word(first).unwrap().parts.len(), 2);
    }

    #[test]
    fn restore_undoes_everything_but_position() {
        let (mut p, _, _, _) = project();
        let (_, a, _) = pieces(&mut p);
        p.trim_end(a, 5.2);
        p.set_stretch(a, 2.0);
        let clip = p.clip_mut(a).unwrap();
        clip.pitch = 7.0;
        clip.reversed = true;
        clip.start = 8.0;
        p.restore_original(a);
        let clip = p.clip(a).unwrap();
        assert_eq!((clip.src_in, clip.src_out, clip.stretch, clip.pitch, clip.reversed, clip.start), (1.0, 1.5, 1.0, 0.0, false, 8.0));
    }

    #[test]
    fn quantizing_a_group_keeps_it_together() {
        let (mut p, _, _, _) = project();
        let (_, a, b) = pieces(&mut p);
        p.group_clips(&[a, b], None);
        p.clip_mut(a).unwrap().start = 5.1;
        p.clip_mut(b).unwrap().start = 5.6;
        p.quantize(&[a, b], 1.0); // 120 BPM, so beats every 0.5 s
        assert_eq!(p.clip(a).unwrap().start, 5.0);
        assert!((p.clip(b).unwrap().start - 5.5).abs() < 1e-9);
    }

    #[test]
    fn duplicated_groups_stay_separate_from_the_original() {
        let (mut p, _, _, _) = project();
        let (_, a, b) = pieces(&mut p);
        p.group_clips(&[a, b], None);
        let copies = p.duplicate_clips(&[a, b], 10.0);
        assert_eq!(p.group_members(copies[0]).len(), 2);
        assert_ne!(p.clip(copies[0]).unwrap().group, p.clip(a).unwrap().group);
    }

    #[test]
    fn old_projects_without_these_fields_still_load() {
        let (p, _, _, _) = project();
        let mut json: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("words");
        for clip in json["tracks"][0]["clips"].as_array_mut().unwrap() {
            let clip = clip.as_object_mut().unwrap();
            for field in ["group", "word_name", "made_word", "origin", "fade_in_auto", "fade_out_auto"] {
                clip.remove(field);
            }
        }
        assert!(Project::from_json(&json.to_string()).is_ok());
    }

    #[test]
    fn stretching_keeps_fades_inside_the_clip() {
        let (mut p, _, _, c) = project();
        p.clip_mut(c).unwrap().fade_out = 9.0;
        p.set_stretch(c, 0.5); // now 5 s long
        let clip = p.clip(c).unwrap();
        assert!(clip.fade_in + clip.fade_out <= clip.length());
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
