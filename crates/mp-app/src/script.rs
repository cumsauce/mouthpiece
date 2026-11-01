//! Drives the real running app from a script, for end-to-end testing. Set MOUTHPIECE_SCRIPT to a
//! file with one step per line. It uses real input events, so it exercises what a user would.
//!
//! Steps: wait <s> | idle | action <Name> | search <text> | enter | sentence <text> | place_sentence
//! | select_track <i> | select_clip <track> <clip> | open_track <i> | open_clip <track> <clip>
//! | bottom <editor/devices/mixer> | browser <words/sentence/files> | seek <s> | range <a> <b>
//! | import <path> | drop <path> | download <model id> | mute_all <tracks to keep, comma separated> | transcript <part of source name> <word> <word>... | export <path> | save <path>
//! | open <path> | move <x> <y> | click/dclick/rclick/cmdclick <x> <y>
//! | drag/altdrag/shiftdrag/cmddrag <x1> <y1> <x2> <y2> | key <name> | type <text>
//! | screenshot <path> | dump <path> | quit

use crate::app::{App, BottomTab};
use crate::browser::BrowserTab;
use crate::export_ui::RangeChoice;
use eframe::egui::{self, Event, Modifiers, PointerButton};
use mp_analysis::transcribe::{finish_transcript, RawSegment, RawWord};
use mp_export::Format;
use mp_model::SourceId;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Script {
    steps: VecDeque<String>,
    wait_until: Option<Instant>,
    waiting_for_idle: bool,
    screenshot_pending: Arc<AtomicBool>,
    settle_frames: u32,
}

impl Script {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("MOUTHPIECE_SCRIPT")?;
        let text = std::fs::read_to_string(path).ok()?;
        let steps = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect();
        Some(Self { steps, wait_until: None, waiting_for_idle: false, screenshot_pending: Arc::new(AtomicBool::new(false)), settle_frames: 0 })
    }
}

/// A file "dropped" by a script, which enters through the same input path as a real drop.
#[derive(Debug)]
struct ScriptDrop(std::path::PathBuf);

impl egui::DroppedFile for ScriptDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

pub fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(mut script) = app.script.take() else { return };
    run(&mut script, app, ctx);
    app.script = Some(script);
}

fn pointer(button: PointerButton, pos: egui::Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers }
}

fn push(app: &mut App, events: Vec<Event>, modifiers: Modifiers) {
    app.inject_frames.push_back((events, modifiers));
}

