use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, vec2};
use tray_icon::Icon;

use anybattery_core::{BatteryStatus, DeviceKind};

pub fn tray_icon(level: Option<u8>, charging: bool) -> Option<Icon> {
    let size = 64u32;
    let rgba = battery_rgba(size, level, charging);
    Icon::from_rgba(rgba, size, size).ok()
}

pub fn window_icon() -> egui::IconData {
    let size = 128u32;
    let rgba = battery_rgba(size, Some(78), false);
    egui::IconData { rgba, width: size, height: size }
}

fn battery_rgba(size: u32, level: Option<u8>, charging: bool) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let scale = size as f32 / 64.0;
    let color = if charging {
        [79u8, 138, 91]
    } else {
        match level {
            Some(0..=10) => [142, 47, 41],
            Some(11..=20) => [217, 138, 43],
            _ => [200, 68, 60],
        }
    };
    let ink = [46u8, 43, 38];
    let block = |x: u32, y: u32, w: u32, h: u32, c: [u8; 4], rgba: &mut Vec<u8>| {
        for yy in y..(y + h).min(size) {
            for xx in x..(x + w).min(size) {
                let i = ((yy * size + xx) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    };
    let s = |v: f32| (v * scale).round() as u32;
    // outline body
    block(s(8.0), s(20.0), s(36.0), s(24.0), [ink[0], ink[1], ink[2], 255], &mut rgba);
    block(s(10.0), s(22.0), s(32.0), s(20.0), [0, 0, 0, 0], &mut rgba);
    // nub
    block(s(44.0), s(27.0), s(6.0), s(10.0), [ink[0], ink[1], ink[2], 255], &mut rgba);
    // cells
    let cells = 5u32;
    let filled = level.map(|l| ((l.min(100) as f32 / 100.0) * cells as f32).ceil() as u32).unwrap_or(0);
    for i in 0..cells {
        let x = 13.0 + i as f32 * 5.6;
        let c = if i < filled { color } else { [120, 112, 98] };
        block(s(x), s(25.0), s(4.0), s(14.0), [c[0], c[1], c[2], 255], &mut rgba);
    }
    if charging {
        let bolt = [
            (30, 23), (26, 31), (30, 31), (27, 41), (34, 32), (30, 32), (34, 23),
        ];
        for pair in bolt.windows(2) {
            draw_thick_line(&mut rgba, size, pair[0], pair[1], 3, [247, 242, 231, 255]);
        }
    }
    rgba
}

fn draw_thick_line(rgba: &mut [u8], size: u32, a: (u32, u32), b: (u32, u32), thickness: i32, color: [u8; 4]) {
    let steps = 24;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = a.0 as f32 + (b.0 as f32 - a.0 as f32) * t;
        let y = a.1 as f32 + (b.1 as f32 - a.1 as f32) * t;
        for dy in -thickness..=thickness {
            for dx in -thickness..=thickness {
                let px = x.round() as i32 + dx;
                let py = y.round() as i32 + dy;
                if px >= 0 && py >= 0 && px < size as i32 && py < size as i32 {
                    let idx = ((py as u32 * size + px as u32) * 4) as usize;
                    rgba[idx..idx + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

pub fn device_accent(kind: DeviceKind) -> Color32 {
    match kind {
        DeviceKind::Mouse => Color32::from_rgb(200, 68, 60),
        DeviceKind::Keyboard => Color32::from_rgb(62, 111, 163),
        DeviceKind::Headset | DeviceKind::Earbuds => Color32::from_rgb(181, 82, 57),
        DeviceKind::Speaker => Color32::from_rgb(217, 138, 43),
        DeviceKind::Controller => Color32::from_rgb(79, 138, 91),
        DeviceKind::Pen => Color32::from_rgb(123, 94, 167),
        DeviceKind::Unknown => Color32::from_rgb(111, 106, 94),
    }
}

pub fn draw_device_icon(painter: &egui::Painter, rect: Rect, kind: DeviceKind, color: Color32) {
    let c = rect.center();
    let px = (rect.width() / 16.0).max(1.0);
    let block = |x: f32, y: f32, w: f32, h: f32, color: Color32| {
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(c.x + x * px, c.y + y * px), vec2(w * px, h * px)),
            egui::CornerRadius::ZERO,
            color,
        );
    };
    match kind {
        DeviceKind::Mouse => {
            block(-3.0, -7.0, 6.0, 14.0, color);
            block(-2.0, -6.0, 4.0, 12.0, Color32::TRANSPARENT);
            block(-3.0, -7.0, 6.0, 2.0, color);
            block(-0.5, -5.0, 1.0, 3.0, color);
        }
        DeviceKind::Keyboard => {
            block(-7.0, -4.0, 14.0, 8.0, color);
            block(-6.0, -3.0, 12.0, 1.0, Color32::TRANSPARENT);
            for row in 0..2 {
                for col in 0..4 {
                    block(-5.0 + col as f32 * 2.7, -2.0 + row as f32 * 2.2, 1.4, 1.2, color);
                }
            }
        }
        DeviceKind::Headset | DeviceKind::Earbuds => {
            block(-6.0, -6.0, 12.0, 2.0, color);
            block(-7.0, -5.0, 2.0, 7.0, color);
            block(5.0, -5.0, 2.0, 7.0, color);
            block(-8.0, 1.0, 3.0, 5.0, color);
            block(5.0, 1.0, 3.0, 5.0, color);
        }
        DeviceKind::Speaker => {
            block(-4.0, -7.0, 8.0, 14.0, color);
            block(-3.0, -6.0, 6.0, 4.0, Color32::TRANSPARENT);
            block(-2.0, 1.0, 4.0, 4.0, Color32::TRANSPARENT);
            block(-1.0, -2.0, 2.0, 2.0, color);
        }
        DeviceKind::Controller => {
            block(-7.0, -4.0, 14.0, 8.0, color);
            block(-6.0, -3.0, 12.0, 6.0, Color32::TRANSPARENT);
            block(-5.0, -1.0, 3.0, 1.0, color);
            block(-4.0, -2.0, 1.0, 3.0, color);
            block(3.0, -2.0, 1.0, 1.0, color);
            block(5.0, 0.0, 1.0, 1.0, color);
        }
        DeviceKind::Pen => {
            for i in 0..8 {
                block(-5.0 + i as f32, 4.0 - i as f32, 2.0, 2.0, color);
            }
            block(-6.0, 4.0, 2.0, 2.0, color);
        }
        DeviceKind::Unknown => {
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::new(rect.width() * 0.7, egui::FontFamily::Name("bold".into())),
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

pub fn draw_pixel_shadow(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(rect.translate(vec2(3.0, 3.0)), egui::CornerRadius::ZERO, color);
}

pub fn pixel_frame(painter: &egui::Painter, rect: Rect, fill: Color32, border: Color32) {
    painter.rect_filled(rect, egui::CornerRadius::ZERO, fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(2.0, border),
        StrokeKind::Inside,
    );
}
