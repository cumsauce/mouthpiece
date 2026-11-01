//! Changes: alternatives to part of a recording, on the same track as it.
//!
//! A change hides a stretch of the track's own clips while it's active and plays its clips instead.
//! Nothing underneath is touched, so going back is only ever a switch. A change can hold several
//! versions, and only the active one's clips are on the track, so they work like any other clips.

use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatchVersion {
    /// What this version says.
    pub text: String,
    /// The clips, while this isn't the active version. The active one's are on the track.
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub id: PatchId,
    pub track: TrackId,
    /// The stretch of the arrangement being replaced.
    pub start: Seconds,
    pub end: Seconds,
    /// What the recording says here.
    pub before: String,
    /// Which words of which transcript this replaces, so that it can be rewritten again.
    pub source: SourceId,
    pub first_word: usize,
    pub last_word: usize,
    pub versions: Vec<PatchVersion>,
    /// The version playing, or `None` for the original.
    pub active: Option<usize>,
    /// The version to return to when switching back from the original.
    pub last_active: usize,
}

impl Patch {
    /// The version playing, or that would play when switched on.
    pub fn current_text(&self) -> &str {
        self.versions.get(self.active.unwrap_or(self.last_active)).map_or("", |v| v.text.as_str())
    }
}

impl Project {
    pub fn patch(&self, id: PatchId) -> Option<&Patch> {
        self.patches.iter().find(|p| p.id == id)
    }

    /// Starts a change with no versions yet, which plays the original until one is added.
    pub fn create_patch(&mut self, track: TrackId, range: (Seconds, Seconds), before: String, words: (SourceId, usize, usize)) -> PatchId {
        let id = PatchId(self.alloc_id());
        self.patches.push(Patch {
            id,
            track,
            start: range.0,
            end: range.1,
            before,
            source: words.0,
            first_word: words.1,
            last_word: words.2,
            versions: Vec::new(),
            active: None,
            last_active: 0,
        });
        id
    }

    /// Adds a version made of `clips`, which are already on the track, and makes it active.
    pub fn add_patch_version(&mut self, id: PatchId, text: String, clips: &[ClipId]) {
        // The clips being added must stay out while the previous version is put away.
        for clip in clips {
            if let Some(c) = self.clip_mut(*clip) {
                c.patch = None;
            }
        }
        self.stash_active_clips(id);
        for clip in clips {
            if let Some(c) = self.clip_mut(*clip) {
                c.patch = Some(id);
            }
        }
        if let Some(patch) = self.patches.iter_mut().find(|p| p.id == id) {
            patch.versions.push(PatchVersion { text, clips: Vec::new() });
            patch.active = Some(patch.versions.len() - 1);
            patch.last_active = patch.versions.len() - 1;
        }
    }

