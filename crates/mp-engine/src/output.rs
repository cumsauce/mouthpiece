//! Connects `EngineCore` to the sound card, and the lock-free bridge between UI and audio threads.

use crate::core::EngineCore;
use crate::graph::{EngineGraph, Voice};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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

/// The UI thread's handle to the running audio engine.
pub struct AudioEngine {
    commands: rtrb::Producer<Command>,
    garbage: rtrb::Consumer<Garbage>,
    pub shared: Arc<Shared>,
    pub sample_rate: u32,
    pub device_name: String,
    /// Keeps the output running. `None` means no device was available, and a timer thread paces
    /// the engine instead so the transport still works.
    _stream: Option<cpal::Stream>,
}

impl AudioEngine {
    /// Opens the default output device, falling back to a silent timer if there is none.
    pub fn start() -> Self {
        let (commands, command_rx) = rtrb::RingBuffer::new(256);
        let (garbage_tx, garbage) = rtrb::RingBuffer::new(1024);
        let shared = Arc::new(Shared::new());
        let host = cpal::default_host();
        let device = host.default_output_device();
        let config = device.as_ref().and_then(|d| d.default_output_config().ok());
        let sample_rate = config.as_ref().map_or(48000, |c| c.sample_rate());
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
        // The stream takes ownership of the callback, so a throwaway stream checks the device first.
        // Without this, a device that won't open would leave the transport frozen.
        let usable = device.zip(config).filter(|(d, c)| device_opens(d, c));
        let stream = match usable {
            Some((device, config)) => build_stream(&device, config, callback),
            None => {
                run_silent_timer(callback);
                None
            }
        };
        Self { commands, garbage, shared, sample_rate, device_name, _stream: stream }
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

fn device_opens(device: &cpal::Device, config: &cpal::SupportedStreamConfig) -> bool {
    let probe = device.build_output_stream_raw(config.config(), config.sample_format(), |_, _| {}, |_| {}, None);
    if let Err(e) = &probe {
        eprintln!("could not open audio device, playing silently: {e}");
    }
    probe.is_ok()
}

fn build_stream(device: &cpal::Device, config: cpal::SupportedStreamConfig, callback: Callback) -> Option<cpal::Stream> {
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let on_error = |e| eprintln!("audio stream error: {e}");
    let stream = match format {
        cpal::SampleFormat::I16 => {
            let mut cb = callback;
            device.build_output_stream::<i16, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I32 => {
            let mut cb = callback;
            device.build_output_stream::<i32, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, |s| (s.clamp(-1.0, 1.0) as f64 * i32::MAX as f64) as i32),
                on_error,
                None,
            )
        }
        _ => {
            let mut cb = callback;
            device.build_output_stream::<f32, _, _>(
                stream_config,
                move |data, _| write(&mut cb, data, channels, |s| s),
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

fn write<T: Copy>(cb: &mut Callback, data: &mut [T], channels: usize, convert: impl Fn(f32) -> T) {
    let capacity = cb.left.len();
    for chunk in data.chunks_mut(channels * capacity) {
        let frames = chunk.len() / channels;
        cb.run(frames);
        for (i, frame) in chunk.chunks_mut(channels).enumerate() {
            let (l, r) = (cb.left[i], cb.right[i]);
            for (c, out) in frame.iter_mut().enumerate() {
                *out = convert(match c {
                    0 => l,
                    1 => r,
                    _ => 0.0,
                });
            }
        }
    }
}

fn run_silent_timer(mut callback: Callback) {
    let _ = std::thread::Builder::new().name("silent-audio".into()).spawn(move || {
        const FRAMES: usize = 512;
        let period = std::time::Duration::from_secs_f32(FRAMES as f32 / callback.sample_rate);
        loop {
            callback.run(FRAMES);
            std::thread::sleep(period);
        }
    });
}
