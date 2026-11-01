//! Every command in the app, with its name and shortcut. The command palette, the keyboard handler
//! and the shortcut sheet all read from this one table.

use crate::app::{AfterDiscard, App, BottomTab, ToastKind};
use eframe::egui::{Context, Event, Key, KeyboardShortcut, Modifiers};
use mp_model::{Clip, ClipId, ClipMode, DeviceKind, TrackKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    PlayPause,
    Stop,
    ToggleLoop,
    LoopSelection,
    ToggleMetronome,
    Undo,
    Redo,
    New,
    Open,
    Save,
    SaveAs,
    Import,
    Export,
    Split,
    Delete,
    Duplicate,
    Copy,
    Paste,
    SelectAll,
    Reverse,
    MuteClip,
    Quantize,
    CloseGaps,
    SliceToWords,
    SnapPitchToKey,
    PitchUp,
    PitchDown,
    Faster,
    Slower,
    TrySounds,
    ChangeWords,
    CompareChange,
    HearOriginal,
    RestoreOriginal,
    RevertWord,
    Group,
    Ungroup,
    Crossfade,
    SaveWord,
    FineNudgeLeft,
    FineNudgeRight,
    ToggleVideo,
    Varispeed,
    FitToTempo,
    NudgeLeft,
    NudgeRight,
    NudgeBarLeft,
    NudgeBarRight,
    ShowPicture,
    LargePicture,
    RenameTrack,
    AudioSettings,
    SplitStems,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomSelection,
    ToggleSnap,
    ToggleEditor,
    ShowEditor,
    ShowDevices,
    ShowMixer,
    ToggleInspector,
    ToggleSay,
    FocusSay,
    NextTake,
    PrevTake,
    Repeat,
    SoloTrack,
    PlayFromSelection,
    Deselect,
    Palette,
    Shortcuts,
    AddVocalTrack,
    AddTrack,
    DeleteTrack,
    Transcribe,
    PlaceRange,
    AddEq,
    AddCompressor,
    AddGate,
    AddSaturator,
    AddDelay,
    AddReverb,
    AddUtility,
    AddLimiter,
}

const fn key(m: Modifiers, k: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(m, k))
}

const NONE: Modifiers = Modifiers::NONE;
const CMD: Modifiers = Modifiers::COMMAND;
const SHIFT: Modifiers = Modifiers::SHIFT;
const ALT: Modifiers = Modifiers::ALT;
const CMD_SHIFT: Modifiers = Modifiers { command: true, shift: true, ..Modifiers::NONE };

