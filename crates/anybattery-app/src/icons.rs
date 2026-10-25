use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2, vec2};
use std::f32::consts::{FRAC_PI_2, TAU};
use tray_icon::Icon;

use anybattery_core::{BatteryStatus, DeviceKind};

pub fn tray_icon(level: Option<u8>, charging: bool) -> Option<Icon> {
    let size = 64u32;
    let (rgba, _, _) = ring_rgba(size, level, charging, true);
    Icon::from_rgba(rgba, size, size).ok()
}

pub fn window_icon() -> egui::IconData {
    let size = 128u32;
    let (rgba, width, height) = ring_rgba(size, Some(78), false, false);
    egui::IconData { rgba, width, height }
}

fn ring_rgba(size: u32, level: Option<u8>, charging: bool, tray: bool) -> (Vec<u8>, u32, u32) {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let outer = size as f32 * 0.43;
    let inner = size as f32 * 0.31;
    let accent = if charging {
        [52.0, 199.0, 89.0]
    } else {
        match level {
            Some(0..=10) => [255.0, 59.0, 48.0],
            Some(11..=20) => [255.0, 159.0, 10.0],
            _ => [0.0, 122.0, 255.0],
        }
    };
    let track = if tray {
        [128.0, 128.0, 128.0]
    } else {
        [255.0, 255.0, 255.0]
    };
    let progress = level.map(|l| (l.min(100) as f32 / 100.0) * TAU).unwrap_or(TAU * 0.72);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let edge = smooth(inner - 0.9, inner + 0.9, distance)
                * (1.0 - smooth(outer - 0.9, outer + 0.9, distance));
            if edge <= 0.0 {
                continue;
            }
            let angle = (dy.atan2(dx) - (-FRAC_PI_2) + TAU) % TAU;
            let filled = level.is_some() && angle <= progress;
            let color = if filled { accent } else { track };
            let alpha = if filled { 255.0 } else if tray { 90.0 } else { 70.0 };
            let i = ((y * size + x) * 4) as usize;
            rgba[i] = color[0] as u8;
            rgba[i + 1] = color[1] as u8;
            rgba[i + 2] = color[2] as u8;
            rgba[i + 3] = (alpha * edge) as u8;
        }
    }

    if charging {
        // Small white bolt in the middle.
        let cx = center;
        let cy = center;
        let s = size as f32 * 0.12;
        let points = [
            Pos2::new(cx - s * 0.15, cy - s),
            Pos2::new(cx + s * 0.55, cy - s * 0.15),
            Pos2::new(cx + s * 0.1, cy - s * 0.15),
            Pos2::new(cx + s * 0.25, cy + s),
            Pos2::new(cx - s * 0.55, cy + s * 0.05),
            Pos2::new(cx - s * 0.05, cy + s * 0.05),
        ];
        fill_polygon(&mut rgba, size, &points, [255, 255, 255, 255]);
    }
    (rgba, size, size)
}

