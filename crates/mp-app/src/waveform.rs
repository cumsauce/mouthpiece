//! Fast waveform drawing, as a single mesh of per-column quads.

use crate::theme::*;
use eframe::egui::{self, epaint::Vertex, Color32, Mesh, Painter, Rect, Shape};
use mp_analysis::peaks::Peaks;
use mp_model::Clip;

pub fn clip_gain(clip: &Clip) -> f32 {
    mp_dsp::db_to_gain(clip.gain_db)
}

/// Draws a clip's waveform into `rect`. `px_per_s` maps arrangement time onto x, and only the part
/// overlapping `clip_rect` is drawn. The colour moves towards a highlight where audio is bright, so
/// consonants and hits stand out from vowels and bass.
#[allow(clippy::too_many_arguments)]
pub fn draw_clip_waveform(
    painter: &Painter,
    rect: Rect,
    clip_rect: Rect,
    clip: &Clip,
    peaks: &Peaks,
    sample_rate: f64,
    base: Color32,
    highlight: Color32,
) {
    let x0 = rect.left().max(clip_rect.left()).floor() as i32;
    let x1 = rect.right().min(clip_rect.right()).ceil() as i32;
    if x1 <= x0 {
        return;
    }
    let mid = rect.center().y;
    let half = rect.height() * 0.5 - 1.0;
    let gain = clip_gain(clip);
    let px_per_s = rect.width() as f64 / clip.length().max(1e-6);
    let mut mesh = Mesh::default();
    let columns = (x1 - x0) as usize;
    mesh.vertices.reserve(columns * 4);
    mesh.indices.reserve(columns * 6);
    let time_at = |x: f32| clip.start + (x - rect.left()) as f64 / px_per_s;
    for x in x0..x1 {
        let (ta, tb) = (time_at(x as f32), time_at(x as f32 + 1.0));
        let bin = peaks.query(clip.source_time_at(ta) * sample_rate, clip.source_time_at(tb) * sample_rate);
        let env = fade_envelope(clip, (ta + tb) * 0.5) * gain;
        let top = mid - (bin.max * env).clamp(-1.0, 1.0) * half;
        let bottom = mid - (bin.min * env).clamp(-1.0, 1.0) * half;
        let (top, bottom) = (top.min(mid - 0.5), bottom.max(mid + 0.5));
        let color = mix(base, highlight, (bin.brightness * 2.2 - 0.1).clamp(0.0, 1.0));
        let i = mesh.vertices.len() as u32;
        let uv = egui::epaint::WHITE_UV;
        for (px, py) in [(x as f32, top), (x as f32 + 1.0, top), (x as f32 + 1.0, bottom), (x as f32, bottom)] {
            mesh.vertices.push(Vertex { pos: egui::pos2(px, py), uv, color });
        }
        mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    painter.add(Shape::mesh(mesh));
}

fn fade_envelope(clip: &Clip, t: f64) -> f32 {
    let from_start = t - clip.start;
    let to_end = clip.end() - t;
    let mut env = 1.0;
    if clip.fade_in > 0.0 && from_start < clip.fade_in {
        env *= (from_start / clip.fade_in).clamp(0.0, 1.0) as f32;
    }
    if clip.fade_out > 0.0 && to_end < clip.fade_out {
        env *= (to_end / clip.fade_out).clamp(0.0, 1.0) as f32;
    }
    env
}
