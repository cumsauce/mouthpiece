//! Reading and writing project files, so that nothing that goes wrong can cost someone their work: a crash
//! mid-save, a full disk, a damaged file, a moved folder, or a file from a newer version.

use crate::{Clip, Project, Source};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Bump when a saved project would lose something if read by an older version.
pub const FORMAT_VERSION: u32 = 1;
const BACKUPS: usize = 3;

/// Replaces a file in one step: it's either the old file or the whole new one, even after a power cut.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map_or("file".into(), |n| n.to_string_lossy().into_owned());
    let unique = format!(".{name}.{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let tmp = path.with_file_name(unique);
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        // The rename itself only survives a power cut once the folder is on disk too.
        #[cfg(unix)]
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// Saved by a newer version. Opening it could lose whatever that version added.
    Newer(u32),
    Unreadable(String),
    Damaged(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Newer(_) => write!(f, "This project was saved by a newer version of Mouthpiece. Update to open it."),
            LoadError::Unreadable(e) => write!(f, "Can't read this project: {e}"),
            LoadError::Damaged(e) => write!(f, "This project file is damaged: {e}"),
        }
    }
}

/// The previous saves of a project, newest first.
pub fn backup_paths(path: &Path) -> Vec<PathBuf> {
    let name = path.file_name().map_or("project".into(), |n| n.to_string_lossy().into_owned());
    (1..=BACKUPS).map(|i| path.with_file_name(format!("{name}.backup{i}"))).collect()
}

fn keep_backup(path: &Path) {
    let backups = backup_paths(path);
    for pair in backups.windows(2).rev() {
        let _ = std::fs::rename(&pair[0], &pair[1]);
    }
    // Copied, because the project stays in place until the new one replaces it.
    let _ = std::fs::copy(path, &backups[0]);
}

