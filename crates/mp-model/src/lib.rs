//! The project document: plain data, pure edit operations and undo history.
//!
//! Nothing here touches audio or UI. Every edit is a method on [`Project`], so edits can be
//! tested directly and the UI stays a thin layer over them.

mod edit;
mod history;

pub use history::History;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Time on the arrangement, in seconds. Seconds rather than beats so stems never drift when the
/// tempo changes: the beat grid is only a view over this timeline.
pub type Seconds = f64;

macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u64);
    )*};
}
id_type!(SourceId, TrackId, ClipId, DeviceId);

/// Minimum clip length on the arrangement, so a clip can always be seen and grabbed.
pub const MIN_CLIP_LEN: Seconds = 0.005;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f64,
    /// Time of the first downbeat, as found by beat tracking.
    pub grid_offset: Seconds,
    pub beats_per_bar: u32,
    pub key: Option<MusicalKey>,
    pub tracks: Vec<Track>,
    pub sources: Vec<Source>,
    pub master: Master,
    pub loop_region: LoopRegion,
    pub metronome: bool,
    next_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicalKey {
    /// 0 = C ... 11 = B.
    pub root: u8,
    pub minor: bool,
}

impl MusicalKey {
    pub const NOTE_NAMES: [&'static str; 12] =
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

    pub fn label(&self) -> String {
        format!("{} {}", Self::NOTE_NAMES[self.root as usize % 12], if self.minor { "min" } else { "maj" })
    }

    /// Semitone offsets from the root that belong to the scale.
    pub fn scale(&self) -> [u8; 7] {
        if self.minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] }
    }

    /// Smallest shift in semitones that moves `midi` onto a note of this scale.
    pub fn snap_semitones(&self, midi: f32) -> f32 {
        let mut best = f32::MAX;
        for octave in -1..=11 {
            for degree in self.scale() {
                let note = (octave * 12 + self.root as i32 + degree as i32) as f32;
                let delta = note - midi;
                if delta.abs() < best.abs() {
                    best = delta;
                }
            }
        }
        best
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub path: PathBuf,
    pub name: String,
    pub duration: Seconds,
    pub bpm: Option<f64>,
    pub kind: TrackKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Vocal,
    Drums,
    Bass,
    Music,
}

impl TrackKind {
    /// Guess what a stem is from its filename.
    pub fn from_filename(name: &str) -> Self {
        let n = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
        if has(&["vocal", "vox", "voice", "acapella", "acappella", "speech", "dialog", "spoken"]) {
            TrackKind::Vocal
        } else if has(&["drum", "kick", "snare", "hat", "perc", "beat"]) {
            TrackKind::Drums
        } else if has(&["bass", "sub", "808"]) {
            TrackKind::Bass
        } else {
            TrackKind::Music
        }
    }

