use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, vec2,
};

use anybattery_core::{
    age_text, BatteryReading, BatteryStatus, DeviceSnapshot, CoarseLevel,
};
use crate::icons;
use crate::theme::{self, Palette};

pub const CARD_HEIGHT: f32 = 78.0;

pub fn section_title(ui: &mut Ui, p: &Palette, value: &str) {
    ui.add_space(2.0);
    ui.label(theme::rich(value, 12.0, p.tertiary, true));
    ui.add_space(2.0);
}

pub fn device_card(ui: &mut Ui, p: &Palette, device: &DeviceSnapshot, now: u64) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    let painter = ui.painter();

    icons::draw_pixel_shadow(painter, rect, p.shadow);
    icons::pixel_frame(painter, rect, p.card, p.card_border);

    let accent = icons::device_accent(device.kind);
    let tile = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 12.0),
        vec2(30.0, 30.0),
    );
    painter.rect_filled(tile, CornerRadius::ZERO, accent.gamma_multiply(0.16));
    painter.rect_stroke(tile, CornerRadius::ZERO, Stroke::new(2.0, accent), StrokeKind::Inside);
    icons::draw_device_icon(
        painter,
        Rect::from_center_size(tile.center(), vec2(20.0, 20.0)),
        device.kind,
        accent,
    );

    let left = rect.left() + 52.0;
    painter.text(
        Pos2::new(left, rect.top() + 26.0),
        Align2::LEFT_CENTER,
        truncate(&device.name, 15),
        FontId::new(12.0, egui::FontFamily::Name("bold".into())),
        p.text,
    );
    painter.text(
        Pos2::new(left, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        truncate(&device_detail(device, now), 24),
        FontId::new(12.0, egui::FontFamily::Proportional),
        p.secondary,
    );

    let right = rect.right() - 12.0;
    match device.battery {
        BatteryReading::Percent { level, charging } => {
            let color = theme::battery_color(*p, level, charging == Some(true));
            painter.text(
                Pos2::new(right, rect.top() + 24.0),
                Align2::RIGHT_CENTER,
                format!("{}%", level),
                FontId::new(24.0, egui::FontFamily::Name("bold".into())),
                p.text,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - 58.0, rect.top() + 48.0),
                vec2(58.0, 12.0),
            );
            pixel_battery(painter, p, bar, level, color);
        }
        BatteryReading::Coarse { level } => {
            painter.text(
                Pos2::new(right, rect.top() + 26.0),
                Align2::RIGHT_CENTER,
                level.label_zh(),
                FontId::new(18.0, egui::FontFamily::Name("bold".into())),
                p.secondary,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - 58.0, rect.top() + 48.0),
                vec2(58.0, 12.0),
            );
            let filled = match level {
                CoarseLevel::Low => 1,
                CoarseLevel::Medium => 2,
                CoarseLevel::Full => 3,
            };
            coarse_pixel_battery(painter, p, bar, filled);
        }
        BatteryReading::Unknown => {
            painter.text(
                Pos2::new(right, rect.top() + 26.0),
                Align2::RIGHT_CENTER,
                "---",
                FontId::new(20.0, egui::FontFamily::Name("bold".into())),
                p.tertiary,
            );
            painter.text(
                Pos2::new(right, rect.top() + 50.0),
                Align2::RIGHT_CENTER,
                "NO DATA",
                FontId::new(12.0, egui::FontFamily::Proportional),
                p.tertiary,
            );
        }
    }

    let device = device.clone();
    response.on_hover_ui(move |ui| {
        device_tooltip(ui, p, &device, now);
    })
}

fn device_tooltip(ui: &mut Ui, p: &Palette, device: &DeviceSnapshot, now: u64) {
    ui.set_max_width(320.0);
    ui.label(theme::rich(&device.name, 12.0, p.text, true));
    ui.add_space(2.0);
    ui.label(theme::rich(
        format!(
            "{} · {} · {}",
            device.transport.label_zh(),
            device.status.label_zh(),
            age_text(device.updated_at_unix, now)
        ),
        12.0,
        p.secondary,
        false,
    ));
    ui.add_space(4.0);
    ui.label(theme::rich(
        format!(
            "来源：{} / {}",
            device.source.provider,
            device.source.verification.label_zh()
        ),
        12.0,
        p.secondary,
        false,
    ));
    if !device.source.detail.is_empty() {
        ui.label(theme::rich(&device.source.detail, 12.0, p.tertiary, false));
    }
    if !device.detail.is_empty() {
        ui.label(theme::rich(&device.detail, 12.0, p.tertiary, false));
    }
    if let Some(path) = &device.path {
        ui.add_space(3.0);
        ui.label(theme::rich(truncate(path, 90), 12.0, p.tertiary, false));
    }
}

