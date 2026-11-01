//! Connects `EngineCore` to the sound card, and the lock-free bridge between UI and audio threads.

use crate::core::EngineCore;
use crate::graph::{EngineGraph, Voice};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Track meters available. Tracks beyond this play normally but show no meter.
pub const MAX_METERS: usize = 256;

enum Command {
    Graph(Box<EngineGraph>),
    Play,
    Stop,
    Seek(u64),
    Audition(Option<Box<Voice>>),
}

enum Garbage {
    Graph(#[allow(dead_code)] Box<EngineGraph>),
    Voice(#[allow(dead_code)] Box<Voice>),
}

/// State published by the audio thread, readable from anywhere.
pub struct Shared {
    pub position: AtomicU64,
    pub playing: AtomicBool,
    pub track_meters: Vec<[AtomicU32; 2]>,
    pub master_meter: [AtomicU32; 2],
    /// Fraction of the callback's time budget used, as f32 bits.
    pub load: AtomicU32,
    pub audition_played: AtomicU64,
    pub audition_total: AtomicU64,
    /// The size of the last callback, in frames: the block size really in use.
    pub block: AtomicU32,
    /// Set when the output has gone away, such as a USB interface being unplugged.
    pub device_lost: AtomicBool,
}

impl Shared {
    fn new() -> Self {
        Self {
            position: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            track_meters: (0..MAX_METERS).map(|_| [AtomicU32::new(0), AtomicU32::new(0)]).collect(),
            master_meter: [AtomicU32::new(0), AtomicU32::new(0)],
            load: AtomicU32::new(0),
            audition_played: AtomicU64::new(0),
            audition_total: AtomicU64::new(0),
            block: AtomicU32::new(0),
            device_lost: AtomicBool::new(false),
        }
    }

    pub fn track_peak(&self, index: usize) -> [f32; 2] {
        self.track_meters.get(index).map_or([0.0; 2], |m| {
            [f32::from_bits(m[0].load(Ordering::Relaxed)), f32::from_bits(m[1].load(Ordering::Relaxed))]
        })
    }

    pub fn master_peak(&self) -> [f32; 2] {
        [
            f32::from_bits(self.master_meter[0].load(Ordering::Relaxed)),
            f32::from_bits(self.master_meter[1].load(Ordering::Relaxed)),
        ]
    }

    pub fn cpu_load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }
}

/// Runs on the audio thread. Everything it touches is preallocated.
struct Callback {
    core: EngineCore,
    commands: rtrb::Consumer<Command>,
    garbage: rtrb::Producer<Garbage>,
    shared: Arc<Shared>,
    left: Vec<f32>,
    right: Vec<f32>,
    sample_rate: f32,
}

impl Callback {
    fn drain_commands(&mut self) {
        // Only act while the garbage queue has room for what the command retires, so nothing is
        // ever freed here. A deferred command simply runs on a later callback.
        while self.garbage.slots() > 0 {
            let Ok(command) = self.commands.pop() else { break };
            match command {
                Command::Graph(graph) => {
                    let old = self.core.install_graph(graph);
                    let _ = self.garbage.push(Garbage::Graph(old));
                }
                Command::Play => self.core.play(),
                Command::Stop => self.core.stop(),
                Command::Seek(p) => self.core.seek(p),
                Command::Audition(voice) => {
                    if let Some(old) = self.core.audition(voice) {
                        let _ = self.garbage.push(Garbage::Voice(old));
                    }
                }
            }
        }
    }

    fn run(&mut self, frames: usize) {
        self.shared.block.store(frames as u32, Ordering::Relaxed);
        self.drain_commands();
        let started = std::time::Instant::now();
        self.core.process(&mut self.left[..frames], &mut self.right[..frames]);
        if let Some((played, total)) = self.core.audition_progress() {
            self.shared.audition_played.store(played, Ordering::Relaxed);
            self.shared.audition_total.store(total, Ordering::Relaxed);
            if played >= total && self.garbage.slots() > 0 {
                if let Some(old) = self.core.audition(None) {
                    let _ = self.garbage.push(Garbage::Voice(old));
                }
                self.shared.audition_total.store(0, Ordering::Relaxed);
            }
        }
        self.publish();
        let budget = frames as f32 / self.sample_rate;
        self.shared.load.store((started.elapsed().as_secs_f32() / budget).to_bits(), Ordering::Relaxed);
    }