/// (action, palette name, shortcut, group)
pub const ACTIONS: &[(Action, &str, Option<KeyboardShortcut>, &str)] = &[
    (Action::PlayPause, "Play / Pause", key(NONE, Key::Space), "Transport"),
    (Action::PlayFromSelection, "Play from the selection", key(SHIFT, Key::Space), "Transport"),
    (Action::Stop, "Stop and return", key(NONE, Key::Enter), "Transport"),
    (Action::SoloTrack, "Solo the selection's track", key(SHIFT, Key::S), "Transport"),
    (Action::ToggleLoop, "Toggle loop", key(CMD, Key::L), "Transport"),
    (Action::LoopSelection, "Loop selected clips", key(SHIFT, Key::L), "Transport"),
    (Action::ToggleMetronome, "Toggle metronome", key(NONE, Key::K), "Transport"),
    (Action::Undo, "Undo", key(CMD, Key::Z), "Edit"),
    (Action::Redo, "Redo", key(CMD_SHIFT, Key::Z), "Edit"),
    (Action::Split, "Split at playhead", key(CMD, Key::E), "Edit"),
    (Action::Delete, "Delete selection", key(NONE, Key::Backspace), "Edit"),
    (Action::Deselect, "Deselect", key(NONE, Key::Escape), "Edit"),
    (Action::Duplicate, "Duplicate selection", key(CMD, Key::D), "Edit"),
    (Action::Copy, "Copy clips", key(CMD, Key::C), "Edit"),
    (Action::Paste, "Paste clips at playhead", key(CMD, Key::V), "Edit"),
    (Action::SelectAll, "Select all clips", key(CMD, Key::A), "Edit"),
    (Action::NudgeLeft, "Nudge left", key(NONE, Key::ArrowLeft), "Edit"),
    (Action::NudgeRight, "Nudge right", key(NONE, Key::ArrowRight), "Edit"),
    (Action::NudgeBarLeft, "Nudge left a bar", key(SHIFT, Key::ArrowLeft), "Edit"),
    (Action::NudgeBarRight, "Nudge right a bar", key(SHIFT, Key::ArrowRight), "Edit"),
    (Action::FineNudgeLeft, "Nudge left 5 ms", key(ALT, Key::ArrowLeft), "Edit"),
    (Action::FineNudgeRight, "Nudge right 5 ms", key(ALT, Key::ArrowRight), "Edit"),
    (Action::Reverse, "Reverse clip", key(NONE, Key::R), "Clip"),
    (Action::MuteClip, "Mute clip", key(NONE, Key::M), "Clip"),
    (Action::PitchUp, "Pitch up a semitone", key(NONE, Key::ArrowUp), "Clip"),
    (Action::PitchDown, "Pitch down a semitone", key(NONE, Key::ArrowDown), "Clip"),
    (Action::Faster, "Speed up 10%", key(NONE, Key::OpenBracket), "Clip"),
    (Action::Slower, "Slow down 10%", key(NONE, Key::CloseBracket), "Clip"),
    (Action::RestoreOriginal, "Restore the clip to how it was first placed", key(SHIFT, Key::R), "Clip"),
    (Action::RevertWord, "Revert to the saved word", None, "Clip"),
    (Action::Varispeed, "Toggle varispeed (tape) mode", key(NONE, Key::V), "Clip"),
    (Action::FitToTempo, "Fit clip to project tempo", None, "Clip"),
    (Action::Quantize, "Quantize to grid", key(NONE, Key::Q), "Sentence mixing"),
    (Action::CloseGaps, "Close gaps between clips", key(NONE, Key::G), "Sentence mixing"),
    (Action::SliceToWords, "Slice clip to words", key(NONE, Key::W), "Sentence mixing"),
    (Action::SnapPitchToKey, "Snap pitch to key", key(NONE, Key::P), "Sentence mixing"),
    (Action::PlaceRange, "Place editor selection", key(NONE, Key::E), "Sentence mixing"),
    (Action::FocusSay, "Say something", key(CMD, Key::F), "Sentence mixing"),
    (Action::NextTake, "Next take", key(NONE, Key::T), "Sentence mixing"),
    (Action::PrevTake, "Previous take", key(SHIFT, Key::T), "Sentence mixing"),
    (Action::Repeat, "Repeat, to stutter", key(SHIFT, Key::D), "Sentence mixing"),
    (Action::ChangeWords, "Change the selected words", key(NONE, Key::C), "Sentence mixing"),
    (Action::CompareChange, "Compare a change with the original", key(NONE, Key::A), "Sentence mixing"),
    (Action::HearOriginal, "Hear the whole original song", key(SHIFT, Key::A), "Sentence mixing"),
    (Action::TrySounds, "Try other sounds and takes in place", key(NONE, Key::F), "Sentence mixing"),
    (Action::Crossfade, "Crossfade clips together", key(NONE, Key::X), "Sentence mixing"),
    (Action::Group, "Group clips", key(CMD, Key::G), "Sentence mixing"),
    (Action::Ungroup, "Ungroup clips", key(CMD_SHIFT, Key::G), "Sentence mixing"),
    (Action::SaveWord, "Save as a word to use again", key(SHIFT, Key::W), "Sentence mixing"),
    (Action::Transcribe, "Transcribe track", key(CMD_SHIFT, Key::T), "Sentence mixing"),
    (Action::ZoomIn, "Zoom in", key(NONE, Key::Equals), "View"),
    (Action::ZoomOut, "Zoom out", key(NONE, Key::Minus), "View"),
    (Action::ZoomFit, "Zoom to fit project", key(NONE, Key::Num0), "View"),
    (Action::ZoomSelection, "Zoom to selection", key(NONE, Key::Z), "View"),
    (Action::ToggleSnap, "Toggle snapping", key(NONE, Key::S), "View"),
    (Action::ToggleEditor, "Show / hide bottom panel", key(NONE, Key::Tab), "View"),
    (Action::ShowEditor, "Show detail editor", key(NONE, Key::Num1), "View"),
    (Action::ShowDevices, "Show devices", key(NONE, Key::Num2), "View"),
    (Action::ShowMixer, "Show mixer", key(NONE, Key::Num3), "View"),
    (Action::ToggleInspector, "Show / hide inspector", key(NONE, Key::I), "View"),
    (Action::ToggleVideo, "Show / hide the picture", key(SHIFT, Key::V), "View"),
    (Action::ToggleSay, "Show / hide the Say panel", key(CMD, Key::B), "View"),
    (Action::ShowPicture, "Show picture effects", None, "View"),
    (Action::LargePicture, "Make the picture large or small", None, "View"),
    (Action::Palette, "Command palette", key(CMD, Key::K), "View"),
    (Action::Shortcuts, "Keyboard shortcuts", key(NONE, Key::Questionmark), "View"),
    (Action::New, "New project", key(CMD, Key::N), "File"),
    (Action::Open, "Open project", key(CMD, Key::O), "File"),
    (Action::AudioSettings, "Audio settings", key(CMD, Key::Comma), "File"),
    (Action::Save, "Save", key(CMD, Key::S), "File"),
    (Action::SaveAs, "Save as", key(CMD_SHIFT, Key::S), "File"),
    (Action::Import, "Import audio files", key(CMD, Key::I), "File"),
    (Action::Export, "Render to MP3, WAV or MP4", key(CMD_SHIFT, Key::R), "File"),
    (Action::AddTrack, "Add track", key(CMD, Key::T), "Track"),
    (Action::AddVocalTrack, "Add Sentence track", None, "Track"),
    (Action::SplitStems, "Split into stems", None, "Track"),
    (Action::RenameTrack, "Rename track", key(CMD, Key::R), "Track"),
    (Action::DeleteTrack, "Delete track", key(CMD, Key::Backspace), "Track"),
    (Action::AddEq, "Add EQ", None, "Device"),
    (Action::AddCompressor, "Add Compressor", None, "Device"),
    (Action::AddGate, "Add Gate", None, "Device"),
    (Action::AddSaturator, "Add Saturator", None, "Device"),
    (Action::AddDelay, "Add Delay", None, "Device"),
    (Action::AddReverb, "Add Reverb", None, "Device"),
    (Action::AddUtility, "Add Utility", None, "Device"),
    (Action::AddLimiter, "Add Limiter", None, "Device"),
];

