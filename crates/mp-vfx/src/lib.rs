//! GPU picture effects. `resolve_chain` works out what every effect is doing at a moment in time,
//! and `Renderer` draws it. Preview and export both go through this, so they match exactly.

mod renderer;

pub use renderer::{Renderer, OUTPUT_FORMAT};

use mp_analysis::peaks::Peaks;
use mp_engine::{Curve, ParamInfo, Unit};
use mp_model::picture::*;
use mp_model::{Project, SourceId, TrackId};

const fn param(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, curve: Curve) -> ParamInfo {
    ParamInfo { id, name, min, max, default, unit, curve }
}
use Curve::{Linear as Lin, Log};

const TRANSFORM: &[ParamInfo] = &[
    param(0, "X", -1.0, 1.0, 0.0, Unit::None, Lin),
    param(1, "Y", -1.0, 1.0, 0.0, Unit::None, Lin),
    param(2, "Zoom", 0.1, 8.0, 1.0, Unit::Times, Log),
    param(3, "Rotate", -180.0, 180.0, 0.0, Unit::Degrees, Lin),
    param(4, "Opacity", 0.0, 1.0, 1.0, Unit::Percent, Lin),
];
const COLOR: &[ParamInfo] = &[
    param(0, "Brightness", -1.0, 1.0, 0.0, Unit::None, Lin),
    param(1, "Contrast", 0.0, 3.0, 1.0, Unit::Times, Lin),
    param(2, "Saturation", 0.0, 3.0, 1.0, Unit::Times, Lin),
    param(3, "Hue", -180.0, 180.0, 0.0, Unit::Degrees, Lin),
    param(4, "Invert", 0.0, 1.0, 0.0, Unit::Percent, Lin),
];
const BLUR: &[ParamInfo] = &[param(0, "Amount", 0.0, 1.0, 0.2, Unit::Percent, Lin)];
const WARP: &[ParamInfo] = &[
    param(0, "X", 0.0, 1.0, 0.5, Unit::None, Lin),
    param(1, "Y", 0.0, 1.0, 0.5, Unit::None, Lin),
    param(2, "Radius", 0.01, 1.0, 0.3, Unit::None, Log),
    param(3, "Bulge", -1.0, 1.0, 0.5, Unit::None, Lin),
    param(4, "Twirl", -360.0, 360.0, 0.0, Unit::Degrees, Lin),
];
const RGB_SPLIT: &[ParamInfo] = &[
    param(0, "Amount", 0.0, 0.1, 0.01, Unit::None, Lin),
    param(1, "Angle", -180.0, 180.0, 0.0, Unit::Degrees, Lin),
];
const PIXELATE: &[ParamInfo] = &[
    param(0, "Size", 0.001, 0.2, 0.02, Unit::None, Log),
    param(1, "Levels", 2.0, 256.0, 256.0, Unit::None, Log),
];
const GLITCH: &[ParamInfo] = &[
    param(0, "Amount", 0.0, 1.0, 0.3, Unit::Percent, Lin),
    param(1, "Blocks", 2.0, 64.0, 16.0, Unit::None, Log),
    param(2, "Speed", 0.0, 30.0, 8.0, Unit::None, Lin),
    param(3, "Offset", 0.0, 0.5, 0.1, Unit::None, Lin),
];
const SHAKE: &[ParamInfo] = &[
    param(0, "Amount", 0.0, 0.2, 0.02, Unit::None, Lin),
    param(1, "Speed", 0.0, 40.0, 12.0, Unit::None, Lin),
    param(2, "Rotate", 0.0, 20.0, 0.0, Unit::Degrees, Lin),
];
const MIRROR: &[ParamInfo] = &[
    param(0, "Segments", 1.0, 12.0, 2.0, Unit::None, Lin),
    param(1, "Angle", -180.0, 180.0, 0.0, Unit::Degrees, Lin),
];
const FLASH: &[ParamInfo] = &[
    param(0, "Amount", 0.0, 1.0, 0.0, Unit::Percent, Lin),
    param(1, "Hue", 0.0, 360.0, 0.0, Unit::Degrees, Lin),
    param(2, "Saturation", 0.0, 1.0, 0.0, Unit::Percent, Lin),
];