    /// Moves the active version's clips off the track, into the version.
    fn stash_active_clips(&mut self, id: PatchId) {
        let Some(patch) = self.patch(id) else { return };
        let (track, active) = (patch.track, patch.active);
        let mut stashed = Vec::new();
        if let Some(t) = self.track_mut(track) {
            let mut i = 0;
            while i < t.clips.len() {
                if t.clips[i].patch == Some(id) {
                    stashed.push(t.clips.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        if let (Some(patch), Some(active)) = (self.patches.iter_mut().find(|p| p.id == id), active) {
            if let Some(version) = patch.versions.get_mut(active) {
                version.clips = stashed;
            }
        }
    }

    /// Plays a version, or the original with `None`.
    pub fn set_patch_active(&mut self, id: PatchId, version: Option<usize>) {
        self.stash_active_clips(id);
        let Some(patch) = self.patches.iter_mut().find(|p| p.id == id) else { return };
        patch.active = version.filter(|v| *v < patch.versions.len());
        let Some(active) = patch.active else { return };
        patch.last_active = active;
        let (track, clips) = (patch.track, std::mem::take(&mut patch.versions[active].clips));
        if let Some(t) = self.track_mut(track) {
            t.clips.extend(clips);
        }
    }

    /// Switches between the original and the last version played.
    pub fn toggle_patch(&mut self, id: PatchId) {
        let Some(patch) = self.patch(id) else { return };
        let next = if patch.active.is_some() { None } else { Some(patch.last_active) };
        self.set_patch_active(id, next);
    }

    /// Removes a change, which puts the recording back as it was.
    pub fn delete_patch(&mut self, id: PatchId) {
        self.stash_active_clips(id);
        self.patches.retain(|p| p.id != id);
    }

    pub fn delete_patch_version(&mut self, id: PatchId, version: usize) {
        self.stash_active_clips(id);
        let Some(patch) = self.patches.iter_mut().find(|p| p.id == id) else { return };
        if version >= patch.versions.len() {
            return;
        }
        patch.versions.remove(version);
        let previous = patch.active;
        patch.active = None;
        patch.last_active = patch.last_active.min(patch.versions.len().saturating_sub(1));
        let next = previous.map(|v| if v > version { v - 1 } else { v.min(patch.versions.len().saturating_sub(1)) });
        if patch.versions.is_empty() {
            self.patches.retain(|p| p.id != id);
        } else {
            self.set_patch_active(id, next);
        }
    }

    /// The stretch of the track a change hides: what it replaces, plus anywhere its clips run past that.
    pub fn patch_range(&self, patch: &Patch) -> (Seconds, Seconds) {
        let clips = self.track(patch.track).into_iter().flat_map(|t| &t.clips).filter(|c| c.patch == Some(patch.id));
        clips.fold((patch.start, patch.end), |(a, b), c| (a.min(c.start), b.max(c.end())))
    }

    /// The stretches of a track that changes are hiding right now.
    pub fn hidden_ranges(&self, track: TrackId) -> Vec<(Seconds, Seconds)> {
        if self.bypass_patches {
            return Vec::new();
        }
        self.patches
            .iter()
            .filter(|p| p.track == track && p.active.is_some())
            .map(|p| self.patch_range(p))
            .collect()
    }

    /// Whether a clip is playing at a time, given the changes on its track.
    pub fn clip_heard_at(&self, track: TrackId, clip: &Clip, t: Seconds) -> bool {
        match clip.patch {
            Some(_) => !self.bypass_patches,
            None => !self.hidden_ranges(track).iter().any(|(a, b)| *a <= t && t < *b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> (Project, TrackId, SourceId) {
        let mut p = Project::default();
        let t = p.add_track("Vocals", TrackKind::Vocal);
        let s = p.add_source("v.wav".into(), "v".into(), 60.0, TrackKind::Vocal);
        p.add_clip(t, s, 0.0, 0.0, 60.0).unwrap();
        (p, t, s)
    }

    fn version(p: &mut Project, t: TrackId, s: SourceId, id: PatchId, at: f64, text: &str) -> ClipId {
        let clip = p.add_slice(t, s, at, 30.0, 30.5, Some(text.to_string())).unwrap();
        p.add_patch_version(id, text.to_string(), &[clip]);
        clip
    }

    #[test]
    fn a_change_hides_its_range_while_active_and_nothing_when_not() {
        let (mut p, t, s) = song();
        let id = p.create_patch(t, (10.0, 11.0), "old".into(), (s, 0, 0));
        assert!(p.hidden_ranges(t).is_empty());
        version(&mut p, t, s, id, 10.0, "new");
        assert_eq!(p.hidden_ranges(t), [(10.0, 11.0)]);
        p.toggle_patch(id);
        assert!(p.hidden_ranges(t).is_empty());
        assert_eq!(p.track(t).unwrap().clips.len(), 1, "the version's clips leave the track");
        p.toggle_patch(id);
        assert_eq!(p.track(t).unwrap().clips.len(), 2);
    }

    #[test]
    fn versions_keep_their_own_clips_and_edits() {
        let (mut p, t, s) = song();
        let id = p.create_patch(t, (10.0, 11.0), "old".into(), (s, 0, 0));
        let first = version(&mut p, t, s, id, 10.0, "one");
        p.clip_mut(first).unwrap().pitch = 3.0;
        let second = version(&mut p, t, s, id, 10.2, "two");
        assert!(p.clip(first).is_none(), "the first version is put away");
        assert!(p.clip(second).is_some());
        p.set_patch_active(id, Some(0));
        assert_eq!(p.clip(first).unwrap().pitch, 3.0);
        assert!(p.clip(second).is_none());
        assert_eq!(p.patch(id).unwrap().current_text(), "one");
    }

    #[test]
    fn deleting_a_change_leaves_the_recording_as_it_was() {
        let (mut p, t, s) = song();
        let before = p.track(t).unwrap().clips.clone();
        let id = p.create_patch(t, (10.0, 11.0), "old".into(), (s, 0, 0));
        version(&mut p, t, s, id, 10.0, "new");
        p.delete_patch(id);
        assert_eq!(p.track(t).unwrap().clips, before);
        assert!(p.hidden_ranges(t).is_empty());
    }

    #[test]
    fn deleting_a_version_moves_to_a_remaining_one() {
        let (mut p, t, s) = song();
        let id = p.create_patch(t, (10.0, 11.0), "old".into(), (s, 0, 0));
        version(&mut p, t, s, id, 10.0, "one");
        version(&mut p, t, s, id, 10.0, "two");
        p.delete_patch_version(id, 1);
        assert_eq!(p.patch(id).unwrap().active, Some(0));
        assert_eq!(p.patch(id).unwrap().current_text(), "one");
        p.delete_patch_version(id, 0);
        assert!(p.patch(id).is_none(), "a change with no versions goes away");
    }

    #[test]
    fn hiding_grows_with_clips_that_run_long() {
        let (mut p, t, s) = song();
        let id = p.create_patch(t, (10.0, 10.2), "old".into(), (s, 0, 0));
        version(&mut p, t, s, id, 10.0, "longer");
        assert_eq!(p.hidden_ranges(t), [(10.0, 10.5)]);
    }

    #[test]
    fn bypassing_hides_every_change_and_plays_the_original() {
        let (mut p, t, s) = song();
        let id = p.create_patch(t, (10.0, 11.0), "old".into(), (s, 0, 0));
        let new = version(&mut p, t, s, id, 10.0, "new");
        let original = p.track(t).unwrap().clips[0].clone();
        let new = p.clip(new).unwrap().clone();
        assert!(!p.clip_heard_at(t, &original, 10.5));
        assert!(p.clip_heard_at(t, &new, 10.2));
        p.bypass_patches = true;
        assert!(p.clip_heard_at(t, &original, 10.5));
        assert!(!p.clip_heard_at(t, &new, 10.2));
    }
}
