//! Mapping between arrangement time and screen position, plus ruler and grid drawing.

use crate::theme::*;
use eframe::egui::{self, Align2, Painter, Rect, Stroke};
use mp_model::Project;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeView {
    pub px_per_s: f64,
    /// Time at the left edge of the view.
    pub scroll: f64,
    /// Screen x of time `scroll`.
    pub left: f32,
    pub width: f32,
}

impl TimeView {
    pub fn new(px_per_s: f64) -> Self {
        Self { px_per_s, scroll: 0.0, left: 0.0, width: 1.0 }
    }

    pub fn x(&self, t: f64) -> f32 {
        self.left + ((t - self.scroll) * self.px_per_s) as f32
    }

    pub fn t(&self, x: f32) -> f64 {
        self.scroll + (x - self.left) as f64 / self.px_per_s
    }

    pub fn right_time(&self) -> f64 {
        self.t(self.left + self.width)
    }

    /// Zooms by `factor`, keeping the time under screen position `anchor_x` still.
    pub fn zoom_at(&mut self, factor: f64, anchor_x: f32) {
        let anchor = self.t(anchor_x);
        self.px_per_s = (self.px_per_s * factor).clamp(1.0, 8000.0);
        self.scroll = (anchor - (anchor_x - self.left) as f64 / self.px_per_s).max(0.0);
    }

    /// Fits a time range into the view, with a small margin.
    pub fn fit(&mut self, from: f64, to: f64) {
        let span = (to - from).max(0.01);
        self.px_per_s = (self.width as f64 * 0.94 / span).clamp(1.0, 8000.0);
        self.scroll = (from - span * 0.03).max(0.0);
    }

    pub fn pan(&mut self, dx_px: f32) {
        self.scroll = (self.scroll + dx_px as f64 / self.px_per_s).max(0.0);
    }

    /// Scrolls just enough to bring `t` back into view, as for a playhead.
    pub fn follow(&mut self, t: f64) {
        let span = self.width as f64 / self.px_per_s;
        if t > self.scroll + span * 0.95 || t < self.scroll {
            self.scroll = (t - span * 0.05).max(0.0);
        }
    }
}

/// Picks grid spacing in beats, so lines stay at least `min_px` apart.
pub fn adaptive_step(project: &Project, view: &TimeView, min_px: f64) -> f64 {
    let beat_px = project.beat_seconds() * view.px_per_s;
    let bar = project.beats_per_bar as f64;
    let mut step = 1.0 / 16.0;
    while step * beat_px < min_px {
        step *= if step >= 1.0 && step < bar { bar } else { 2.0 };
    }
    step
}

pub fn draw_grid(painter: &Painter, rect: Rect, project: &Project, view: &TimeView) {
    let step = adaptive_step(project, view, 14.0);
    let beat = project.beat_seconds();
    let bar = project.beats_per_bar as f64;
    let first = ((view.scroll - project.grid_offset) / (beat * step)).floor() as i64;
    let last = ((view.right_time() - project.grid_offset) / (beat * step)).ceil() as i64;
    for i in first..=last {
        let beats = i as f64 * step;
        let t = project.grid_offset + beats * beat;
        if t < 0.0 {
            continue;
        }
        let x = view.x(t).round() + 0.5;
        let is_bar = (beats / bar).fract().abs() < 1e-9;
        let is_beat = beats.fract().abs() < 1e-9;
        let color = if is_bar {
            alpha(BG_4, 170)
        } else if is_beat {
            alpha(BG_4, 90)
        } else {
            alpha(BG_4, 45)
        };
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.0, color));
    }
}

pub fn draw_ruler(painter: &Painter, rect: Rect, project: &Project, view: &TimeView) {
    painter.rect_filled(rect, 0.0, BG_2);
    let label_step = adaptive_step(project, view, 56.0);
    let beat = project.beat_seconds();
    let bar = project.beats_per_bar as f64;
    let first = ((view.scroll - project.grid_offset) / (beat * label_step)).floor() as i64;
    let last = ((view.right_time() - project.grid_offset) / (beat * label_step)).ceil() as i64;
    for i in first..=last {
        let beats = i as f64 * label_step;
        let t = project.grid_offset + beats * beat;
        if t < 0.0 {
            continue;
        }
        let x = view.x(t).round() + 0.5;
        let bars = (beats / bar).floor();
        let sub = beats - bars * bar;
        let is_bar = sub.abs() < 1e-9;
        painter.line_segment(
            [egui::pos2(x, rect.bottom() - if is_bar { 9.0 } else { 5.0 }), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0, if is_bar { TEXT_FAINT } else { BG_4 }),
        );
        // Past sixteenths, musical labels would repeat, so zoomed this far in shows clock time.
        let label = if label_step < 0.25 {
            format_time(t)
        } else if is_bar {
            format!("{}", bars as i64 + 1)
        } else {
            let whole = sub.floor();
            if (sub - whole).abs() < 1e-9 {
                format!("{}.{}", bars as i64 + 1, whole as i64 + 1)
            } else {
                format!("{}.{}.{}", bars as i64 + 1, whole as i64 + 1, ((sub - whole) * 4.0).round() as i64 + 1)
            }
        };
        painter.text(
            egui::pos2(x + 4.0, rect.top() + 3.0),
            Align2::LEFT_TOP,
            label,
            mono(10.0),
            if is_bar { TEXT_DIM } else { TEXT_FAINT },
        );
    }
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, LINE));
}

pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let minutes = (s / 60.0) as u64;
    format!("{:02}:{:06.3}", minutes, s - minutes as f64 * 60.0)
}

pub fn format_bars(project: &Project, seconds: f64) -> String {
    let beats = ((seconds - project.grid_offset) / project.beat_seconds()).max(0.0);
    let per_bar = project.beats_per_bar as f64;
    let bar = (beats / per_bar).floor();
    let beat = (beats - bar * per_bar).floor();
    let sixteenth = ((beats - beats.floor()) * 4.0).floor();
    format!("{}.{}.{}", bar as u64 + 1, beat as u64 + 1, sixteenth as u64 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> TimeView {
        TimeView { px_per_s: 100.0, scroll: 10.0, left: 50.0, width: 500.0 }
    }

    #[test]
    fn x_and_t_are_inverse() {
        let v = view();
        assert_eq!(v.x(10.0), 50.0);
        assert!((v.t(v.x(12.34)) - 12.34).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_anchor_still() {
        let mut v = view();
        let at = v.t(300.0);
        v.zoom_at(2.0, 300.0);
        assert!((v.t(300.0) - at).abs() < 1e-3);
        assert_eq!(v.px_per_s, 200.0);
    }

    #[test]
    fn follow_scrolls_only_when_needed() {
        let mut v = view();
        v.follow(12.0);
        assert_eq!(v.scroll, 10.0);
        v.follow(100.0);
        assert!(v.scroll > 90.0);
    }

    #[test]
    fn bar_formatting() {
        let p = Project::default(); // 120 BPM, 4/4
        assert_eq!(format_bars(&p, 0.0), "1.1.1");
        assert_eq!(format_bars(&p, 2.0), "2.1.1");
        assert_eq!(format_bars(&p, 2.625), "2.2.2");
        assert_eq!(format_time(61.5), "01:01.500");
    }
}
