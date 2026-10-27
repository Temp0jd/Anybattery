use std::sync::OnceLock;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, vec2};
use tray_icon::Icon;

use anybattery_core::{BatteryStatus, DeviceKind};
use crate::theme::{self, Palette};

/// Same 16x16 map used by `build.rs` to generate the Windows icon resource.
const ICON_SOURCE: &str = include_str!("../assets/anybattery-icon.pixels");
const ICON_SIZE: usize = 16;
const ICON_INK: [u8; 4] = [46, 43, 38, 255];
const ICON_PANEL: [u8; 4] = [244, 239, 228, 255];
const ICON_EMPTY_CELL: [u8; 4] = [201, 193, 174, 255];
const ICON_BOLT: [u8; 4] = [247, 242, 231, 255];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IconPixel {
    Transparent,
    Ink,
    Panel,
    Cell(u8),
}

fn icon_grid() -> &'static [[IconPixel; ICON_SIZE]; ICON_SIZE] {
    static GRID: OnceLock<[[IconPixel; ICON_SIZE]; ICON_SIZE]> = OnceLock::new();
    GRID.get_or_init(parse_icon_grid)
}

fn parse_icon_grid() -> [[IconPixel; ICON_SIZE]; ICON_SIZE] {
    let mut grid = [[IconPixel::Transparent; ICON_SIZE]; ICON_SIZE];
    let mut rows = 0;
    for (y, line) in ICON_SOURCE.lines().filter(|line| !line.trim().is_empty()).enumerate() {
        assert!(y < ICON_SIZE, "icon map has more than {ICON_SIZE} rows");
        let mut columns = 0;
        for (x, ch) in line.chars().enumerate() {
            assert!(x < ICON_SIZE, "icon map row {y} is wider than {ICON_SIZE}");
            grid[y][x] = match ch {
                '.' => IconPixel::Transparent,
                '#' => IconPixel::Ink,
                'w' => IconPixel::Panel,
                '1' => IconPixel::Cell(1),
                '2' => IconPixel::Cell(2),
                '3' => IconPixel::Cell(3),
                _ => panic!("unknown icon pixel {ch:?} at {x},{y}"),
            };
            columns += 1;
        }
        assert_eq!(columns, ICON_SIZE, "icon map row {y} must have {ICON_SIZE} columns");
        rows += 1;
    }
    assert_eq!(rows, ICON_SIZE, "icon map must have {ICON_SIZE} rows");
    grid
}

fn filled_cells(level: Option<u8>) -> u8 {
    level
        .map(|level| ((level.min(100) as u32 * 3 + 99) / 100) as u8)
        .unwrap_or(0)
}

fn icon_pixel_color(pixel: IconPixel, filled: u8, fill: [u8; 4]) -> [u8; 4] {
    match pixel {
        IconPixel::Transparent => [0, 0, 0, 0],
        IconPixel::Ink => ICON_INK,
        IconPixel::Panel => ICON_PANEL,
        IconPixel::Cell(index) => {
            if index <= filled {
                fill
            } else {
                ICON_EMPTY_CELL
            }
        }
    }
}

/// The 16px charging bolt, in icon-map coordinates. Drawn after the cells so it stays legible.
const BOLT: [(usize, usize); 10] = [
    (7, 4),
    (6, 5),
    (7, 5),
    (6, 6),
    (5, 7),
    (6, 7),
    (7, 7),
    (8, 7),
    (7, 8),
    (6, 9),
];

pub fn tray_icon(level: Option<u8>, charging: bool) -> Option<Icon> {
    let size = 64u32;
    let rgba = battery_rgba(size, level, charging);
    Icon::from_rgba(rgba, size, size).ok()
}

pub fn window_icon() -> egui::IconData {
    let size = 128u32;
    let rgba = brand_rgba(size);
    egui::IconData { rgba, width: size, height: size }
}

/// Tray icon: the five-tier colour comes from the same helper the device cards use.
pub fn battery_rgba(size: u32, level: Option<u8>, charging: bool) -> Vec<u8> {
    let p = theme::palette(false);
    let color = theme::battery_color_for_level(p, level.unwrap_or(0));
    render_rgba(
        size,
        filled_cells(level),
        [color.r(), color.g(), color.b(), 255],
        charging,
    )
}

/// Static brand icon: the fixed accent-red cells used by the exe, window and title bar.
fn brand_rgba(size: u32) -> Vec<u8> {
    render_rgba(size, 3, [200, 68, 60, 255], false)
}

fn render_rgba(size: u32, filled: u8, fill: [u8; 4], charging: bool) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let grid = icon_grid();
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let color = icon_pixel_color(grid[y][x], filled, fill);
            if color[3] == 0 {
                continue;
            }
            fill_icon_pixel(&mut rgba, size, x, y, color);
        }
    }
    if charging {
        for &(x, y) in &BOLT {
            fill_icon_pixel(&mut rgba, size, x, y, ICON_BOLT);
        }
    }
    rgba
}