    fn publish(&self) {
        let s = &self.shared;
        s.position.store(self.core.position, Ordering::Relaxed);
        s.playing.store(self.core.playing, Ordering::Relaxed);
        for (meter, peak) in s.track_meters.iter().zip(&self.core.track_peaks).take(self.core.graph().tracks.len()) {
            meter[0].store(peak[0].to_bits(), Ordering::Relaxed);
            meter[1].store(peak[1].to_bits(), Ordering::Relaxed);
        }
        s.master_meter[0].store(self.core.master_peak[0].to_bits(), Ordering::Relaxed);
        s.master_meter[1].store(self.core.master_peak[1].to_bits(), Ordering::Relaxed);
    }
}

/// What the person has chosen. `None` means Automatic: follow the system.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub host: Option<String>,
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
    /// The output channel the left side goes to, counting from 0. The right side is the next one.
    pub first_channel: u16,
}

/// An output that could be chosen, and what it supports.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputInfo {
    pub name: String,
    pub sample_rates: Vec<u32>,
    /// Smallest and largest block size, if the driver says.
    pub block_range: Option<(u32, u32)>,
    pub channels: u16,
}

/// The driver models this build can use. Most systems have one.
pub fn list_hosts() -> Vec<String> {
    cpal::available_hosts().iter().map(|h| h.name().to_string()).collect()
}

fn host_named(name: Option<&str>) -> cpal::Host {
    name.and_then(|n| cpal::available_hosts().into_iter().find(|h| h.name() == n))
        .and_then(|id| cpal::host_from_id(id).ok())
        .unwrap_or_else(cpal::default_host)
}

fn output_info(device: &cpal::Device) -> OutputInfo {
    const COMMON_RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];
    let ranges: Vec<_> = device.supported_output_configs().map(|c| c.collect()).unwrap_or_default();
    let sample_rates = COMMON_RATES.iter().copied().filter(|r| ranges.iter().any(|c: &cpal::SupportedStreamConfigRange| c.min_sample_rate() <= *r && *r <= c.max_sample_rate())).collect();
    let block_range = ranges.iter().find_map(|c| match c.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    });
    let channels = ranges.iter().map(|c| c.channels()).max().unwrap_or(2);
    OutputInfo { name: device.to_string(), sample_rates, block_range, channels }
}

/// Every output on a driver. This can take a moment, so it isn't for calling every frame.
pub fn list_outputs(host: Option<&str>) -> Vec<OutputInfo> {
    host_named(host).output_devices().map(|devices| devices.map(|d| output_info(&d)).collect()).unwrap_or_default()
}

/// The name of the output the system is using.
pub fn system_output_name(host: Option<&str>) -> Option<String> {
    host_named(host).default_output_device().map(|d| d.to_string())
}

/// Finds a stream setup for what's been asked for, preferring float samples.
fn pick_stream(device: &cpal::Device, config: &AudioConfig) -> Option<cpal::SupportedStreamConfig> {
    let fallback = device.default_output_config().ok();
    let rate = config.sample_rate.or(fallback.as_ref().map(|c| c.sample_rate()))?;
    let needed = config.first_channel.saturating_add(2);
    let ranges: Vec<_> = device.supported_output_configs().ok()?.collect();
    let fits = |c: &&cpal::SupportedStreamConfigRange| c.channels() >= needed && c.min_sample_rate() <= rate && rate <= c.max_sample_rate();
    let wanted_format = fallback.as_ref().map(|c| c.sample_format());
    let best = ranges
        .iter()
        .filter(fits)
        .min_by_key(|c| (c.sample_format() != cpal::SampleFormat::F32 && Some(c.sample_format()) != wanted_format, c.channels()));
    match best {
        Some(range) => Some((*range).with_sample_rate(rate)),
        None => fallback,
    }
}