pub fn shortcut_text(shortcut: &KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let mut s = String::new();
    if m.shift {
        s.push('⇧');
    }
    if m.alt {
        s.push('⌥');
    }
    if m.command {
        s.push('⌘');
    }
    let k = match shortcut.logical_key {
        Key::Space => "Space",
        Key::Enter => "Return",
        Key::Backspace => "Delete",
        Key::Tab => "Tab",
        Key::Escape => "Esc",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Equals => "=",
        Key::Minus => "-",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        other => other.name(),
    };
    s.push_str(k);
    crate::ds::keys(&s)
}

impl App {
    /// Handles keyboard shortcuts. Plain-key shortcuts are skipped while text is being typed.
    pub fn handle_shortcuts(&mut self, ctx: &Context) {
        let typing = ctx.egui_wants_keyboard_input();
        // Most specific first, so that ⇧⌘Z isn't taken by ⌘Z.
        let mut ordered: Vec<_> = ACTIONS.iter().filter_map(|(a, _, s, _)| s.map(|s| (*a, s))).collect();
        ordered.sort_by_key(|(_, s)| {
            let m = s.modifiers;
            std::cmp::Reverse(m.shift as u8 + m.command as u8 + m.alt as u8)
        });
        // Slash goes to the input from anywhere. It's here since it's a second key for FocusSay.
        if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Slash)) {
            self.run(Action::FocusSay);
        }
        // The forward delete key does the same as the main one.
        if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
            self.run(Action::Delete);
        }
        // The windowing layer turns these two into clipboard events and sends no key press for them.
        // A text field that's being typed in takes them itself.
        if !typing {
            let (copy, paste) = ctx.input(|i| {
                (i.events.iter().any(|e| matches!(e, Event::Copy)), i.events.iter().any(|e| matches!(e, Event::Paste(_))))
            });
            if copy {
                self.run(Action::Copy);
            }
            if paste {
                self.run(Action::Paste);
            }
        }
        // While typing, these belong to the text field, however much they'd also mean to the project.
        const TEXT_EDITING: [Action; 4] = [Action::Undo, Action::Redo, Action::SelectAll, Action::DeleteTrack];
        for (action, shortcut) in ordered {
            if typing && (!shortcut.modifiers.command || TEXT_EDITING.contains(&action)) {
                continue;
            }
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.run(action);
                break;
            }
        }
    }

    fn selected_clips(&self) -> Vec<Clip> {
        self.selection.iter().filter_map(|c| self.project.clip(*c).cloned()).collect()
    }

    fn edit_selected(&mut self, label: &str, f: impl Fn(&mut Clip)) {
        let ids = self.selection.clone();
        self.edit(label, |p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    f(c);
                }
            }
        });
    }

    fn scale_selected_stretch(&mut self, label: &str, factor: f64) {
        let ids = self.selection.clone();
        self.edit(label, |p| {
            for id in ids {
                if let Some(stretch) = p.clip(id).map(|c| c.stretch) {
                    p.set_stretch(id, stretch * factor);
                }
            }
        });
    }

    pub fn grid_or_small_step(&self) -> f64 {
        self.project.beat_seconds() * self.grid_division.max(1.0 / 16.0)
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::PlayPause => self.toggle_play(),
            Action::Stop => self.stop(),
            Action::ToggleLoop => self.edit("Toggle loop", |p| p.loop_region.enabled = !p.loop_region.enabled),
            Action::LoopSelection => self.loop_selection(),
            Action::ToggleMetronome => self.edit("Toggle metronome", |p| p.metronome = !p.metronome),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::New => self.after_discard(AfterDiscard::New),
            Action::Open => self.after_discard(AfterDiscard::Open),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Import => self.import_dialog(),
            Action::Export => self.open_export_dialog(),
            Action::Split => self.split_at_playhead(),
            Action::Delete => {
                if self.delete_selected_key() {
                } else if !self.selection.is_empty() {
                    self.delete_selection();
                } else if self.track_focus {
                    self.run(Action::DeleteTrack);
                }
            }
            Action::Duplicate => self.duplicate_selection(),
            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste(),
            Action::SelectAll => {
                self.selection = self.project.tracks.iter().flat_map(|t| &t.clips).map(|c| c.id).collect();
            }
            Action::Reverse => self.edit_selected("Reverse", |c| c.reversed = !c.reversed),
            Action::MuteClip => self.edit_selected("Mute clip", |c| c.muted = !c.muted),
            Action::PitchUp => self.edit_selected("Pitch up", |c| c.pitch = (c.pitch + 1.0).min(48.0)),
            Action::PitchDown => self.edit_selected("Pitch down", |c| c.pitch = (c.pitch - 1.0).max(-48.0)),
            Action::Faster => self.scale_selected_stretch("Speed up", 1.0 / 1.1),
            Action::Slower => self.scale_selected_stretch("Slow down", 1.1),
            Action::TrySounds => self.open_trial(),
            Action::ChangeWords => self.open_rewrite(),
            Action::CompareChange => self.compare_change(),
            Action::HearOriginal => {
                // A listening switch rather than an edit, so it isn't an undo step.
                self.project.bypass_patches = !self.project.bypass_patches;
                self.needs_compile = true;
                let text = if self.project.bypass_patches { "Playing the original song" } else { "Playing your changes" };
                self.toast(ToastKind::Info, text);
            }
            Action::RestoreOriginal => {
                let ids = self.selection.clone();
                self.edit("Restore original", |p| ids.iter().for_each(|i| p.restore_original(*i)));
            }
            Action::RevertWord => self.revert_selected_word(),
            Action::Group => {
                let ids = self.selection.clone();
                if ids.len() < 2 {
                    self.toast(ToastKind::Info, "Select two or more clips to group");
                } else {
                    self.edit("Group", |p| {
                        p.group_clips(&ids, None);
                    });
                }
            }
            Action::Ungroup => {
                let ids = self.selection.clone();
                self.edit("Ungroup", |p| p.ungroup_clips(&ids));
            }
            Action::Crossfade => self.crossfade_selection(),
            Action::SaveWord => self.open_save_word(),
            Action::FineNudgeLeft => self.nudge_by(-0.005),
            Action::FineNudgeRight => self.nudge_by(0.005),
            Action::ToggleVideo => self.show_video = !self.show_video,
            Action::Varispeed => self.edit_selected("Change stretch mode", |c| {
                c.mode = if c.mode == ClipMode::Elastic { ClipMode::Varispeed } else { ClipMode::Elastic };
            }),
            Action::FitToTempo => self.fit_selection_to_tempo(),
            Action::Quantize => {
                let (ids, grid) = (self.selection.clone(), self.grid_division);
                self.edit("Quantize", |p| p.quantize(&ids, grid));
            }
            Action::CloseGaps => {
                let ids = self.selection.clone();
                self.edit("Close gaps", |p| p.close_gaps(&ids, 0.0));
            }
            Action::SliceToWords => self.slice_selection_to_words(),
            Action::SnapPitchToKey => self.snap_selection_pitch(),
            Action::NudgeLeft => self.nudge(-1.0),
            Action::NudgeRight => self.nudge(1.0),
            Action::NudgeBarLeft => self.nudge_by(-self.bar_seconds()),
            Action::NudgeBarRight => self.nudge_by(self.bar_seconds()),
            Action::ShowPicture => {
                self.picture.mode = crate::panels::picture::ChainMode::Picture;
                self.show_tab(BottomTab::Devices);
            }
            Action::LargePicture => self.picture.large = !self.picture.large,
            Action::AudioSettings => self.open_audio_settings(),
            Action::SplitStems => self.split_selected(),
            Action::RenameTrack => {
                if let Some(track) = self.selected_track.and_then(|t| self.project.track(t)) {
                    self.rename_track = Some((track.id, track.name.clone(), false));
                }
            }
            Action::ZoomIn => self.zoom_focused(1.4),
            Action::ZoomOut => self.zoom_focused(1.0 / 1.4),
            Action::ZoomFit => {
                let end = self.project.duration().max(10.0);
                self.arr_view.fit(0.0, end);
            }
            Action::ZoomSelection => self.zoom_to_selection(),
            Action::ToggleSnap => self.snap = !self.snap,
            Action::ToggleEditor => self.show_bottom = !self.show_bottom,
            Action::ShowEditor => self.show_tab(BottomTab::Editor),
            Action::ShowDevices => self.show_tab(BottomTab::Devices),
            Action::ShowMixer => self.show_tab(BottomTab::Mixer),
            Action::ToggleInspector => self.show_inspector = !self.show_inspector,
            Action::ToggleSay => self.show_say = !self.show_say,
            Action::FocusSay => {
                self.show_say = true;
                self.say.tab = crate::panels::say::SayTab::Say;
                self.say.focus_input = true;
            }
            Action::NextTake => self.step_selected_take(1),
            Action::PrevTake => self.step_selected_take(-1),
            Action::Repeat => self.duplicate_selection(),
            Action::SoloTrack => self.solo_selection_track(),
            Action::PlayFromSelection => self.play_from_selection(),
            Action::Deselect => {
                self.selection.clear();
                self.editor.range = None;
            }
            Action::Palette => {
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
                self.palette_index = 0;
            }
            Action::Shortcuts => self.show_shortcuts = !self.show_shortcuts,
            Action::AddTrack => {
                let track = self.edit("Add track", |p| {
                    let n = p.tracks.len() + 1;
                    p.add_track(format!("Track {n}"), TrackKind::Music)
                });
                self.selected_track = Some(track);
            }
            Action::AddVocalTrack => {
                let track = self.edit("Add Sentence track", App::sentence_track);
                self.selected_track = Some(track);
            }
            Action::DeleteTrack => {
                let name = self.selected_track.and_then(|t| self.project.track(t)).map(|t| t.name.clone());
                if let (Some(track), Some(name)) = (self.selected_track.take(), name) {
                    self.edit("Delete track", |p| p.remove_track(track));
                    self.selection.retain(|c| self.project.clip(*c).is_some());
                    self.track_focus = false;
                    self.toast_with(ToastKind::Info, format!("Deleted {name}"), crate::app::ToastAction::Undo);
                }
            }
            Action::Transcribe => self.transcribe_selected(),
            Action::PlaceRange => self.place_editor_range(),
            Action::AddEq => self.add_device(DeviceKind::Eq),
            Action::AddCompressor => self.add_device(DeviceKind::Compressor),
            Action::AddGate => self.add_device(DeviceKind::Gate),
            Action::AddSaturator => self.add_device(DeviceKind::Saturator),
            Action::AddDelay => self.add_device(DeviceKind::Delay),
            Action::AddReverb => self.add_device(DeviceKind::Reverb),
            Action::AddUtility => self.add_device(DeviceKind::Utility),
            Action::AddLimiter => self.add_device(DeviceKind::Limiter),
        }
    }

    fn bar_seconds(&self) -> f64 {
        self.project.beat_seconds() * self.project.beats_per_bar as f64
    }

    fn show_tab(&mut self, tab: BottomTab) {
        self.show_bottom = true;
        self.bottom_tab = tab;
    }

    /// Swaps selected clips to another take of the same word. They stay where they are, so this is
    /// how you try takes once a line is on the timeline.
    fn step_selected_take(&mut self, delta: i32) {
        let mut swaps = Vec::new();
        for id in self.selection.clone() {
            let Some(clip) = self.project.clip(id).cloned() else { continue };
            let takes = self.takes_of_clip(&clip);
            if takes.len() < 2 {
                continue;
            }
            let next = (self.current_take(&clip, &takes) as i32 + delta).rem_euclid(takes.len() as i32) as usize;
            swaps.push((id, takes[next].clone()));
        }
        if swaps.is_empty() {
            self.toast(ToastKind::Info, "This has no other takes");
            return;
        }
        let audition = swaps[0].1.clone();
        self.edit("Change take", |p| {
            for (id, hit) in &swaps {
                if let Some((a, b)) = App::padded_range(p, hit) {
                    p.swap_clip_audio(*id, mp_model::SourceId(hit.source), a, b);
                }
            }
        });
        self.flash_clips(&swaps.iter().map(|s| s.0).collect::<Vec<_>>());
        self.audition_hit(&audition);
    }

    fn solo_selection_track(&mut self) {
        let track = self.selection.first().and_then(|c| self.project.track_of_clip(*c)).or(self.selected_track);
        let Some(track) = track else {
            self.toast(ToastKind::Info, "Select a clip or track first");
            return;
        };
        self.edit("Solo track", |p| {
            if let Some(t) = p.track_mut(track) {
                t.solo = !t.solo;
            }
        });
    }

    fn play_from_selection(&mut self) {
        if let Some((a, _)) = self.selection_span() {
            // A change is heard coming in from the song around it.
            let in_change = self.selection.first().and_then(|c| self.project.clip(*c)).is_some_and(|c| c.patch.is_some());
            let lead = if in_change { crate::panels::changes::PRE_ROLL } else { 0.0 };
            self.seek((a - lead).max(0.0));
        }
        self.play_start = self.playhead;
        self.engine.audition(None);
        self.engine.play();
    }

    pub fn add_device(&mut self, kind: DeviceKind) {
        let target = if self.master_selected { None } else { self.selected_track };
        if target.is_none() && !self.master_selected {
            self.toast(ToastKind::Info, "Select a track first");
            return;
        }
        self.edit(&format!("Add {}", kind.name()), |p| p.add_device(target, kind.clone()));
        self.picture.mode = crate::panels::picture::ChainMode::Sound;
        self.show_tab(BottomTab::Devices);
    }

    fn zoom_focused(&mut self, factor: f64) {
        if self.show_bottom && self.bottom_tab == BottomTab::Editor && self.editor.hovered {
            let view = &mut self.editor.view;
            let center = view.left + view.width * 0.5;
            view.zoom_at(factor, center);
        } else {
            let px = self.arr_view.x(self.playhead).clamp(self.arr_view.left, self.arr_view.left + self.arr_view.width);
            self.arr_view.zoom_at(factor, px);
        }
    }

    fn selection_span(&self) -> Option<(f64, f64)> {
        let clips = self.selected_clips();
        let a = clips.iter().map(|c| c.start).fold(f64::MAX, f64::min);
        let b = clips.iter().map(Clip::end).fold(f64::MIN, f64::max);
        (!clips.is_empty()).then_some((a, b))
    }

    fn zoom_to_selection(&mut self) {
        if let Some((a, b)) = self.selection_span() {
            self.arr_view.fit(a, b);
            self.editor.pending_fit = Some((a, b));
        }
    }

    fn loop_selection(&mut self) {
        if let Some((a, b)) = self.selection_span() {
            self.edit("Loop selection", |p| {
                p.loop_region.start = a;
                p.loop_region.end = b;
                p.loop_region.enabled = true;
            });
        }
    }

    /// Splits selected clips at the playhead, or every clip under it if nothing is selected.
    fn split_at_playhead(&mut self) {
        let at = self.playhead;
        let targets: Vec<ClipId> = if self.selection.is_empty() {
            let wanted = self.selected_track;
            self.project
                .tracks
                .iter()
                .filter(|t| wanted.is_none() || Some(t.id) == wanted)
                .flat_map(|t| &t.clips)
                .map(|c| c.id)
                .collect()
        } else {
            self.selection.clone()
        };
        self.edit("Split", |p| {
            for id in targets {
                p.split_clip(id, at);
            }
        });
    }

    fn delete_selection(&mut self) {
        let ids = std::mem::take(&mut self.selection);
        self.edit("Delete", |p| {
            for id in ids {
                p.remove_clip(id);
            }
        });
    }

    /// Duplicates selected clips directly after the selection's end.
    pub fn duplicate_selection(&mut self) {
        let Some((a, b)) = self.selection_span() else { return };
        let ids = self.selection.clone();
        let copies = self.edit("Duplicate", |p| p.duplicate_clips(&ids, b - a));
        self.selection = copies;
    }

    fn copy_selection(&mut self) {
        self.clipboard = self
            .selection
            .iter()
            .filter_map(|id| Some((self.project.clip(*id)?.clone(), self.project.track_of_clip(*id)?)))
            .collect();
        let n = self.clipboard.len();
        if n > 0 {
            // A clipboard with no text produces no paste event, so this also has to put text there.
            let names: Vec<String> = self.clipboard.iter().map(|(c, _)| self.clip_label(c)).collect();
            self.ctx.copy_text(names.join(" "));
            self.toast(ToastKind::Info, format!("Copied {n} clip{}", if n == 1 { "" } else { "s" }));
        }
    }

    fn paste(&mut self) {
        let first = self.clipboard.iter().map(|(c, _)| c.start).fold(f64::MAX, f64::min);
        if self.clipboard.is_empty() {
            return;
        }
        let at = self.playhead;
        let items = self.clipboard.clone();
        let (pasted, end) = self.edit("Paste", |p| {
            let mut out = Vec::new();
            let mut end = at;
            let mut groups = std::collections::HashMap::new();
            for (clip, track) in &items {
                let id = mp_model::ClipId(p.alloc_id());
                let mut copy = clip.clone();
                copy.id = id;
                copy.start = at + clip.start - first;
                if let Some(group) = clip.group {
                    copy.group = Some(*groups.entry(group).or_insert_with(|| mp_model::GroupId(p.alloc_id())));
                }
                end = end.max(copy.end());
                let target = if p.track(*track).is_some() { Some(*track) } else { p.tracks.first().map(|t| t.id) };
                if let Some(t) = target.and_then(|t| p.track_mut(t)) {
                    t.clips.push(copy);
                    out.push(id);
                }
            }
            (out, end)
        });
        self.selection = pasted;
        self.seek(end);
    }

    /// Moves clips by a grid step, or by a millisecond-scale step with snapping off.
    fn nudge(&mut self, direction: f64) {
        let step = if self.snap { self.grid_or_small_step() } else { 0.005 };
        self.nudge_by(direction * step);
    }

    /// Flips the selected change between the original and its version, which takes effect mid-playback.
    fn compare_change(&mut self) {
        let from_clip = self.selection.first().and_then(|c| self.project.clip(*c)).and_then(|c| c.patch);
        let Some(id) = from_clip.or(self.selected_patch) else {
            self.toast(ToastKind::Info, "Select a change first");
            return;
        };
        self.selected_patch = Some(id);
        self.edit("Compare", |p| p.toggle_patch(id));
        self.selection.retain(|c| self.project.clip(*c).is_some());
    }

    fn nudge_by(&mut self, step: f64) {
        if self.selection.is_empty() {
            self.seek(self.playhead + step);
            return;
        }
        let ids = self.selection.clone();
        self.edit("Nudge", |p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    c.start = (c.start + step).max(0.0);
                }
            }
            p.update_auto_crossfades();
        });
    }

    /// Blends selected clips into each other. Dragging them afterwards changes how long the blend is.
    fn crossfade_selection(&mut self) {
        const OVERLAP: f64 = 0.02;
        if self.selection.len() < 2 {
            self.toast(ToastKind::Info, "Select two or more clips to crossfade");
            return;
        }
        let ids = self.selection.clone();
        self.edit("Crossfade", |p| p.crossfade_clips(&ids, OVERLAP));
    }

    /// Opens the dialog that names the selection as a word, suggesting a name from what it spells.
    fn open_save_word(&mut self) {
        if self.selection.is_empty() {
            self.toast(ToastKind::Info, "Select the clips that make up the word first");
            return;
        }
        let clips = self.selection.clone();
        let first = clips.first().and_then(|c| self.project.clip(*c));
        let name = first.and_then(|c| c.word_name.clone()).unwrap_or_else(|| {
            let labels: Vec<String> = clips.iter().filter_map(|c| self.project.clip(*c)).filter_map(|c| c.label.clone()).collect();
            labels.join(" ")
        });
        self.word_dialog = Some(crate::app::WordDialog { name, clips, renaming: None, focused: false });
    }

    fn revert_selected_word(&mut self) {
        let Some(first) = self.selection.first().copied() else { return };
        if self.project.clip(first).is_none_or(|c| c.made_word.is_none()) {
            self.toast(ToastKind::Info, "This isn't a saved word");
            return;
        }
        let placed = self.edit("Revert to saved", |p| p.revert_to_saved(first));
        self.flash_clips(&placed);
        self.selection = placed;
    }

    fn fit_selection_to_tempo(&mut self) {
        let ids = self.selection.clone();
        let unknown = ids.iter().any(|id| {
            self.project.clip(*id).and_then(|c| self.project.source(c.source)).and_then(|s| s.bpm).is_none()
        });
        self.edit("Fit to tempo", |p| ids.iter().for_each(|id| p.fit_clip_to_tempo(*id)));
        if unknown {
            self.toast(ToastKind::Info, "Some clips have no detected tempo");
        }
    }

    pub fn slice_selection_to_words(&mut self) {
        let targets: Vec<ClipId> = if self.selection.is_empty() {
            self.selected_track
                .and_then(|t| self.project.track(t))
                .map(|t| t.clips.iter().map(|c| c.id).collect())
                .unwrap_or_default()
        } else {
            self.selection.clone()
        };
        let mut plans = Vec::new();
        for id in targets {
            let Some(clip) = self.project.clip(id) else { continue };
            let Some(transcript) = self.index.transcript(clip.source.0) else { continue };
            let ranges: Vec<(f64, f64, String)> = transcript
                .words
                .iter()
                .filter(|w| w.end > clip.src_in && w.start < clip.src_out)
                .map(|w| (w.start, w.end, w.text.clone()))
                .collect();
            if !ranges.is_empty() {
                plans.push((id, ranges));
            }
        }
        if plans.is_empty() {
            self.toast(ToastKind::Info, "Select a transcribed clip first");
            return;
        }
        let parts = self.edit("Slice to words", |p| {
            plans.iter().flat_map(|(id, r)| p.slice_clip_to_ranges(*id, r)).collect::<Vec<_>>()
        });
        self.toast(ToastKind::Good, format!("Sliced into {} words", parts.len()));
        self.selection = parts;
    }

    /// Moves each selected clip to the nearest note of the project's key, or the nearest semitone
    /// when no key is set.
    pub fn snap_selection_pitch(&mut self) {
        let key = self.project.key;
        let mut shifts = Vec::new();
        let mut unpitched = 0;
        for clip in self.selected_clips() {
            match self.clip_pitch(clip.source, clip.src_in, clip.src_out) {
                Some(hz) => {
                    let original = mp_dsp::hz_to_midi(hz);
                    let current = original + clip.pitch;
                    let delta = key.map_or(current.round() - current, |k| k.snap_semitones(current));
                    shifts.push((clip.id, clip.pitch + delta));
                }
                None => unpitched += 1,
            }
        }
        let tuned = shifts.len();
        self.edit("Snap pitch", |p| {
            for (id, pitch) in shifts {
                if let Some(c) = p.clip_mut(id) {
                    c.mode = ClipMode::Elastic;
                    c.pitch = pitch;
                }
            }
        });
        let target = key.map_or("the nearest note".to_string(), |k| k.label());
        let note = if unpitched > 0 { format!(" ({unpitched} had no clear pitch)") } else { String::new() };
        self.toast(ToastKind::Good, format!("Tuned {tuned} clip{} to {target}{note}", if tuned == 1 { "" } else { "s" }));
    }

    fn transcribe_selected(&mut self) {
        let source = self
            .selection
            .first()
            .and_then(|c| self.project.clip(*c))
            .map(|c| c.source)
            .or_else(|| {
                let track = self.selected_track.and_then(|t| self.project.track(t))?;
                track.clips.first().map(|c| c.source)
            });
        match source {
            Some(source) => self.request_transcribe(source, true),
            None => self.toast(ToastKind::Info, "Select a clip or track first"),
        }
    }

    fn open_export_dialog(&mut self) {
        self.export_dialog = Some(crate::panels::render::ExportDialog::new(&self.project));
    }

    /// Runs an action by its debug name, for the test script.
    pub fn run_by_name(&mut self, name: &str) -> bool {
        match ACTIONS.iter().find(|(a, ..)| format!("{a:?}") == name) {
            Some((a, ..)) => {
                self.run(*a);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let mut seen = std::collections::HashSet::new();
        for (action, _, shortcut, _) in ACTIONS {
            if let Some(s) = shortcut {
                let key = (s.logical_key, s.modifiers.shift, s.modifiers.command, s.modifiers.alt);
                assert!(seen.insert(key), "{action:?} reuses a shortcut");
            }
        }
    }

    #[test]
    fn command_shortcuts_fire_on_mac_and_elsewhere() {
        let mac = Modifiers { mac_cmd: true, command: true, shift: true, ..Modifiers::NONE };
        let other = Modifiers { ctrl: true, command: true, shift: true, ..Modifiers::NONE };
        assert!(mac.matches_logically(CMD_SHIFT));
        assert!(other.matches_logically(CMD_SHIFT));
    }
}