fn fill_icon_pixel(rgba: &mut [u8], size: u32, x: usize, y: usize, color: [u8; 4]) {
    let x0 = x as u32 * size / ICON_SIZE as u32;
    let x1 = (x as u32 + 1) * size / ICON_SIZE as u32;
    let y0 = y as u32 * size / ICON_SIZE as u32;
    let y1 = (y as u32 + 1) * size / ICON_SIZE as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            let index = ((py * size + px) * 4) as usize;
            rgba[index..index + 4].copy_from_slice(&color);
        }
    }
}

/// Draw the same pixel battery in the title bar, using theme colours so it stays legible in
/// both light and dark themes. Every cell edge is snapped to the physical pixel grid.
pub fn draw_battery_mark(painter: &egui::Painter, rect: Rect, p: &Palette) {
    let grid = icon_grid();
    let cell_w = rect.width() / ICON_SIZE as f32;
    let cell_h = rect.height() / ICON_SIZE as f32;
    let outline = if p.dark { p.text } else { p.card_border };
    let panel = if p.dark { p.card_solid } else { p.bg_glow };
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let color = match grid[y][x] {
                IconPixel::Transparent => continue,
                IconPixel::Ink => outline,
                IconPixel::Panel => panel,
                IconPixel::Cell(_) => p.accent,
            };
            let cell = Rect::from_min_max(
                Pos2::new(
                    rect.left() + x as f32 * cell_w,
                    rect.top() + y as f32 * cell_h,
                ),
                Pos2::new(
                    rect.left() + (x + 1) as f32 * cell_w,
                    rect.top() + (y + 1) as f32 * cell_h,
                ),
            );
            painter.rect_filled(
                theme::snap_rect(painter.ctx().pixels_per_point(), cell),
                egui::CornerRadius::ZERO,
                color,
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * size + x) * 4) as usize;
        rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn icon_map_is_a_sixteen_pixel_grid() {
        let grid = icon_grid();
        assert_eq!(grid.len(), ICON_SIZE);
        assert!(grid.iter().all(|row| row.len() == ICON_SIZE));
        assert_eq!(grid[0][0], IconPixel::Transparent);
        assert_eq!(grid[3][2], IconPixel::Ink);
        assert_eq!(grid[4][3], IconPixel::Panel);
        assert_eq!(grid[5][3], IconPixel::Cell(1));
        assert_eq!(grid[5][9], IconPixel::Cell(3));
        assert_eq!(grid[7][12], IconPixel::Ink, "positive terminal");
    }

    #[test]
    fn battery_rgba_uses_the_same_map_and_palette() {
        let rgba = battery_rgba(16, Some(100), false);
        assert_eq!(pixel(&rgba, 16, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&rgba, 16, 2, 3), ICON_INK);
        assert_eq!(pixel(&rgba, 16, 3, 4), ICON_PANEL);
        assert_eq!(pixel(&rgba, 16, 3, 5), [79, 138, 91, 255]);
        assert_eq!(pixel(&rgba, 16, 9, 10), [79, 138, 91, 255]);

        let scaled = battery_rgba(64, Some(100), false);
        assert_eq!(scaled.len(), 64 * 64 * 4);
        assert_eq!(pixel(&scaled, 64, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&scaled, 64, 8, 12), ICON_INK);
        assert_eq!(pixel(&scaled, 64, 12, 16), ICON_PANEL);
    }

    #[test]
    fn tray_cells_use_the_card_tier_helper_at_every_boundary() {
        let palette = theme::palette(false);
        for level in [1u8, 10, 11, 20, 21, 50, 51, 80, 81, 100] {
            let expected = theme::battery_color_for_level(palette, level);
            let rgba = battery_rgba(16, Some(level), false);
            assert_eq!(
                pixel(&rgba, 16, 3, 5),
                [expected.r(), expected.g(), expected.b(), 255],
                "tray fill for {level}%"
            );
        }
    }

    #[test]
    fn level_selects_the_number_of_filled_cells() {
        assert_eq!(filled_cells(None), 0);
        assert_eq!(filled_cells(Some(0)), 0);
        assert_eq!(filled_cells(Some(1)), 1);
        assert_eq!(filled_cells(Some(50)), 2);
        assert_eq!(filled_cells(Some(100)), 3);

        let low = battery_rgba(16, Some(10), false);
        assert_eq!(pixel(&low, 16, 3, 5), [174, 52, 45, 255]);
        assert_eq!(pixel(&low, 16, 6, 5), ICON_EMPTY_CELL);
        assert_eq!(pixel(&low, 16, 9, 5), ICON_EMPTY_CELL);

        // Charging keeps the 21-50 ochre tier instead of replacing it with green.
        let charging = battery_rgba(16, Some(50), true);
        assert_eq!(pixel(&charging, 16, 3, 5), [166, 132, 20, 255]);
        assert_eq!(pixel(&charging, 16, 7, 4), ICON_BOLT, "charging bolt is drawn");
    }
}