/// The UI thread's handle to the running audio engine.
pub struct AudioEngine {
    commands: rtrb::Producer<Command>,
    garbage: rtrb::Consumer<Garbage>,
    pub shared: Arc<Shared>,
    pub sample_rate: u32,
    pub device_name: String,
    /// What was asked for, which isn't necessarily what could be opened.
    pub config: AudioConfig,
    /// Why the output isn't what was asked for, in words for the person using it.
    pub problem: Option<String>,
    /// Keeps the output running. `None` means no device was available, and a timer thread paces
    /// the engine instead so the transport still works.
    stream: Option<cpal::Stream>,
    stop_timer: Arc<AtomicBool>,
}

impl AudioEngine {
    /// Opens the chosen output, falling back to the system's, and to a silent timer if there is none.
    /// `wake` is called from another thread when the device fails, so that an idle UI can respond.
    pub fn start(config: AudioConfig, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (commands, command_rx) = rtrb::RingBuffer::new(256);
        let (garbage_tx, garbage) = rtrb::RingBuffer::new(1024);
        let shared = Arc::new(Shared::new());
        let host = host_named(config.host.as_deref());
        let chosen = config.device.as_ref().and_then(|name| host.output_devices().ok()?.find(|d| d.to_string() == *name));
        let mut problem = match (&config.device, &chosen) {
            (Some(name), None) => Some(format!("{name} isn't connected, so this is using the system output.")),
            _ => None,
        };
        let device = chosen.or_else(|| host.default_output_device());
        let picked = device.as_ref().and_then(|d| pick_stream(d, &config));
        let sample_rate = picked.as_ref().map_or(48000, |c| c.sample_rate());
        // Far larger than any real callback. Anything longer is split in `write`.
        const MAX_CALLBACK: usize = 16384;
        let callback = Callback {
            core: EngineCore::new(sample_rate as f32),
            commands: command_rx,
            garbage: garbage_tx,
            shared: shared.clone(),
            left: vec![0.0; MAX_CALLBACK],
            right: vec![0.0; MAX_CALLBACK],
            sample_rate: sample_rate as f32,
        };
        let device_name = device.as_ref().map_or("No output device".to_string(), |d| d.to_string());
        let stop_timer = Arc::new(AtomicBool::new(false));
        // The stream takes ownership of the callback, so a throwaway stream checks the device first.
        // Without this, a device that won't open would leave the transport frozen.
        let block = config.block_size;
        let usable = device.zip(picked).filter(|(d, c)| device_opens(d, c, block));
        let stream = match usable {
            Some((device, picked)) => {
                let first = (config.first_channel as usize).min((picked.channels() as usize).saturating_sub(2));
                build_stream(&device, picked, block, first, callback, shared.clone(), wake)
            }
            None => {
                run_silent_timer(callback, stop_timer.clone());
                problem = Some(format!("{device_name} won't open, so nothing can be heard. Try another output or block size."));
                None
            }
        };
        Self { commands, garbage, shared, sample_rate, device_name, config, problem, stream, stop_timer }
    }

    /// Lets go of the device, so that the next engine can open it.
    pub fn close(&mut self) {
        self.stream = None;
        self.stop_timer.store(true, Ordering::Relaxed);
    }

    /// Frees anything the audio thread has retired. Call regularly from the UI thread.
    pub fn collect_garbage(&mut self) {
        while self.garbage.pop().is_ok() {}
    }

    /// Whether another graph can be sent right now.
    pub fn can_send(&self) -> bool {
        self.commands.slots() > 8
    }