/// `target` as seen from `base`, for finding a file again when a whole folder has moved.
fn relative_to(base: &Path, target: &Path) -> Option<PathBuf> {
    let (base, target): (Vec<_>, Vec<_>) = (base.components().collect(), target.components().collect());
    if base.first() != target.first() || !matches!(base.first(), Some(Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    let shared = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    base[shared..].iter().for_each(|_| out.push(".."));
    target[shared..].iter().for_each(|c| out.push(c));
    Some(out)
}

fn record_relative_paths(project: &mut Project, folder: &Path) {
    for source in &mut project.sources {
        source.relative_path = relative_to(folder, &source.path);
        source.relative_picture = source.picture_path.as_deref().and_then(|p| relative_to(folder, p));
    }
}

/// Finds a file that isn't where it was: moved along with the project, or sitting beside it.
fn find_moved(recorded: &Path, relative: Option<&Path>, folder: Option<&Path>) -> Option<PathBuf> {
    if recorded.exists() {
        return None;
    }
    let folder = folder?;
    let along = relative.map(|r| folder.join(r)).filter(|p| p.exists());
    along.or_else(|| recorded.file_name().map(|n| folder.join(n)).filter(|p| p.exists()))
}

fn resolve_paths(source: &mut Source, folder: Option<&Path>) {
    if let Some(found) = find_moved(&source.path, source.relative_path.as_deref(), folder) {
        source.path = found;
    }
    if let Some(picture) = source.picture_path.clone() {
        if let Some(found) = find_moved(&picture, source.relative_picture.as_deref(), folder) {
            source.picture_path = Some(found);
        }
    }
    source.relative_path = None;
    source.relative_picture = None;
}

fn finite64(value: &mut f64, fallback: f64, repairs: &mut usize) {
    if !value.is_finite() {
        *value = fallback;
        *repairs += 1;
    }
}

fn finite32(value: &mut f32, fallback: f32, repairs: &mut usize) {
    if !value.is_finite() {
        *value = fallback;
        *repairs += 1;
    }
}

/// Puts a clip back within what the engine expects. Returns false if nothing usable is left of it.
fn repair_clip(clip: &mut Clip, repairs: &mut usize) -> bool {
    finite64(&mut clip.start, 0.0, repairs);
    finite64(&mut clip.src_in, 0.0, repairs);
    finite64(&mut clip.src_out, 0.0, repairs);
    finite64(&mut clip.stretch, 1.0, repairs);
    finite64(&mut clip.fade_in, 0.0, repairs);
    finite64(&mut clip.fade_out, 0.0, repairs);
    finite32(&mut clip.gain_db, 0.0, repairs);
    finite32(&mut clip.pan, 0.0, repairs);
    finite32(&mut clip.pitch, 0.0, repairs);
    if clip.stretch <= 0.0 {
        clip.stretch = 1.0;
        *repairs += 1;
    }
    clip.start = clip.start.max(0.0);
    clip.src_in = clip.src_in.max(0.0);
    clip.fade_in = clip.fade_in.max(0.0);
    clip.fade_out = clip.fade_out.max(0.0);
    clip.src_out > clip.src_in
}

impl Project {
    /// Repairs anything out of range, such as a value that isn't a number, which JSON can't even store.
    /// Returns how many things it had to change, so that a person can be told.
    pub fn sanitize(&mut self) -> usize {
        let mut repairs = 0;
        finite64(&mut self.bpm, 120.0, &mut repairs);
        if self.bpm <= 0.0 {
            self.bpm = 120.0;
            repairs += 1;
        }
        finite64(&mut self.grid_offset, 0.0, &mut repairs);
        finite64(&mut self.loop_region.start, 0.0, &mut repairs);
        finite64(&mut self.loop_region.end, 0.0, &mut repairs);
        self.beats_per_bar = self.beats_per_bar.max(1);
        finite32(&mut self.master.gain_db, 0.0, &mut repairs);

        let sources: std::collections::HashSet<_> = self.sources.iter().map(|s| s.id).collect();
        let tracks: std::collections::HashSet<_> = self.tracks.iter().map(|t| t.id).collect();
        let before = self.patches.len();
        self.patches.retain(|p| tracks.contains(&p.track));
        repairs += before - self.patches.len();
        let patches: std::collections::HashSet<_> = self.patches.iter().map(|p| p.id).collect();

        let mut keep_clip = |clip: &mut Clip| -> bool {
            let usable = repair_clip(clip, &mut repairs) && sources.contains(&clip.source);
            if !usable {
                repairs += 1;
            }
            if clip.patch.is_some_and(|p| !patches.contains(&p)) {
                clip.patch = None;
                repairs += 1;
            }
            usable
        };
        for track in &mut self.tracks {
            track.clips.retain_mut(&mut keep_clip);
        }
        for version in self.patches.iter_mut().flat_map(|p| &mut p.versions) {
            version.clips.retain_mut(&mut keep_clip);
        }
        for track in &mut self.tracks {
            finite32(&mut track.gain_db, 0.0, &mut repairs);
            finite32(&mut track.pan, 0.0, &mut repairs);
            finite32(&mut track.height, 76.0, &mut repairs);
        }
        let devices = self.tracks.iter_mut().flat_map(|t| &mut t.devices).chain(&mut self.master.devices);
        for device in devices {
            let count = device.params.len();
            device.params.retain(|_, v| v.is_finite());
            repairs += count - device.params.len();
        }
        let effects = self.tracks.iter_mut().flat_map(|t| &mut t.picture).chain(&mut self.master.picture);
        for effect in effects {
            let count = effect.params.len();
            effect.params.retain(|_, v| v.is_finite());
            repairs += count - effect.params.len();
            for motion in effect.motion.values_mut() {
                let count = motion.keys.len();
                motion.keys.retain(|k| k.t.is_finite() && k.value.is_finite());
                repairs += count - motion.keys.len();
            }
        }

        // A new id must never match an old one, or an edit would land on the wrong thing.
        let highest = self
            .sources
            .iter()
            .map(|s| s.id.0)
            .chain(self.tracks.iter().map(|t| t.id.0))
            .chain(self.tracks.iter().flat_map(|t| &t.clips).flat_map(|c| [Some(c.id.0), c.group.map(|g| g.0)]).flatten())
            .chain(self.patches.iter().flat_map(|p| &p.versions).flat_map(|v| &v.clips).map(|c| c.id.0))
            .chain(self.patches.iter().map(|p| p.id.0))
            .chain(self.words.iter().map(|w| w.id.0))
            .chain(self.tracks.iter().flat_map(|t| t.devices.iter().map(|d| d.id.0).chain(t.picture.iter().map(|e| e.id.0))))
            .chain(self.master.devices.iter().map(|d| d.id.0))
            .chain(self.master.picture.iter().map(|e| e.id.0))
            .max()
            .unwrap_or(0);
        if self.next_id <= highest {
            self.next_id = highest + 1;
            repairs += 1;
        }
        repairs
    }

    /// The text of a project file, checked by reading it back first.
    pub fn to_saved_text(&self, folder: Option<&Path>) -> Result<String, String> {
        let mut copy = self.clone();
        copy.sanitize();
        copy.format_version = FORMAT_VERSION;
        if let Some(folder) = folder {
            record_relative_paths(&mut copy, folder);
        }
        let text = copy.to_json().map_err(|e| e.to_string())?;
        Project::from_json(&text).map_err(|e| format!("it wouldn't read back: {e}"))?;
        Ok(text)
    }

    /// Saves a project. Unless it's an autosave, the previous saves are kept as backups.
    pub fn save_file(&self, path: &Path, keep_backups: bool) -> Result<(), String> {
        let text = self.to_saved_text(path.parent())?;
        if keep_backups && path.exists() {
            keep_backup(path);
        }
        atomic_write(path, text.as_bytes()).map_err(|e| e.to_string())
    }

    /// Reads a project, finds files that moved with it, and repairs what it can. Also returns how much it repaired.
    pub fn load_file(path: &Path) -> Result<(Project, usize), LoadError> {
        let text = std::fs::read_to_string(path).map_err(|e| LoadError::Unreadable(e.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| LoadError::Damaged(e.to_string()))?;
        let version = value["format_version"].as_u64().unwrap_or(0) as u32;
        if version > FORMAT_VERSION {
            return Err(LoadError::Newer(version));
        }
        let mut project: Project = serde_json::from_value(value).map_err(|e| LoadError::Damaged(e.to_string()))?;
        let repairs = project.sanitize();
        project.format_version = FORMAT_VERSION;
        for source in &mut project.sources {
            resolve_paths(source, path.parent());
        }
        Ok((project, repairs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackKind;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mp-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project_with_clip() -> Project {
        let mut p = Project::default();
        let t = p.add_track("T", TrackKind::Music);
        let s = p.add_source("a.wav".into(), "a".into(), 10.0, TrackKind::Music);
        p.add_clip(t, s, 0.0, 0.0, 5.0);
        p
    }

    #[test]
    fn saving_then_loading_gives_the_same_project() {
        let dir = scratch("round");
        let path = dir.join("p.mouthpiece");
        let p = project_with_clip();
        p.save_file(&path, true).unwrap();
        let (back, repairs) = Project::load_file(&path).unwrap();
        assert_eq!(repairs, 0);
        assert_eq!(back, p);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("tmp")).collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn every_save_keeps_the_previous_ones() {
        let dir = scratch("backups");
        let path = dir.join("p.mouthpiece");
        for bpm in [100.0, 110.0, 120.0, 130.0, 140.0] {
            let mut p = Project::default();
            p.bpm = bpm;
            p.save_file(&path, true).unwrap();
        }
        let bpm = |p: &Path| Project::load_file(p).unwrap().0.bpm;
        let backups = backup_paths(&path);
        assert_eq!(bpm(&path), 140.0);
        assert_eq!(backups.iter().map(|b| bpm(b)).collect::<Vec<_>>(), [130.0, 120.0, 110.0]);
    }

    #[test]
    fn a_value_that_is_not_a_number_does_not_make_the_file_unreadable() {
        let dir = scratch("nan");
        let path = dir.join("p.mouthpiece");
        let mut p = project_with_clip();
        p.tracks[0].gain_db = f32::NAN;
        p.tracks[0].clips[0].stretch = f64::INFINITY;
        p.bpm = f64::NAN;
        p.save_file(&path, false).unwrap();
        let (back, _) = Project::load_file(&path).unwrap();
        assert_eq!(back.tracks[0].gain_db, 0.0);
        assert_eq!(back.tracks[0].clips[0].stretch, 1.0);
        assert_eq!(back.bpm, 120.0);
    }

    #[test]
    fn repairs_clips_and_ids_that_cannot_work() {
        let mut p = project_with_clip();
        let good = p.tracks[0].clips[0].clone();
        let mut no_source = good.clone();
        no_source.source = crate::SourceId(999);
        let mut empty = good.clone();
        empty.src_out = empty.src_in;
        p.tracks[0].clips.extend([no_source, empty]);
        p.tracks[0].clips[0].id = crate::ClipId(5000);
        assert!(p.sanitize() > 0);
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert!(p.alloc_id() > 5000);
    }

    #[test]
    fn a_newer_file_is_refused_rather_than_damaged_by_saving_over_it() {
        let dir = scratch("newer");
        let path = dir.join("p.mouthpiece");
        let mut value: serde_json::Value = serde_json::from_str(&Project::default().to_json().unwrap()).unwrap();
        value["format_version"] = (FORMAT_VERSION + 1).into();
        std::fs::write(&path, value.to_string()).unwrap();
        assert_eq!(Project::load_file(&path), Err(LoadError::Newer(FORMAT_VERSION + 1)));
    }

    #[test]
    fn damaged_and_old_files_are_told_apart() {
        let dir = scratch("damaged");
        let path = dir.join("p.mouthpiece");
        std::fs::write(&path, "{ \"bpm\": 12").unwrap();
        assert!(matches!(Project::load_file(&path), Err(LoadError::Damaged(_))));
        // Projects from before there was a version still open.
        let mut value: serde_json::Value = serde_json::from_str(&Project::default().to_json().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("format_version");
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(Project::load_file(&path).is_ok());
    }

    #[test]
    fn files_are_found_when_their_folder_moves_with_the_project() {
        let dir = scratch("moved");
        let old = dir.join("old");
        std::fs::create_dir_all(old.join("Song stems")).unwrap();
        std::fs::write(old.join("Song stems/Vocals.wav"), b"x").unwrap();
        let mut p = Project::default();
        p.add_source(old.join("Song stems/Vocals.wav"), "Vocals".into(), 1.0, TrackKind::Vocal);
        p.save_file(&old.join("p.mouthpiece"), false).unwrap();
        let new = dir.join("elsewhere/new");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        let (back, _) = Project::load_file(&new.join("p.mouthpiece")).unwrap();
        assert_eq!(back.sources[0].path, new.join("Song stems/Vocals.wav"));
        assert_eq!(back.sources[0].relative_path, None, "this only matters in the file");
    }
}
