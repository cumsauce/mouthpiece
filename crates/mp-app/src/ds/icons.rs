//! Vector icons, drawn from simple strokes so they stay crisp at any scale.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Play,
    Pause,
    ToStart,
    Loop,
    Metronome,
    Magnet,
    Plus,
    Minus,
    Close,
    Search,
    Power,
    Reverse,
    Undo,
    Redo,
    PanelLeft,
    PanelBottom,
    PanelRight,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Repeat,
    Split,
    Trash,
    Wand,
    Note,
    Faster,
    Slower,
    Commands,
    Check,
    Crossfade,
    Link,
    Bookmark,
    Restore,
}

pub(super) fn arc_points(c: Pos2, r: f32, from: f32, to: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

pub fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.5, color);
    let v = egui::vec2;
    let at = |p: Vec2| c + p * r;
    let line = |a: Vec2, b: Vec2| painter.line_segment([at(a), at(b)], stroke);
    let poly = |points: &[Vec2]| {
        painter.add(Shape::line(points.iter().map(|p| at(*p)).collect(), stroke));
    };
    let solid = |points: &[Vec2]| {
        painter.add(Shape::convex_polygon(points.iter().map(|p| at(*p)).collect(), color, Stroke::NONE));
    };
    let panel = |bar: Rect| {
        let outer = Rect::from_center_size(c, v(1.7, 1.5) * r);
        painter.rect_stroke(outer, CornerRadius::same(2), stroke, egui::StrokeKind::Middle);
        let scaled = Rect::from_min_max(at(bar.min.to_vec2()), at(bar.max.to_vec2()));
        painter.rect_filled(scaled, CornerRadius::ZERO, color);
    };
    let bar_rect = |x0, y0, x1, y1| Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
    match icon {
        Icon::Play => solid(&[v(-0.55, -0.78), v(0.8, 0.0), v(-0.55, 0.78)]),
        Icon::Pause => {
            for x in [-0.55, 0.2] {
                painter.rect_filled(Rect::from_min_size(at(v(x, -0.7)), v(0.38, 1.4) * r), CornerRadius::same(1), color);
            }
        }
        Icon::ToStart => {
            painter.rect_filled(Rect::from_min_size(at(v(-0.75, -0.7)), v(0.25, 1.4) * r), CornerRadius::same(1), color);
            solid(&[v(0.75, -0.7), v(0.75, 0.7), v(-0.35, 0.0)]);
        }
        Icon::Loop => {
            let points = arc_points(c, r * 0.72, -0.25 * PI, 1.4 * PI, 24);
            let end = points[points.len() - 1];
            painter.add(Shape::line(points, stroke));
            painter.line_segment([end, end + v(0.45, -0.1) * r], stroke);
            painter.line_segment([end, end + v(0.1, -0.45) * r], stroke);
        }
        Icon::Metronome => {
            poly(&[v(-0.6, 0.8), v(-0.25, -0.8), v(0.25, -0.8), v(0.6, 0.8), v(-0.6, 0.8)]);
            line(v(0.0, 0.35), v(0.55, -0.55));
        }
        Icon::Magnet => {
            let mut points = vec![at(v(0.6, 0.75))];
            points.extend(arc_points(at(v(0.0, -0.05)), r * 0.6, 0.0, -PI, 16));
            points.push(at(v(-0.6, 0.75)));
            painter.add(Shape::line(points, Stroke::new(2.0, color)));
        }
        Icon::Plus => {
            line(v(-0.7, 0.0), v(0.7, 0.0));
            line(v(0.0, -0.7), v(0.0, 0.7));
        }
        Icon::Minus => {
            line(v(-0.7, 0.0), v(0.7, 0.0));
        }
        Icon::Close => {
            line(v(-0.55, -0.55), v(0.55, 0.55));
            line(v(-0.55, 0.55), v(0.55, -0.55));
        }
        Icon::Search => {
            painter.circle_stroke(at(v(-0.15, -0.15)), r * 0.52, stroke);
            line(v(0.25, 0.25), v(0.8, 0.8));
        }
        Icon::Power => {
            painter.add(Shape::line(arc_points(c, r * 0.65, -0.3 * PI, 1.3 * PI, 20), stroke));
            line(v(0.0, -0.9), v(0.0, -0.2));
        }
        Icon::Reverse => {
            line(v(0.8, 0.0), v(-0.8, 0.0));
            line(v(-0.8, 0.0), v(-0.3, -0.5));
            line(v(-0.8, 0.0), v(-0.3, 0.5));
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Undo { 1.0 } else { -1.0 };
            poly(&[v(-0.7 * flip, -0.25), v(0.25 * flip, -0.25), v(0.7 * flip, 0.1), v(0.25 * flip, 0.55), v(-0.3 * flip, 0.55)]);
            poly(&[v(-0.3 * flip, -0.65), v(-0.7 * flip, -0.25), v(-0.3 * flip, 0.15)]);
        }
        Icon::PanelLeft => panel(bar_rect(-0.85, -0.75, -0.3, 0.75)),
        Icon::PanelRight => panel(bar_rect(0.3, -0.75, 0.85, 0.75)),
        Icon::PanelBottom => panel(bar_rect(-0.85, 0.2, 0.85, 0.75)),
        Icon::ChevronLeft => poly(&[v(0.25, -0.6), v(-0.3, 0.0), v(0.25, 0.6)]),
        Icon::ChevronRight => poly(&[v(-0.25, -0.6), v(0.3, 0.0), v(-0.25, 0.6)]),
        Icon::ChevronDown => poly(&[v(-0.6, -0.25), v(0.0, 0.3), v(0.6, -0.25)]),
        Icon::Repeat => {
            for x in [-0.75, -0.2, 0.35] {
                painter.rect_filled(Rect::from_min_size(at(v(x, -0.6)), v(0.38, 1.2) * r), CornerRadius::same(1), color);
            }
        }
        Icon::Split => {
            line(v(0.0, -0.9), v(0.0, 0.9));
            poly(&[v(-0.8, -0.35), v(-0.35, 0.0), v(-0.8, 0.35)]);
            poly(&[v(0.8, -0.35), v(0.35, 0.0), v(0.8, 0.35)]);
        }
        Icon::Trash => {
            line(v(-0.7, -0.5), v(0.7, -0.5));
            line(v(-0.25, -0.75), v(0.25, -0.75));
            poly(&[v(-0.5, -0.5), v(-0.4, 0.8), v(0.4, 0.8), v(0.5, -0.5)]);
        }
        Icon::Wand => {
            line(v(-0.75, 0.75), v(0.3, -0.3));
            for (a, b) in [(v(0.6, -0.85), v(0.6, -0.35)), (v(0.35, -0.6), v(0.85, -0.6)), (v(-0.2, -0.75), v(-0.2, -0.45))] {
                line(a, b);
            }
        }
        Icon::Note => {
            painter.circle_filled(at(v(-0.3, 0.55)), r * 0.28, color);
            line(v(-0.05, 0.55), v(-0.05, -0.75));
            line(v(-0.05, -0.75), v(0.55, -0.45));
        }
        Icon::Faster | Icon::Slower => {
            let (a, b) = if icon == Icon::Faster { (-0.15, 0.55) } else { (0.15, -0.55) };
            for x in [-0.35, 0.3] {
                poly(&[v(x + a, -0.6), v(x + b, 0.0), v(x + a, 0.6)]);
            }
        }
        Icon::Commands => {
            for (x, y) in [(-0.55, -0.55), (0.15, -0.55), (-0.55, 0.15), (0.15, 0.15)] {
                painter.rect_stroke(Rect::from_min_size(at(v(x, y)), v(0.4, 0.4) * r), CornerRadius::same(1), stroke, egui::StrokeKind::Middle);
            }
        }
        Icon::Check => poly(&[v(-0.7, 0.05), v(-0.2, 0.55), v(0.7, -0.55)]),
        Icon::Crossfade => {
            line(v(-0.85, -0.6), v(0.85, 0.6));
            line(v(-0.85, 0.6), v(0.85, -0.6));
        }
        Icon::Link => {
            painter.rect_stroke(Rect::from_min_size(at(v(-0.85, -0.3)), v(0.95, 0.6) * r), CornerRadius::same(3), stroke, egui::StrokeKind::Middle);
            painter.rect_stroke(Rect::from_min_size(at(v(-0.1, -0.3)), v(0.95, 0.6) * r), CornerRadius::same(3), stroke, egui::StrokeKind::Middle);
        }
        Icon::Bookmark => poly(&[v(-0.5, -0.8), v(0.5, -0.8), v(0.5, 0.8), v(0.0, 0.35), v(-0.5, 0.8), v(-0.5, -0.8)]),
        Icon::Restore => {
            let points = arc_points(c, r * 0.72, -0.9 * PI, 0.75 * PI, 24);
            let begin = points[0];
            painter.add(Shape::line(points, stroke));
            painter.line_segment([begin, begin + v(-0.1, -0.5) * r], stroke);
            painter.line_segment([begin, begin + v(0.45, -0.15) * r], stroke);
        }
    }
}
