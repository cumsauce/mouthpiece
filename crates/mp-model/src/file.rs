//! Reading and writing project files, so that nothing that goes wrong can cost someone their work: a crash
//! mid-save, a full disk, a damaged file, a moved folder, or a file from a newer version.

use crate::{Clip, Project, Source};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Bump when a saved project would lose something if read by an older version.
pub const FORMAT_VERSION: u32 = 1;
const BACKUPS: usize = 3;
const EXTENSION: &str = "mouthpiece";
/// The project inside a package. A package is a folder holding this and everything it plays, so that it can
/// be moved or sent as one thing. The inner file means that any system's file dialog can open one too.
pub const PACKAGE_FILE: &str = "project.mouthpiece";
const MEDIA_FOLDER: &str = "media";

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
    let folder = folder?;
    // Inside the project's own folder wins, or a copy of a project would play the original's files.
    if let Some(inside) = relative.filter(|r| !r.starts_with("..")).map(|r| folder.join(r)).filter(|p| p.exists()) {
        return (inside != recorded).then_some(inside);
    }
    if recorded.exists() {
        return None;
    }
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

/// The project file to read for something picked: a package, or a file from before there were packages.
pub fn project_file(picked: &Path) -> PathBuf {
    if picked.is_dir() {
        picked.join(PACKAGE_FILE)
    } else {
        picked.to_path_buf()
    }
}

/// The package a project file is in, if it's in one.
pub fn package_of(file: &Path) -> Option<&Path> {
    let is_inner = file.file_name().is_some_and(|n| n == PACKAGE_FILE);
    file.parent().filter(|p| is_inner && p.extension().is_some_and(|e| e == EXTENSION))
}

/// What a project is called, which is the name of its package or file.
pub fn project_title(file: &Path) -> String {
    let named = package_of(file).unwrap_or(file);
    named.file_stem().map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned())
}

/// Gets a package ready to save to at `path`, and returns its project file. A project file from before there
/// were packages moves inside, with nothing lost, and the first save then keeps it as a backup.
pub fn make_package(path: &Path) -> std::io::Result<PathBuf> {
    if package_of(path).is_some() {
        std::fs::create_dir_all(path.parent().unwrap_or(path))?;
        return Ok(path.to_path_buf());
    }
    let package = if path.extension().is_some_and(|e| e == EXTENSION) {
        path.to_path_buf()
    } else {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{EXTENSION}"));
        PathBuf::from(name)
    };
    if package.is_file() {
        let name = package.file_name().map_or("project".into(), |n| n.to_string_lossy().into_owned());
        let packing = package.with_file_name(format!(".{name}.{}.packing", std::process::id()));
        std::fs::create_dir_all(&packing)?;
        std::fs::rename(&package, packing.join(PACKAGE_FILE))?;
        if let Err(error) = std::fs::rename(&packing, &package) {
            let _ = std::fs::rename(packing.join(PACKAGE_FILE), &package);
            let _ = std::fs::remove_dir(&packing);
            return Err(error);
        }
    } else {
        std::fs::create_dir_all(&package)?;
    }
    Ok(package.join(PACKAGE_FILE))
}

/// The files a project plays that aren't inside its package yet.
pub fn media_to_collect(project: &Project, package: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in project.sources.iter().flat_map(|s| std::iter::once(&s.path).chain(&s.picture_path)) {
        if !path.starts_with(package) && path.is_file() && !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    paths
}

fn same_contents(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut a, mut b) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let read = a.read(&mut x)?;
        if read == 0 {
            return Ok(true);
        }
        b.read_exact(&mut y[..read])?;
        if x[..read] != y[..read] {
            return Ok(false);
        }
    }
}

/// Copies one file into a folder, or finds it's there already. Files that share a name stay apart, as every
/// song's stems do. A file only appears under its final name once it's all there.
fn collect_file(from: &Path, media: &Path) -> std::io::Result<PathBuf> {
    let stem = from.file_stem().map_or("media".into(), |n| n.to_string_lossy().into_owned());
    let extension = from.extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    for n in 1u32.. {
        let name = if n == 1 { format!("{stem}{extension}") } else { format!("{stem} {n}{extension}") };
        let to = media.join(&name);
        if !to.exists() {
            let tmp = media.join(format!(".{name}.tmp"));
            std::fs::copy(from, &tmp)?;
            // Windows can only flush a file opened for writing. A copy of a read-only file can't be opened that
            // way, but nothing depends on it until the project is saved again, so it can go unflushed.
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&tmp) {
                file.sync_all()?;
            }
            std::fs::rename(&tmp, &to)?;
            return Ok(to);
        }
        if same_contents(from, &to)? {
            return Ok(to);
        }
    }
    unreachable!("there's always another number to try")
}