/// An effect's own parameters. The order fixes where each goes in the shader.
pub fn params_of(kind: PictureKind) -> &'static [ParamInfo] {
    match kind {
        PictureKind::Transform => TRANSFORM,
        PictureKind::Color => COLOR,
        PictureKind::Blur => BLUR,
        PictureKind::Warp => WARP,
        PictureKind::RgbSplit => RGB_SPLIT,
        PictureKind::Pixelate => PIXELATE,
        PictureKind::Glitch => GLITCH,
        PictureKind::Shake => SHAKE,
        PictureKind::Mirror => MIRROR,
        PictureKind::Flash => FLASH,
    }
}

/// The parameters every effect gets when it's limited to an area.
pub const AREA_PARAMS: &[ParamInfo] = &[
    param(AREA_X, "Area X", 0.0, 1.0, 0.5, Unit::None, Lin),
    param(AREA_Y, "Area Y", 0.0, 1.0, 0.5, Unit::None, Lin),
    param(AREA_W, "Width", 0.01, 1.0, 0.4, Unit::None, Log),
    param(AREA_H, "Height", 0.01, 1.0, 0.4, Unit::None, Log),
    param(AREA_FEATHER, "Feather", 0.0, 1.0, 0.2, Unit::Percent, Lin),
];

pub fn param_info(kind: PictureKind, id: u32) -> Option<&'static ParamInfo> {
    params_of(kind).iter().chain(AREA_PARAMS).find(|p| p.id == id)
}

/// One effect at one moment, ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pass {
    /// The shader's number for this effect. See `shader.wgsl`.
    pub kind: u32,
    pub params: [f32; 12],
    /// Position and size.
    pub area: [f32; 4],
    pub area_on: bool,
    pub ellipse: bool,
    pub invert: bool,
    pub feather: f32,
}

fn kind_number(kind: PictureKind) -> u32 {
    PictureKind::ALL.iter().position(|k| *k == kind).map_or(0, |i| i as u32 + 1)
}

/// The value of an effect's parameter at a time, within its range.
pub fn value_at(effect: &PictureEffect, info: &ParamInfo, project: &Project, t: f64, level: &dyn Fn(TrackId, f64) -> f32) -> f32 {
    let base = effect.params.get(&info.id).copied().unwrap_or(info.default);
    let Some(motion) = effect.motion.get(&info.id) else { return base };
    let heard = motion.follow.map_or(0.0, |f| level(f.track, t));
    motion.value(base, t, project.beats_at(t), heard).clamp(info.min, info.max)
}

/// What a chain of effects is doing at a time.
pub fn resolve_chain(chain: &[PictureEffect], project: &Project, t: f64, level: &dyn Fn(TrackId, f64) -> f32) -> Vec<Pass> {
    chain
        .iter()
        .filter(|e| e.enabled)
        .map(|effect| {
            let mut params = [0f32; 12];
            for (i, info) in params_of(effect.kind).iter().enumerate().take(12) {
                params[i] = value_at(effect, info, project, t, level);
            }
            let area = |id: u32| {
                let info = AREA_PARAMS.iter().find(|p| p.id == id).unwrap_or(&AREA_PARAMS[0]);
                value_at(effect, info, project, t, level)
            };
            Pass {
                kind: kind_number(effect.kind),
                params,
                area: [area(AREA_X), area(AREA_Y), area(AREA_W), area(AREA_H)],
                area_on: effect.area,
                ellipse: effect.area_ellipse,
                invert: effect.area_invert,
                feather: area(AREA_FEATHER),
            }
        })
        .collect()
}

/// The full chain for the picture at a time: the showing clip's track, then master.
pub fn resolve_frame(project: &Project, t: f64, level: &dyn Fn(TrackId, f64) -> f32) -> Vec<Pass> {
    let track = project.held_picture_clip(t).and_then(|c| project.track_of_clip(c.id));
    let mut passes = track.and_then(|id| project.track(id)).map_or_else(Vec::new, |tr| resolve_chain(&tr.picture, project, t, level));
    passes.extend(resolve_chain(&project.master.picture, project, t, level));
    passes
}