fn smooth(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fill_polygon(rgba: &mut [u8], size: u32, points: &[Pos2], color: [u8; 4]) {
    for y in 0..size {
        for x in 0..size {
            let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            if point_in_polygon(p, points) {
                let i = ((y * size + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&color);
            }
        }
    }
}

fn point_in_polygon(p: Pos2, points: &[Pos2]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let pi = points[i];
        let pj = points[j];
        if (pi.y > p.y) != (pj.y > p.y)
            && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn device_accent(kind: DeviceKind, p: &crate::theme::Palette) -> Color32 {
    match kind {
        DeviceKind::Mouse => p.accent,
        DeviceKind::Keyboard => Color32::from_rgb(94, 92, 230),
        DeviceKind::Headset | DeviceKind::Earbuds => Color32::from_rgb(255, 55, 95),
        DeviceKind::Speaker => Color32::from_rgb(255, 149, 0),
        DeviceKind::Controller => p.green,
        DeviceKind::Pen => Color32::from_rgb(175, 82, 222),
        DeviceKind::Unknown => p.secondary,
    }
}

pub fn draw_device_icon(
    painter: &egui::Painter,
    rect: Rect,
    kind: DeviceKind,
    color: Color32,
) {
    let stroke = Stroke::new(1.55, color);
    let c = rect.center();
    let w = rect.width();
    let h = rect.height();
    match kind {
        DeviceKind::Mouse => {
            let body = Rect::from_center_size(c, vec2(w * 0.40, h * 0.62));
            painter.rect_stroke(body, 7, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [Pos2::new(c.x, body.top() + 2.0), Pos2::new(c.x, c.y - 2.0)],
                Stroke::new(1.3, color),
            );
            painter.line_segment(
                [Pos2::new(body.left() + 2.0, c.y - 2.0), Pos2::new(body.right() - 2.0, c.y - 2.0)],
                Stroke::new(1.2, color.gamma_multiply(0.75)),
            );
        }
        DeviceKind::Keyboard => {
            let body = Rect::from_center_size(c, vec2(w * 0.62, h * 0.42));
            painter.rect_stroke(body, 4, stroke, egui::StrokeKind::Middle);
            for row in 0..2 {
                for col in 0..4 {
                    let x = body.left() + 4.0 + col as f32 * (body.width() - 8.0) / 4.0;
                    let y = body.top() + 4.0 + row as f32 * (body.height() - 8.0) / 2.0;
                    painter.line_segment(
                        [Pos2::new(x, y), Pos2::new(x + 3.0, y)],
                        Stroke::new(1.0, color.gamma_multiply(0.65)),
                    );
                }
            }
        }
        DeviceKind::Headset | DeviceKind::Earbuds => {
            let arc_radius = w * 0.26;
            let points: Vec<Pos2> = (0..=18)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 18.0 * std::f32::consts::PI;
                    Pos2::new(c.x + a.cos() * arc_radius, c.y - 1.0 + a.sin() * arc_radius)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            let cup = vec2(w * 0.12, h * 0.24);
            for x in [c.x - arc_radius, c.x + arc_radius] {
                painter.rect_stroke(
                    Rect::from_center_size(Pos2::new(x, c.y + h * 0.10), cup),
                    3,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
        DeviceKind::Speaker => {
            let body = Rect::from_center_size(c, vec2(w * 0.34, h * 0.58));
            painter.rect_stroke(body, 5, stroke, egui::StrokeKind::Middle);
            painter.circle_stroke(
                Pos2::new(c.x, body.center().y + 2.0),
                w * 0.07,
                Stroke::new(1.2, color),
            );
            painter.circle_filled(Pos2::new(c.x, body.top() + 6.0), 1.2, color);
        }
        DeviceKind::Controller => {
            let body = Rect::from_center_size(c, vec2(w * 0.62, h * 0.38));
            painter.rect_stroke(body, 8, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [Pos2::new(c.x - w * 0.18, c.y - 3.0), Pos2::new(c.x - w * 0.18, c.y + 4.0)],
                Stroke::new(1.2, color),
            );
            painter.line_segment(
                [Pos2::new(c.x - w * 0.22, c.y + 0.5), Pos2::new(c.x - w * 0.14, c.y + 0.5)],
                Stroke::new(1.2, color),
            );
            painter.circle_stroke(Pos2::new(c.x + w * 0.16, c.y - 2.0), 2.0, Stroke::new(1.1, color));
            painter.circle_stroke(Pos2::new(c.x + w * 0.22, c.y + 3.0), 2.0, Stroke::new(1.1, color));
        }
        DeviceKind::Pen => {
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.20, c.y + h * 0.22),
                    Pos2::new(c.x + w * 0.18, c.y - h * 0.20),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.20, c.y + h * 0.22),
                    Pos2::new(c.x - w * 0.10, c.y + h * 0.10),
                ],
                Stroke::new(1.1, color),
            );
        }
        DeviceKind::Unknown => {
            painter.circle_stroke(c, w * 0.22, stroke);
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::new(w * 0.22, egui::FontFamily::Proportional),
                color,
            );
        }
    }
}

pub fn status_dot_color(status: BatteryStatus, p: &crate::theme::Palette) -> Color32 {
    match status {
        BatteryStatus::Available => p.green,
        BatteryStatus::Charging => p.accent,
        BatteryStatus::Full => p.green,
        BatteryStatus::Asleep | BatteryStatus::Stale => p.tertiary,
        BatteryStatus::Unsupported => p.orange,
        BatteryStatus::Error => p.red,
        BatteryStatus::Unknown => p.tertiary,
    }
}