/// Copies files into a package. Returns where each one went and, if it had to stop, why. What was copied by
/// then still counts, and the rest of the project keeps playing the originals.
pub fn collect_media(paths: &[PathBuf], package: &Path, on_progress: &mut dyn FnMut(f32)) -> (Vec<(PathBuf, PathBuf)>, Option<String>) {
    let media = package.join(MEDIA_FOLDER);
    if let Err(error) = std::fs::create_dir_all(&media) {
        return (Vec::new(), Some(error.to_string()));
    }
    let mut moves = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        on_progress(i as f32 / paths.len() as f32);
        match collect_file(path, &media) {
            Ok(to) => moves.push((path.clone(), to)),
            Err(error) => return (moves, Some(error.to_string())),
        }
    }
    (moves, None)
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

    /// Points sources at the copies `collect_media` made.
    pub fn use_collected(&mut self, moves: &[(PathBuf, PathBuf)]) {
        let moved = |path: &mut PathBuf| {
            if let Some((_, to)) = moves.iter().find(|(from, _)| from == path) {
                *path = to.clone();
            }
        };
        for source in &mut self.sources {
            moved(&mut source.path);
            source.picture_path.iter_mut().for_each(moved);
        }
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
    fn a_package_holds_its_media_and_still_works_after_moving_or_copying() {
        let dir = scratch("package");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/Vocals.wav"), b"one").unwrap();
        std::fs::write(dir.join("b/Vocals.wav"), b"two").unwrap();
        let mut p = Project::default();
        p.add_source(dir.join("a/Vocals.wav"), "A".into(), 1.0, TrackKind::Vocal);
        p.add_source(dir.join("b/Vocals.wav"), "B".into(), 1.0, TrackKind::Vocal);

        let file = make_package(&dir.join("Song")).unwrap();
        let package = dir.join("Song.mouthpiece");
        assert_eq!(file, package.join(PACKAGE_FILE));
        assert_eq!(project_title(&file), "Song");
        let (moves, error) = collect_media(&media_to_collect(&p, &package), &package, &mut |_| {});
        assert_eq!((moves.len(), error), (2, None));
        p.use_collected(&moves);
        assert_eq!(std::fs::read(&p.sources[0].path).unwrap(), b"one");
        assert_eq!(std::fs::read(&p.sources[1].path).unwrap(), b"two");
        assert_ne!(p.sources[0].path, p.sources[1].path);
        assert!(media_to_collect(&p, &package).is_empty());
        p.save_file(&file, true).unwrap();

        // Collecting the same files again doesn't make more of them.
        let mut again = Project::default();
        again.add_source(dir.join("a/Vocals.wav"), "A".into(), 1.0, TrackKind::Vocal);
        let (moves, _) = collect_media(&media_to_collect(&again, &package), &package, &mut |_| {});
        assert_eq!(moves[0].1, p.sources[0].path);

        // A copy plays its own files, even with the original still there.
        let copy = dir.join("Copy.mouthpiece");
        std::fs::create_dir_all(copy.join("media")).unwrap();
        std::fs::copy(&file, copy.join(PACKAGE_FILE)).unwrap();
        for entry in std::fs::read_dir(package.join("media")).unwrap().flatten() {
            std::fs::copy(entry.path(), copy.join("media").join(entry.file_name())).unwrap();
        }
        let (back, _) = Project::load_file(&project_file(&copy)).unwrap();
        assert!(back.sources.iter().all(|s| s.path.starts_with(&copy)), "{:?}", back.sources[0].path);
    }

    #[test]
    fn an_old_project_file_becomes_a_package_and_is_kept_as_a_backup() {
        let dir = scratch("convert");
        let old = dir.join("Song.mouthpiece");
        let mut p = Project::default();
        p.bpm = 100.0;
        p.save_file(&old, false).unwrap();
        let file = make_package(&old).unwrap();
        assert!(old.is_dir());
        p.bpm = 140.0;
        p.save_file(&file, true).unwrap();
        assert_eq!(Project::load_file(&project_file(&old)).unwrap().0.bpm, 140.0);
        assert_eq!(Project::load_file(&backup_paths(&file)[0]).unwrap().0.bpm, 100.0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing left over");
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