/// How loud a track is at a time, from its waveform. Worked out from the time alone, so that scrubbing
/// and rendering get the same answer as playing. It rises instantly and falls away, like a meter.
pub fn track_level<'a>(project: &Project, track: TrackId, t: f64, peaks: &impl Fn(SourceId) -> Option<&'a Peaks>, sample_rate: f64) -> f32 {
    const STEPS: usize = 10;
    const STEP: f64 = 0.015;
    const RELEASE: f32 = 0.72;
    let Some(track) = project.track(track) else { return 0.0 };
    let mut best = 0f32;
    let mut fall = 1f32;
    for step in 0..STEPS {
        let at = t - step as f64 * STEP;
        for clip in track.clips.iter().filter(|c| !c.muted && c.start <= at && at < c.end()) {
            let Some(p) = peaks(clip.source) else { continue };
            let a = clip.source_time_at(at) * sample_rate;
            let bin = p.query(a, a + STEP * sample_rate);
            let gain = 10f32.powf(clip.gain_db / 20.0);
            best = best.max(bin.max.abs().max(bin.min.abs()) * gain * fall);
        }
        fall *= RELEASE;
    }
    best.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn silent(_: TrackId, _: f64) -> f32 {
        0.0
    }

    #[test]
    fn every_effect_has_a_shader_number_and_fits_the_uniforms() {
        for kind in PictureKind::ALL {
            assert!(kind_number(kind) >= 1);
            assert!(params_of(kind).len() <= 12, "{kind:?}");
        }
    }

    #[test]
    fn disabled_effects_are_skipped_and_defaults_apply() {
        let mut p = Project::default();
        let a = p.add_picture_effect(None, PictureKind::Transform);
        let b = p.add_picture_effect(None, PictureKind::Flash);
        p.picture_effect_mut(b).unwrap().enabled = false;
        let passes = resolve_chain(&p.master.picture, &p, 0.0, &silent);
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].params[2], 1.0, "default zoom");
        let _ = a;
    }

    #[test]
    fn motion_changes_with_time_and_stays_in_range() {
        let mut p = Project::default();
        let id = p.add_picture_effect(None, PictureKind::Transform);
        let effect = p.picture_effect_mut(id).unwrap();
        effect.motion.insert(2, Motion { beat: Some(BeatMotion { every: 1.0, shape: PulseShape::Decay, amount: 7.0 }), ..Default::default() });
        let on_beat = resolve_chain(&p.master.picture, &p, 0.0, &silent)[0].params[2];
        let later = resolve_chain(&p.master.picture, &p, 0.2, &silent)[0].params[2];
        assert_eq!(on_beat, 8.0);
        assert!(later < on_beat);
    }

    #[test]
    fn levels_come_from_the_waveform_and_fall_away() {
        let mut p = Project::default();
        let t = p.add_track("Drums", mp_model::TrackKind::Drums);
        let s = p.add_source("d".into(), "d".into(), 2.0, mp_model::TrackKind::Drums);
        p.add_clip(t, s, 0.0, 0.0, 2.0).unwrap();
        // Loud for the first half second, then silent.
        let mut x = vec![0.8f32; 24000];
        x.extend(vec![0f32; 72000]);
        let mut peaks = HashMap::new();
        peaks.insert(s, Peaks::build(&x));
        let at = |time| track_level(&p, t, time, &|source| peaks.get(&source), 48000.0);
        assert!(at(0.2) > 0.7);
        assert!(at(0.55) > 0.0 && at(0.55) < 0.7, "{}", at(0.55));
        assert_eq!(at(1.5), 0.0);
    }

    #[test]
    fn the_gpu_draws_what_it_is_asked_to() {
        // Machines with no GPU at all can't run this, which isn't a failure of the code.
        let Some(mut renderer) = Renderer::headless(8, 8) else { return };
        let red = [255u8, 0, 0, 255].repeat(64);
        renderer.upload(&red, 8, 8);
        renderer.render(&[], 0.0, 1.0);
        let same = renderer.read_output();
        assert_eq!(&same[..4], &[255, 0, 0, 255], "no effects leaves it alone");

        // Full flash paints the whole frame white.
        let mut params = [0f32; 12];
        params[0] = 1.0;
        let flash = Pass { kind: kind_number(PictureKind::Flash), params, area: [0.5; 4], area_on: false, ellipse: true, invert: false, feather: 0.0 };
        renderer.render(&[flash], 0.0, 1.0);
        let white = renderer.read_output();
        assert!(white[0] > 250 && white[1] > 250 && white[2] > 250, "{:?}", &white[..4]);

        // Limited to a small area, the corners keep their colour and the middle changes.
        let area = Pass { area_on: true, area: [0.5, 0.5, 0.3, 0.3], feather: 0.0, ..flash };
        renderer.render(&[area, area], 0.0, 1.0);
        let out = renderer.read_output();
        assert_eq!(&out[..3], &[255, 0, 0], "corner");
        let middle = (4 * 8 + 4) * 4;
        assert!(out[middle + 1] > 250, "middle {:?}", &out[middle..middle + 4]);
    }
}