fn device_detail(device: &DeviceSnapshot, now: u64) -> String {
    let transport = device.transport.label_zh();
    let age = age_text(device.updated_at_unix, now);
    let source = device.source.verification.label_zh();
    match device.status {
        BatteryStatus::Unsupported => "已连接 / 未匹配电量协议".to_owned(),
        BatteryStatus::Asleep => format!("{} / {} / 休眠", transport, age),
        BatteryStatus::Stale => format!("{} / {} / 可能不准", transport, age),
        BatteryStatus::Error => format!("{} / 读取失败", transport),
        BatteryStatus::Unknown => format!("{} / 等待数据", transport),
        _ => {
            if device.battery.charging() == Some(true) {
                format!("{} / 充电中 / {}", transport, source)
            } else {
                format!("{} / {} / {}", transport, age, source)
            }
        }
    }
}

fn pixel_battery(painter: &egui::Painter, p: &Palette, rect: Rect, level: u8, color: Color32) {
    let cells = 8;
    let gap = 1.5;
    let cell_width = (rect.width() - gap * (cells as f32 - 1.0)) / cells as f32;
    let filled = ((level.min(100) as f32 / 100.0) * cells as f32).ceil() as usize;
    for i in 0..cells {
        let x = rect.left() + i as f32 * (cell_width + gap);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(cell_width, rect.height()));
        let fill = if i < filled { color } else { p.track };
        painter.rect_filled(cell, CornerRadius::ZERO, fill);
        painter.rect_stroke(cell, CornerRadius::ZERO, Stroke::new(1.0, p.card_border), StrokeKind::Inside);
    }
}

fn coarse_pixel_battery(painter: &egui::Painter, p: &Palette, rect: Rect, filled: usize) {
    let cells = 4;
    let gap = 2.0;
    let cell_width = (rect.width() - gap * 3.0) / cells as f32;
    for i in 0..cells {
        let x = rect.left() + i as f32 * (cell_width + gap);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(cell_width, rect.height()));
        painter.rect_filled(
            cell,
            CornerRadius::ZERO,
            if i < filled { p.secondary } else { p.track },
        );
        painter.rect_stroke(cell, CornerRadius::ZERO, Stroke::new(1.0, p.card_border), StrokeKind::Inside);
    }
}

pub enum SegmentResult {
    None,
    Selected(usize),
}

pub fn segment(ui: &mut Ui, p: &Palette, labels: &[&str], selected: usize) -> SegmentResult {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let painter = ui.painter();
    icons::draw_pixel_shadow(painter, rect, p.shadow);
    icons::pixel_frame(painter, rect, p.control, p.card_border);
    let seg_width = (rect.width() - 4.0) / labels.len() as f32;
    let mut result = SegmentResult::None;
    for (index, label) in labels.iter().enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(rect.left() + 2.0 + index as f32 * seg_width, rect.top() + 2.0),
            vec2(seg_width, rect.height() - 4.0),
        );
        if index == selected {
            painter.rect_filled(seg, CornerRadius::ZERO, p.accent);
        }
        painter.text(
            seg.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::new(
                12.0,
                if index == selected {
                    egui::FontFamily::Name("bold".into())
                } else {
                    egui::FontFamily::Proportional
                },
            ),
            if index == selected { p.card_solid } else { p.secondary },
        );
        let response = ui.interact(seg, Id::new(("segment", label, index)), Sense::click());
        if response.clicked() && index != selected {
            result = SegmentResult::Selected(index);
        }
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Refresh,
    Settings,
    Diagnostics,
    Back,
    Copy,
}

pub fn icon_button(ui: &mut Ui, p: &Palette, glyph: Glyph, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::click());
    if response.hovered() {
        icons::pixel_frame(ui.painter(), rect, p.hover, p.card_border);
    }
    draw_glyph(ui.painter(), rect, glyph, if response.hovered() { p.text } else { p.secondary });
    response.on_hover_text(tooltip)
}

