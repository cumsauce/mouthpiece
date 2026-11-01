//! Decodes any supported audio file to stereo at the engine's sample rate.

use mp_dsp::{resample::resample, Stereo};
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "aiff", "aif", "aifc", "ogg", "oga", "opus", "m4a", "aac", "mp4", "caf", "mkv"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

pub fn decode_file(path: &Path, target_rate: u32) -> Result<Stereo, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open file: {e}"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("unsupported format: {e}"))?;
    let track = format.default_track(TrackType::Audio).ok_or("file has no audio track")?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("missing codec parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| format!("unsupported codec: {e}"))?;

    let mut out = Stereo::default();
    let mut rate = 0;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // Some containers report the end of a damaged file this way. Keep what decoded.
            Err(Error::IoError(_)) => break,
            Err(e) => return Err(format!("read error: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
            Err(e) => return Err(format!("decode error: {e}")),
        };
        rate = decoded.spec().rate();
        let channels = decoded.spec().channels().count().max(1);
        interleaved.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut interleaved);
        for frame in interleaved.chunks_exact(channels) {
            out.l.push(frame[0]);
            out.r.push(if channels > 1 { frame[1] } else { frame[0] });
        }
    }
    if out.frames() == 0 {
        return Err("no audio decoded".to_string());
    }
    if rate != target_rate {
        let ratio = target_rate as f64 / rate as f64;
        out.l = resample(&out.l, ratio);
        out.r = resample(&out.r, ratio);
    }
    Ok(out)
}