fn run(script: &mut Script, app: &mut App, ctx: &egui::Context) {
    // A screenshot completes after the frame that requested it has been drawn.
    if script.screenshot_pending.load(Ordering::Acquire) {
        return;
    }
    if !app.inject_frames.is_empty() {
        script.settle_frames = 3;
        return;
    }
    if script.settle_frames > 0 {
        script.settle_frames -= 1;
        return;
    }
    if let Some(until) = script.wait_until {
        if Instant::now() < until {
            return;
        }
        script.wait_until = None;
    }
    if script.waiting_for_idle {
        if !app.jobs.active.is_empty() || app.cache.pending() > 0 {
            return;
        }
        script.waiting_for_idle = false;
    }
    let Some(step) = script.steps.pop_front() else { return };
    println!("script: {step}");
    let (command, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
    let numbers: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let usizes: Vec<usize> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    script.settle_frames = 2;
    match command {
        "wait" => script.wait_until = Some(Instant::now() + Duration::from_secs_f32(numbers[0])),
        "idle" => script.waiting_for_idle = true,
        "action" => {
            if !app.run_by_name(rest) {
                println!("script: UNKNOWN ACTION {rest}");
            }
        }
        "search" => {
            app.browser.tab = BrowserTab::Words;
            app.browser.query = rest.to_string();
            app.run_search();
        }
        "enter" => {
            if let Some(hit) = app.browser.results.get(app.browser.selected).cloned() {
                app.place_hit_at_playhead(&hit);
            }
        }
        "sentence" => {
            app.browser.tab = BrowserTab::Sentence;
            app.browser.sentence = rest.to_string();
            app.browser.sentence_dirty = true;
        }
        "place_sentence" => app.place_sentence(),
        "select_track" => app.selected_track = app.project.tracks.get(usizes[0]).map(|t| t.id),
        "select_clip" => {
            if let Some(c) = app.project.tracks.get(usizes[0]).and_then(|t| t.clips.get(usizes[1])) {
                app.selection = vec![c.id];
            }
        }
        "open_track" => {
            if let Some(id) = app.project.tracks.get(usizes[0]).map(|t| t.id) {
                app.open_track_in_editor(id);
            }
        }
        "open_clip" => {
            if let Some(id) = app.project.tracks.get(usizes[0]).and_then(|t| t.clips.get(usizes[1])).map(|c| c.id) {
                app.open_clip_in_editor(id);
            }
        }
        "bottom" => {
            app.show_bottom = true;
            app.bottom_tab = match rest {
                "devices" => BottomTab::Devices,
                "mixer" => BottomTab::Mixer,
                _ => BottomTab::Editor,
            };
        }
        "browser" => {
            app.browser.tab = match rest {
                "sentence" => BrowserTab::Sentence,
                "files" => BrowserTab::Files,
                _ => BrowserTab::Words,
            };
        }
        "seek" => app.seek(numbers[0] as f64),
        "range" => app.editor.range = Some((numbers[0] as f64, numbers[1] as f64)),
        "import" => app.import_path(rest.into()),
        "drop" => app.script_dropped.push(Arc::new(ScriptDrop(rest.into()))),
        "download" => {
            if let Some(model) = mp_analysis::transcribe::MODELS.iter().find(|m| m.id == rest) {
                if !model.is_installed() {
                    app.jobs.download_model(*model);
                }
            }
        }
        "mute_all" => {
            let names: Vec<&str> = rest.split(',').collect();
            app.edit("Mute", |p| p.tracks.iter_mut().for_each(|t| t.mute = !names.contains(&t.name.as_str())));
        }
        "transcript" => inject_transcript(app, rest),
        "export" => {
            let format = if rest.ends_with(".wav") { Format::Wav24 } else { Format::Mp3 { kbps: 320 } };
            app.start_export(rest.into(), format, RangeChoice::Project, 0.5, "Script".to_string());
        }
        "save" => app.save_to(std::path::Path::new(rest)),
        "open" => app.open_project_at(std::path::Path::new(rest)),
        "move" => push(app, vec![Event::PointerMoved(egui::pos2(numbers[0], numbers[1]))], Modifiers::NONE),
        "click" | "dclick" | "rclick" | "cmdclick" => {
            let pos = egui::pos2(numbers[0], numbers[1]);
            let button = if command == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
            let m = if command == "cmdclick" { Modifiers::COMMAND } else { Modifiers::NONE };
            push(app, vec![Event::PointerMoved(pos)], m);
            for _ in 0..if command == "dclick" { 2 } else { 1 } {
                push(app, vec![pointer(button, pos, true, m)], m);
                push(app, vec![pointer(button, pos, false, m)], m);
            }
        }
        "drag" | "altdrag" | "shiftdrag" | "cmddrag" => {
            let m = match command {
                "altdrag" => Modifiers::ALT,
                "shiftdrag" => Modifiers::SHIFT,
                "cmddrag" => Modifiers::COMMAND,
                _ => Modifiers::NONE,
            };
            let from = egui::pos2(numbers[0], numbers[1]);
            let to = egui::pos2(numbers[2], numbers[3]);
            push(app, vec![Event::PointerMoved(from)], m);
            push(app, vec![Event::PointerMoved(from)], m);
            push(app, vec![pointer(PointerButton::Primary, from, true, m)], m);
            for i in 1..=8 {
                push(app, vec![Event::PointerMoved(from + (to - from) * i as f32 / 8.0)], m);
            }
            push(app, vec![pointer(PointerButton::Primary, to, false, m)], m);
        }
        "key" => {
            if let Some(key) = egui::Key::from_name(rest) {
                let press = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
                push(app, vec![press(true)], Modifiers::NONE);
                push(app, vec![press(false)], Modifiers::NONE);
            }
        }
        "type" => push(app, vec![Event::Text(rest.to_string())], Modifiers::NONE),
        "screenshot" => {
            let (path, pending, repaint) = (rest.to_string(), script.screenshot_pending.clone(), ctx.clone());
            pending.store(true, Ordering::Release);
            ctx.request_screenshot(move |image| {
                save_png(&path, &image);
                println!("script: saved {path}");
                pending.store(false, Ordering::Release);
                repaint.request_repaint();
            });
        }
        "dump" => dump(app, rest),
        "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        _ => println!("script: UNKNOWN STEP {step}"),
    }
}

/// Spreads the given words evenly over a source, as a stand-in for Whisper in UI tests.
fn inject_transcript(app: &mut App, rest: &str) {
    let mut parts = rest.split_whitespace();
    let name = parts.next().unwrap_or_default();
    let words: Vec<&str> = parts.collect();
    let Some(source) = app.project.sources.iter().find(|s| s.name.contains(name)).map(|s| (s.id, s.duration)) else { return };
    let (id, duration): (SourceId, f64) = source;
    let Some(runtime) = app.runtime.get(&id) else { return };
    let step = duration / words.len().max(1) as f64;
    let raw: Vec<RawWord> = words
        .iter()
        .enumerate()
        .map(|(i, w)| RawWord { text: w.to_string(), anchor: (i as f64 + 0.5) * step, confidence: 0.5 + 0.5 * ((i % 3) as f32 / 2.0) })
        .collect();
    let segment = RawSegment { start: 0.0, end: duration, words: raw };
    let transcript = finish_transcript(vec![segment], &runtime.mono, app.sample_rate as f32, "script");
    app.index.set_transcript(id.0, transcript);
    app.browser.search_dirty = true;
}

fn dump(app: &App, path: &str) {
    let state = serde_json::json!({
        "playhead": app.playhead,
        "bpm": app.project.bpm,
        "grid_offset": app.project.grid_offset,
        "key": app.project.key.map(|k| k.label()),
        "selection": app.selection.len(),
        "selected_track": app.selected_track.and_then(|t| app.project.track(t)).map(|t| t.name.clone()),
        "words": app.index.word_count(),
        "results": app.browser.results.iter().map(|h| h.text.clone()).collect::<Vec<_>>(),
        "plan": app.browser.plan.iter().map(|p| (p.text.clone(), format!("{:?}", p.source), p.options.len())).collect::<Vec<_>>(),
        "transcripts": app.project.sources.iter().filter_map(|s| app.index.transcript(s.id.0)).map(|t| t.words.iter().map(|w| serde_json::json!({
            "text": w.text, "start": w.start, "end": w.end, "conf": w.confidence,
            "phonemes": w.phonemes.iter().map(|p| format!("{}:{:.3}", p.symbol, p.start)).collect::<Vec<_>>(),
            "pitch": w.pitch_hz })).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "undo": app.history.labels().collect::<Vec<_>>(),
        "toasts": app.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(),
        "bottom_tab": format!("{:?}", app.bottom_tab),
        "show_bottom": app.show_bottom,
        "editor_range": app.editor.range,
        "tracks": app.project.tracks.iter().map(|t| serde_json::json!({
            "name": t.name, "kind": t.kind.label(), "mute": t.mute, "gain": t.gain_db,
            "devices": t.devices.iter().map(|d| d.kind.name()).collect::<Vec<_>>(),
            "clips": t.clips.iter().map(|c| serde_json::json!({
                "start": c.start, "src_in": c.src_in, "src_out": c.src_out, "stretch": c.stretch,
                "pitch": c.pitch, "reversed": c.reversed, "label": c.label, "fade_in": c.fade_in,
                "fade_out": c.fade_out, "gain": c.gain_db })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let _ = std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap_or_default());
}

fn save_png(path: &str, image: &egui::ColorImage) {
    let Ok(file) = std::fs::File::create(path) else { return };
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        let _ = writer.write_image_data(&bytes);
    }
}