fn draw_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(2.0, color);
    match glyph {
        Glyph::Refresh => {
            painter.line_segment([Pos2::new(c.x - 6.0, c.y - 3.0), Pos2::new(c.x - 6.0, c.y + 3.0)], stroke);
            painter.line_segment([Pos2::new(c.x - 6.0, c.y - 3.0), Pos2::new(c.x + 5.0, c.y - 3.0)], stroke);
            painter.line_segment([Pos2::new(c.x + 5.0, c.y - 3.0), Pos2::new(c.x + 5.0, c.y + 3.0)], stroke);
            painter.line_segment([Pos2::new(c.x + 5.0, c.y + 3.0), Pos2::new(c.x - 6.0, c.y + 3.0)], stroke);
            painter.line_segment([Pos2::new(c.x + 1.0, c.y), Pos2::new(c.x + 6.0, c.y)], stroke);
        }
        Glyph::Settings => {
            for (offset, knob) in [(-5.0f32, -2.0f32), (0.0, 3.0), (5.0, -3.0)] {
                let y = c.y + offset;
                painter.line_segment([Pos2::new(c.x - 7.0, y), Pos2::new(c.x + 7.0, y)], Stroke::new(2.0, color));
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(c.x + knob, y), vec2(4.0, 6.0)),
                    CornerRadius::ZERO,
                    color,
                );
            }
        }
        Glyph::Diagnostics => {
            let points = vec![
                Pos2::new(c.x - 8.0, c.y),
                Pos2::new(c.x - 4.0, c.y),
                Pos2::new(c.x - 2.0, c.y - 5.0),
                Pos2::new(c.x + 1.0, c.y + 5.0),
                Pos2::new(c.x + 4.0, c.y),
                Pos2::new(c.x + 8.0, c.y),
            ];
            painter.add(egui::Shape::line(points, stroke));
        }
        Glyph::Back => {
            painter.line_segment([Pos2::new(c.x - 2.0, c.y - 6.0), Pos2::new(c.x - 8.0, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x - 8.0, c.y), Pos2::new(c.x - 2.0, c.y + 6.0)], stroke);
        }
        Glyph::Copy => {
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x - 2.0, c.y + 2.0), vec2(9.0, 10.0)),
                CornerRadius::ZERO,
                Stroke::new(2.0, color),
                StrokeKind::Middle,
            );
            painter.line_segment([Pos2::new(c.x - 3.0, c.y - 4.0), Pos2::new(c.x + 5.0, c.y - 4.0)], stroke);
            painter.line_segment([Pos2::new(c.x + 5.0, c.y - 4.0), Pos2::new(c.x + 5.0, c.y + 1.0)], stroke);
        }
    }
}

pub fn toggle(ui: &mut Ui, p: &Palette, value: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool(response.id, *value);
    let track_color = theme::lerp_color(p.control, p.green, t);
    icons::pixel_frame(ui.painter(), rect, track_color, p.card_border);
    let knob = Rect::from_min_size(
        Pos2::new(rect.left() + 3.0 + t * (rect.width() - 22.0), rect.top() + 3.0),
        vec2(16.0, 16.0),
    );
    ui.painter().rect_filled(knob, CornerRadius::ZERO, p.card_solid);
    ui.painter().rect_stroke(knob, CornerRadius::ZERO, Stroke::new(2.0, p.card_border), StrokeKind::Inside);
    response.clicked()
}

pub fn settings_row(ui: &mut Ui, p: &Palette, title: &str, detail: &str, control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(12.0, egui::FontFamily::Proportional),
        p.text,
    );
    if !detail.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 12.0, rect.top() + 36.0),
            Align2::LEFT_CENTER,
            truncate(detail, 30),
            FontId::new(12.0, egui::FontFamily::Proportional),
            p.tertiary,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - 120.0, rect.top() + 14.0),
                Pos2::new(rect.right() - 8.0, rect.bottom() - 14.0),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    control(&mut child);
}

pub fn group(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(2.0, p.card_border))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(egui::Margin::symmetric(4, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

pub fn pill_button(ui: &mut Ui, p: &Palette, text: &str, selected: bool) -> Response {
    let fill = if selected { p.accent } else { p.control };
    let color = if selected { p.card_solid } else { p.text };
    ui.add(
        egui::Button::new(theme::rich(text, 12.0, color, selected))
            .fill(fill)
            .stroke(Stroke::new(2.0, p.card_border))
            .corner_radius(CornerRadius::ZERO)
            .min_size(vec2(0.0, 28.0)),
    )
}

pub fn truncate(value: &str, limit: usize) -> String {
    let count = value.chars().count();
    if count <= limit {
        value.to_owned()
    } else {
        let mut out: String = value.chars().take(limit.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn footer(ui: &mut Ui, p: &Palette, poll_secs: u64, last_scan: Option<std::time::Instant>, scanning: bool) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 44.0), Sense::hover());
    let painter = ui.painter();
    painter.line_segment(
        [Pos2::new(rect.left(), rect.top() + 1.0), Pos2::new(rect.right(), rect.top() + 1.0)],
        Stroke::new(2.0, p.card_border),
    );
    let age = last_scan
        .map(|instant| {
            let secs = instant.elapsed().as_secs();
            if scanning {
                "SCANNING".to_owned()
            } else if secs < 5 {
                "刚刚刷新".to_owned()
            } else if secs < 60 {
                format!("{} 秒前", secs)
            } else {
                format!("{} 分钟前", secs / 60)
            }
        })
        .unwrap_or_else(|| "等待首次扫描".to_owned());
    painter.text(
        Pos2::new(rect.left() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("每 {} 秒 / {}", poll_secs, age),
        FontId::new(12.0, egui::FontFamily::Proportional),
        p.tertiary,
    );
    let button_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 62.0, rect.center().y - 13.0),
        vec2(62.0, 26.0),
    );
    let response = ui.interact(button_rect, Id::new("footer.diagnostics"), Sense::click());
    icons::pixel_frame(
        painter,
        button_rect,
        if response.hovered() { p.hover } else { p.control },
        p.card_border,
    );
    painter.text(
        button_rect.center(),
        Align2::CENTER_CENTER,
        "诊断",
        FontId::new(12.0, egui::FontFamily::Proportional),
        p.secondary,
    );
    response
}