    /// Position in the track list.
    pub fn order(self) -> u8 {
        match self {
            TrackKind::Vocal => 0,
            TrackKind::Drums => 1,
            TrackKind::Bass => 2,
            TrackKind::Music => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrackKind::Vocal => "Vocal",
            TrackKind::Drums => "Drums",
            TrackKind::Bass => "Bass",
            TrackKind::Music => "Music",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub kind: TrackKind,
    /// Index into the UI palette.
    pub color: u8,
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub height: f32,
    pub clips: Vec<Clip>,
    pub devices: Vec<DeviceInstance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClipMode {
    /// Time and pitch are independent (phase-vocoder based stretcher).
    Elastic,
    /// Tape-style: stretching also changes the pitch.
    Varispeed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub source: SourceId,
    /// Position on the arrangement.
    pub start: Seconds,
    /// Range of the source that the clip plays.
    pub src_in: Seconds,
    pub src_out: Seconds,
    pub gain_db: f32,
    pub pan: f32,
    pub fade_in: Seconds,
    pub fade_out: Seconds,
    pub reversed: bool,
    /// Output length divided by source length. 2.0 is half speed.
    pub stretch: f64,
    pub pitch: f32,
    pub mode: ClipMode,
    pub preserve_formants: bool,
    pub muted: bool,
    /// The word or phoneme this clip was cut from, shown on the clip.
    pub label: Option<String>,
}

impl Clip {
    pub fn new(id: ClipId, source: SourceId, start: Seconds, src_in: Seconds, src_out: Seconds) -> Self {
        Self {
            id,
            source,
            start,
            src_in,
            src_out,
            gain_db: 0.0,
            pan: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            reversed: false,
            stretch: 1.0,
            pitch: 0.0,
            mode: ClipMode::Elastic,
            preserve_formants: true,
            muted: false,
            label: None,
        }
    }

    pub fn src_len(&self) -> Seconds {
        self.src_out - self.src_in
    }

    pub fn length(&self) -> Seconds {
        self.src_len() * self.stretch
    }

    pub fn end(&self) -> Seconds {
        self.start + self.length()
    }

    /// Whether playback needs a stretched or repitched render rather than the raw source.
    pub fn needs_render(&self) -> bool {
        (self.stretch - 1.0).abs() > 1e-6 || self.effective_pitch().abs() > 1e-4
    }

    /// Pitch change heard, including the shift varispeed causes.
    pub fn effective_pitch(&self) -> f32 {
        match self.mode {
            ClipMode::Elastic => self.pitch,
            ClipMode::Varispeed => (-12.0 * self.stretch.log2()) as f32,
        }
    }

    /// Maps an arrangement time inside the clip to the source position heard at that time.
    pub fn source_time_at(&self, t: Seconds) -> Seconds {
        let d = ((t - self.start) / self.stretch).clamp(0.0, self.src_len());
        if self.reversed { self.src_out - d } else { self.src_in + d }
    }

    /// Inverse of [`Self::source_time_at`].
    pub fn time_of_source(&self, s: Seconds) -> Seconds {
        let d = if self.reversed { self.src_out - s } else { s - self.src_in };
        self.start + d * self.stretch
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceInstance {
    pub id: DeviceId,
    pub kind: DeviceKind,
    pub enabled: bool,
    /// Parameter values by stable parameter id. Ids are never reused, so old projects keep loading.
    pub params: BTreeMap<u32, f32>,
}

/// What a device is. `Clap` reserves the project format for hosted plugins: it stores everything
/// needed to re-instantiate one, and engine code only sees a `Device` trait object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DeviceKind {
    Utility,
    Eq,
    Compressor,
    Gate,
    Saturator,
    Delay,
    Reverb,
    Limiter,
    Clap { path: PathBuf, plugin_id: String, state: Vec<u8> },
}

impl DeviceKind {
    pub fn name(&self) -> &str {
        match self {
            DeviceKind::Utility => "Utility",
            DeviceKind::Eq => "EQ",
            DeviceKind::Compressor => "Compressor",
            DeviceKind::Gate => "Gate",
            DeviceKind::Saturator => "Saturator",
            DeviceKind::Delay => "Delay",
            DeviceKind::Reverb => "Reverb",
            DeviceKind::Limiter => "Limiter",
            DeviceKind::Clap { plugin_id, .. } => plugin_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Master {
    pub gain_db: f32,
    pub devices: Vec<DeviceInstance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoopRegion {
    pub start: Seconds,
    pub end: Seconds,
    pub enabled: bool,
}

impl Default for Project {
    fn default() -> Self {
        let mut p = Self {
            name: "Untitled".into(),
            bpm: 120.0,
            grid_offset: 0.0,
            beats_per_bar: 4,
            key: None,
            tracks: Vec::new(),
            sources: Vec::new(),
            master: Master { gain_db: 0.0, devices: Vec::new() },
            loop_region: LoopRegion { start: 0.0, end: 8.0, enabled: false },
            metronome: false,
            next_id: 1,
        };
        let id = DeviceId(p.alloc_id());
        p.master.devices.push(DeviceInstance {
            id,
            kind: DeviceKind::Limiter,
            enabled: true,
            params: BTreeMap::new(),
        });
        p
    }
}

impl Project {
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn beat_seconds(&self) -> Seconds {
        60.0 / self.bpm
    }

    pub fn duration(&self) -> Seconds {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(Clip::end)
            .fold(0.0, f64::max)
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).find(|c| c.id == id)
    }

    pub fn track_of_clip(&self, id: ClipId) -> Option<TrackId> {
        self.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == id)).map(|t| t.id)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}