    fn send(&mut self, command: Command) {
        // Only full if the audio thread has stalled. The state is resent on the next edit.
        let _ = self.commands.push(command);
    }

    pub fn set_graph(&mut self, graph: Box<EngineGraph>) {
        self.send(Command::Graph(graph));
    }

    pub fn play(&mut self) {
        self.send(Command::Play);
    }

    pub fn stop(&mut self) {
        self.send(Command::Stop);
    }

    pub fn seek(&mut self, frame: u64) {
        self.shared.position.store(frame, Ordering::Relaxed);
        self.send(Command::Seek(frame));
    }

    pub fn audition(&mut self, voice: Option<Box<Voice>>) {
        if voice.is_none() {
            self.shared.audition_total.store(0, Ordering::Relaxed);
        }
        self.send(Command::Audition(voice));
    }

    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
}

fn stream_config(picked: &cpal::SupportedStreamConfig, block: Option<u32>) -> cpal::StreamConfig {
    let mut config = picked.config();
    if let Some(block) = block {
        config.buffer_size = cpal::BufferSize::Fixed(block);
    }
    config
}

fn device_opens(device: &cpal::Device, picked: &cpal::SupportedStreamConfig, block: Option<u32>) -> bool {
    let probe = device.build_output_stream_raw(stream_config(picked, block), picked.sample_format(), |_, _| {}, |_| {}, None);
    if let Err(e) = &probe {
        eprintln!("could not open audio device, playing silently: {e}");
    }
    probe.is_ok()
}

fn build_stream(
    device: &cpal::Device,
    picked: cpal::SupportedStreamConfig,
    block: Option<u32>,
    first: usize,
    callback: Callback,
    shared: Arc<Shared>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Option<cpal::Stream> {
    let channels = picked.channels() as usize;
    let format = picked.sample_format();
    let stream_config = stream_config(&picked, block);
    let on_error = move |e: cpal::Error| {
        eprintln!("audio stream error: {e}");
        if matches!(e.kind(), cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::StreamInvalidated) {
            shared.device_lost.store(true, Ordering::Relaxed);
            wake();
        }
    };
    let stream = match format {
        cpal::SampleFormat::I16 => {
            let mut cb = callback;
            device.build_output_stream::<i16, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, first, |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I32 => {
            let mut cb = callback;
            device.build_output_stream::<i32, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, first, |s| (s.clamp(-1.0, 1.0) as f64 * i32::MAX as f64) as i32),
                on_error,
                None,
            )
        }
        _ => {
            let mut cb = callback;
            device.build_output_stream::<f32, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, first, |s| s),
                on_error,
                None,
            )
        }
    };
    match stream {
        Ok(stream) => match stream.play() {
            Ok(()) => Some(stream),
            Err(e) => {
                eprintln!("could not start audio: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("could not open audio device: {e}");
            None
        }
    }
}

fn write<T: Copy>(cb: &mut Callback, data: &mut [T], channels: usize, first: usize, convert: impl Fn(f32) -> T) {
    let capacity = cb.left.len();
    for chunk in data.chunks_mut(channels * capacity) {
        let frames = chunk.len() / channels;
        cb.run(frames);
        for (i, frame) in chunk.chunks_mut(channels).enumerate() {
            let (l, r) = (cb.left[i], cb.right[i]);
            if channels == 1 {
                frame[0] = convert((l + r) * 0.5);
                continue;
            }
            for (c, out) in frame.iter_mut().enumerate() {
                *out = convert(if c == first {
                    l
                } else if c == first + 1 {
                    r
                } else {
                    0.0
                });
            }
        }
    }
}

fn run_silent_timer(mut callback: Callback, stop: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new().name("silent-audio".into()).spawn(move || {
        const FRAMES: usize = 512;
        let period = std::time::Duration::from_secs_f32(FRAMES as f32 / callback.sample_rate);
        while !stop.load(Ordering::Relaxed) {
            callback.run(FRAMES);
            std::thread::sleep(period);
        }
    });
}
