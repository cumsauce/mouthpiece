//! Renders a project to an audio file, through the same engine code as playback.

use mp_engine::graph::EngineGraph;
use mp_engine::render_offline;
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Id3Tag, Quality};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    /// Constant bitrate in kbit/s: 128, 192, 256 or 320.
    Mp3 { kbps: u32 },
    Wav16,
    Wav24,
    WavFloat,
}

impl Format {
    pub fn extension(&self) -> &'static str {
        match self {
            Format::Mp3 { .. } => "mp3",
            _ => "wav",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportSettings {
    pub format: Format,
    /// First frame to render.
    pub start: u64,
    /// One past the last frame to render.
    pub end: u64,
    /// Extra time rendered after the end so reverb and delay tails ring out.
    pub tail_seconds: f32,
    pub title: String,
}

/// Writes through a temp file, so a failed or cancelled render never leaves a partial file behind.
/// `on_progress` receives 0..=1, and returning false cancels. Returns Ok(false) if cancelled.
pub fn export(
    graph: Box<EngineGraph>,
    sample_rate: u32,
    settings: &ExportSettings,
    path: &Path,
    on_progress: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let tmp = path.with_extension(format!("{}.part", settings.format.extension()));
    let result = match settings.format {
        Format::Mp3 { kbps } => export_mp3(graph, sample_rate, settings, &tmp, kbps, on_progress),
        _ => export_wav(graph, sample_rate, settings, &tmp, on_progress),
    };
    match result {
        Ok(true) => {
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
            Ok(true)
        }
        other => {
            let _ = std::fs::remove_file(&tmp);
            other
        }
    }
}

fn tail_frames(settings: &ExportSettings, sample_rate: u32) -> u64 {
    (settings.tail_seconds * sample_rate as f32) as u64
}

fn export_wav(
    graph: Box<EngineGraph>,
    sample_rate: u32,
    settings: &ExportSettings,
    path: &Path,
    mut on_progress: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let (bits, sample_format) = match settings.format {
        Format::Wav16 => (16, hound::SampleFormat::Int),
        Format::Wav24 => (24, hound::SampleFormat::Int),
        _ => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: 2, sample_rate, bits_per_sample: bits, sample_format };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let mut error = None;
    let mut completed = true;
    let scale = match bits {
        16 => i16::MAX as f32,
        _ => 8_388_607.0,
    };
    render_offline(
        graph,
        sample_rate as f32,
        settings.start,
        settings.end,
        tail_frames(settings, sample_rate),
        |l, r| {
            for (a, b) in l.iter().zip(r) {
                let result = match sample_format {
                    hound::SampleFormat::Float => writer.write_sample(*a).and_then(|_| writer.write_sample(*b)),
                    hound::SampleFormat::Int => writer
                        .write_sample((a.clamp(-1.0, 1.0) * scale) as i32)
                        .and_then(|_| writer.write_sample((b.clamp(-1.0, 1.0) * scale) as i32)),
                };
                if let Err(e) = result {
                    error.get_or_insert_with(|| e.to_string());
                }
            }
        },
        |p| {
            completed = on_progress(p);
            completed
        },
    );
    writer.finalize().map_err(|e| e.to_string())?;
    match error {
        Some(e) => Err(e),
        None => Ok(completed),
    }
}

fn export_mp3(
    graph: Box<EngineGraph>,
    sample_rate: u32,
    settings: &ExportSettings,
    path: &Path,
    kbps: u32,
    mut on_progress: impl FnMut(f32) -> bool,
) -> Result<bool, String> {
    let bitrate = match kbps {
        0..=128 => Bitrate::Kbps128,
        129..=192 => Bitrate::Kbps192,
        193..=256 => Bitrate::Kbps256,
        _ => Bitrate::Kbps320,
    };
    let mut builder = Builder::new().ok_or("could not start the MP3 encoder")?;
    let err = |e: mp3lame_encoder::BuildError| format!("MP3 encoder: {e:?}");
    builder.set_num_channels(2).map_err(err)?;
    builder.set_sample_rate(sample_rate).map_err(err)?;
    builder.set_brate(bitrate).map_err(err)?;
    builder.set_quality(Quality::Best).map_err(err)?;
    let _ = builder.set_id3_tag(Id3Tag {
        title: settings.title.as_bytes(),
        artist: b"",
        album: b"",
        album_art: &[],
        year: b"",
        comment: b"Made with Mouthpiece",
    });
    let mut encoder = builder.build().map_err(err)?;

    let mut file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    let mut out: Vec<u8> = Vec::new();
    let mut error = None;
    let mut completed = true;
    render_offline(
        graph,
        sample_rate as f32,
        settings.start,
        settings.end,
        tail_frames(settings, sample_rate),
        |l, r| {
            out.clear();
            out.reserve(mp3lame_encoder::max_required_buffer_size(l.len()));
            let encoded = encoder.encode_to_vec(DualPcm { left: l, right: r }, &mut out);
            match encoded {
                Ok(_) => {
                    if let Err(e) = file.write_all(&out) {
                        error.get_or_insert_with(|| e.to_string());
                    }
                }
                Err(e) => {
                    error.get_or_insert_with(|| format!("MP3 encoder: {e:?}"));
                }
            }
        },
        |p| {
            completed = on_progress(p);
            completed
        },
    );
    if let Some(e) = error {
        return Err(e);
    }
    if !completed {
        return Ok(false);
    }
    out.clear();
    out.reserve(mp3lame_encoder::max_required_buffer_size(0));
    encoder.flush_to_vec::<FlushNoGap>(&mut out).map_err(|e| format!("MP3 encoder: {e:?}"))?;
    file.write_all(&out).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    Ok(true)
}
