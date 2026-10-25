use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2, vec2,
};

use anybattery_core::{
    age_text, now_unix, BatteryReading, BatteryStatus, DeviceKind, DeviceSnapshot,
};
use crate::icons;
use crate::theme::{self, Palette};

pub const CARD_HEIGHT: f32 = 76.0;

pub fn body(size: f32, color: Color32) -> egui::RichText {
    theme::rich("", size, color, false).size(size)
}

pub fn label(ui: &mut Ui, value: impl Into<String>, size: f32, color: Color32, bold: bool) {
    ui.label(theme::rich(value, size, color, bold));
}

pub fn section_title(ui: &mut Ui, p: &Palette, value: &str) {
    ui.add_space(4.0);
    ui.label(theme::rich(value, 11.5, p.tertiary, true));
    ui.add_space(2.0);
}

pub fn device_card(ui: &mut Ui, p: &Palette, device: &DeviceSnapshot, now: u64) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    let painter = ui.painter();

    painter.rect_filled(
        rect.translate(vec2(0.0, 1.5)),
        CornerRadius::same(18),
        p.shadow,
    );
    painter.rect_filled(rect, CornerRadius::same(18), p.card);
    painter.rect_stroke(
        rect,
        CornerRadius::same(18),
        Stroke::new(1.0, p.card_border),
        StrokeKind::Inside,
    );

    let accent = icons::device_accent(device.kind, p);
    let icon_center = Pos2::new(rect.left() + 34.0, rect.center().y);
    painter.circle_filled(icon_center, 19.0, accent.gamma_multiply(0.12));
    icons::draw_device_icon(
        painter,
        Rect::from_center_size(icon_center, vec2(22.0, 22.0)),
        device.kind,
        accent,
    );

    let left = rect.left() + 62.0;
    let name = truncate(&device.name, 15);
    painter.text(
        Pos2::new(left, rect.top() + 27.0),
        Align2::LEFT_CENTER,
        name,
        FontId::new(14.5, egui::FontFamily::Name("bold".into())),
        p.text,
    );

    let detail = device_detail(device, now);
    painter.text(
        Pos2::new(left, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        detail,
        FontId::new(11.0, egui::FontFamily::Proportional),
        p.secondary,
    );

    let right = rect.right() - 18.0;
    match device.battery {
        BatteryReading::Percent { level, charging } => {
            let color = theme::battery_color(*p, level, charging == Some(true));
            painter.text(
                Pos2::new(right, rect.top() + 25.0),
                Align2::RIGHT_CENTER,
                format!("{}%", level),
                FontId::new(19.0, egui::FontFamily::Name("bold".into())),
                p.text,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - 58.0, rect.top() + 46.0),
                vec2(58.0, 6.0),
            );
            battery_bar(painter, p, bar, level, color);
        }
        BatteryReading::Coarse { level } => {
            painter.text(
                Pos2::new(right, rect.top() + 27.0),
                Align2::RIGHT_CENTER,
                level.label_zh(),
                FontId::new(15.0, egui::FontFamily::Name("bold".into())),
                p.secondary,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - 58.0, rect.top() + 46.0),
                vec2(58.0, 6.0),
            );
            let filled = match level {
                anybattery_core::CoarseLevel::Low => 1,
                anybattery_core::CoarseLevel::Medium => 2,
                anybattery_core::CoarseLevel::Full => 3,
            };
            coarse_bar(painter, p, bar, filled);
        }
        BatteryReading::Unknown => {
            painter.text(
                Pos2::new(right, rect.top() + 26.0),
                Align2::RIGHT_CENTER,
                "—",
                FontId::new(19.0, egui::FontFamily::Name("bold".into())),
                p.tertiary,
            );
            painter.text(
                Pos2::new(right, rect.top() + 47.0),
                Align2::RIGHT_CENTER,
                if device.status == BatteryStatus::Unsupported { "未识别" } else { "无数据" },
                FontId::new(10.5, egui::FontFamily::Proportional),
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
    ui.label(theme::rich(&device.name, 14.0, p.text, true));
    ui.add_space(2.0);
    ui.label(theme::rich(
        format!(
            "{} · {} · {}",
            device.transport.label_zh(),
            device.status.label_zh(),
            age_text(device.updated_at_unix, now)
        ),
        11.5,
        p.secondary,
        false,
    ));
    ui.add_space(4.0);
    ui.label(theme::rich(
        format!("来源：{}（{}）", device.source.provider, device.source.verification.label_zh()),
        11.0,
        p.secondary,
        false,
    ));
    if !device.source.detail.is_empty() {
        ui.label(theme::rich(&device.source.detail, 10.5, p.tertiary, false));
    }
    if !device.detail.is_empty() {
        ui.label(theme::rich(&device.detail, 10.5, p.tertiary, false));
    }
    if let Some(path) = &device.path {
        ui.add_space(3.0);
        ui.label(theme::rich(truncate(path, 90), 9.5, p.tertiary, false));
    }
}

fn device_detail(device: &DeviceSnapshot, now: u64) -> String {
    let transport = device.transport.label_zh();
    let age = age_text(device.updated_at_unix, now);
    let source = device.source.verification.label_zh();
    match device.status {
        BatteryStatus::Unsupported => {
            if device.transport.is_bluetooth() {
                "蓝牙已连接 · 未找到电量协议".to_owned()
            } else {
                "已连接 · 未找到电量协议".to_owned()
            }
        }
        BatteryStatus::Asleep => format!("{} · {} · 设备休眠", transport, age),
        BatteryStatus::Stale => format!("{} · {} · 可能不准确", transport, age),
        BatteryStatus::Error => format!("{} · 读取失败", transport),
        BatteryStatus::Unknown => format!("{} · 等待数据", transport),
        _ => {
            if device.battery.charging() == Some(true) {
                format!("{} · 充电中 · {}", transport, source)
            } else {
                format!("{} · {} · {}", transport, age, source)
            }
        }
    }
}

fn battery_bar(painter: &egui::Painter, p: &Palette, rect: Rect, level: u8, color: Color32) {
    painter.rect_filled(rect, CornerRadius::same(3), p.track);
    let width = rect.width() * (level.min(100) as f32 / 100.0);
    if width > 0.5 {
        let filled = Rect::from_min_size(rect.min, vec2(width, rect.height()));
        painter.rect_filled(filled, CornerRadius::same(3), color);
    }
}

fn coarse_bar(painter: &egui::Painter, p: &Palette, rect: Rect, filled: u8) {
    let gap = 3.0;
    let segment = (rect.width() - gap * 3.0) / 4.0;
    for i in 0..4 {
        let x = rect.left() + i as f32 * (segment + gap);
        let color = if i < filled { p.secondary } else { p.track };
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, rect.top()), vec2(segment, rect.height())),
            CornerRadius::same(2),
            color,
        );
    }
}

pub enum SegmentResult {
    None,
    Selected(usize),
}

pub fn segment(ui: &mut Ui, p: &Palette, labels: &[&str], selected: usize) -> SegmentResult {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(9), p.track);
    let seg_width = (rect.width() - 4.0) / labels.len() as f32;
    let mut result = SegmentResult::None;
    for (index, label) in labels.iter().enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(rect.left() + 2.0 + index as f32 * seg_width, rect.top() + 2.0),
            vec2(seg_width, rect.height() - 4.0),
        );
        if index == selected {
            painter.rect_filled(
                seg.translate(vec2(0.0, 0.5)),
                CornerRadius::same(7),
                p.shadow,
            );
            painter.rect_filled(seg, CornerRadius::same(7), p.card_solid);
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
            if index == selected { p.text } else { p.secondary },
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
    Close,
    Minimize,
    Zoom,
    Copy,
}

pub fn icon_button(ui: &mut Ui, p: &Palette, glyph: Glyph, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), 15.0, p.hover);
    }
    draw_glyph(ui.painter(), rect, glyph, if response.hovered() { p.text } else { p.secondary });
    response.on_hover_text(tooltip)
}

fn draw_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.6, color);
    match glyph {
        Glyph::Refresh => {
            let radius = 7.0;
            let points: Vec<Pos2> = (0..=22)
                .map(|i| {
                    let a = -0.6 + i as f32 / 21.0 * 5.2;
                    Pos2::new(c.x + a.cos() * radius, c.y + a.sin() * radius)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            painter.line_segment(
                [Pos2::new(c.x + 4.5, c.y - 7.6), Pos2::new(c.x + 7.4, c.y - 4.0)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + 7.4, c.y - 4.0), Pos2::new(c.x + 3.4, c.y - 3.2)],
                stroke,
            );
        }
        Glyph::Settings => {
            for (offset, knob) in [(-5.5f32, -2.0f32), (0.0, 3.0), (5.5, -3.0)] {
                let y = c.y + offset;
                painter.line_segment(
                    [Pos2::new(c.x - 7.0, y), Pos2::new(c.x + 7.0, y)],
                    Stroke::new(1.4, color),
                );
                painter.circle_filled(Pos2::new(c.x + knob, y), 2.2, color);
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
            painter.line_segment(
                [Pos2::new(c.x - 3.0, c.y - 6.0), Pos2::new(c.x - 8.0, c.y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 8.0, c.y), Pos2::new(c.x - 3.0, c.y + 6.0)],
                stroke,
            );
        }
        Glyph::Close => {
            painter.line_segment(
                [Pos2::new(c.x - 4.0, c.y - 4.0), Pos2::new(c.x + 4.0, c.y + 4.0)],
                Stroke::new(1.4, color),
            );
            painter.line_segment(
                [Pos2::new(c.x + 4.0, c.y - 4.0), Pos2::new(c.x - 4.0, c.y + 4.0)],
                Stroke::new(1.4, color),
            );
        }
        Glyph::Minimize => {
            painter.line_segment(
                [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)],
                Stroke::new(1.4, color),
            );
        }
        Glyph::Zoom => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(10.0, 10.0)),
                CornerRadius::same(2),
                Stroke::new(1.3, color),
                StrokeKind::Middle,
            );
        }
        Glyph::Copy => {
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x - 1.5, c.y + 1.5), vec2(9.0, 10.0)),
                CornerRadius::same(2),
                Stroke::new(1.3, color),
                StrokeKind::Middle,
            );
            painter.line_segment(
                [Pos2::new(c.x - 3.0, c.y - 4.5), Pos2::new(c.x + 5.0, c.y - 4.5)],
                Stroke::new(1.3, color),
            );
        }
    }
}

pub fn traffic_lights(ui: &mut Ui, p: &Palette) -> Option<Glyph> {
    let (rect, _) = ui.allocate_exact_size(vec2(70.0, 22.0), Sense::hover());
    let painter = ui.painter();
    let colors = [
        Color32::from_rgb(255, 95, 87),
        Color32::from_rgb(254, 188, 46),
        Color32::from_rgb(40, 200, 64),
    ];
    let mut action = None;
    for (index, color) in colors.iter().enumerate() {
        let center = Pos2::new(rect.left() + 9.0 + index as f32 * 20.0, rect.center().y);
        let hit = Rect::from_center_size(center, vec2(16.0, 16.0));
        let response = ui.interact(hit, Id::new(("traffic", index)), Sense::click());
        let dim = if p.dark { 0.82 } else { 1.0 };
        painter.circle_filled(center, 6.0, color.gamma_multiply(dim));
        if response.hovered() {
            let glyph = match index {
                0 => Glyph::Close,
                1 => Glyph::Minimize,
                _ => Glyph::Zoom,
            };
            draw_glyph(painter, hit, glyph, Color32::from_rgb(60, 50, 20));
            if response.clicked() {
                action = Some(glyph);
            }
        }
    }
    action
}

pub fn toggle(ui: &mut Ui, p: &Palette, value: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(46.0, 28.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool(response.id, *value);
    let track_color = theme::lerp_color(p.control, p.green, t);
    ui.painter().rect_filled(rect, CornerRadius::same(14), track_color);
    let knob_x = rect.left() + 14.0 + t * (rect.width() - 28.0);
    ui.painter().circle_filled(
        Pos2::new(knob_x, rect.center().y),
        12.0,
        Color32::from_rgb(255, 255, 255),
    );
    response.clicked()
}

pub fn settings_row(ui: &mut Ui, p: &Palette, title: &str, detail: &str, control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 54.0), Sense::hover());
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(13.5, egui::FontFamily::Proportional),
        p.text,
    );
    if !detail.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 14.0, rect.top() + 36.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::new(10.5, egui::FontFamily::Proportional),
            p.tertiary,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - 130.0, rect.top() + 13.0),
                Pos2::new(rect.right() - 10.0, rect.bottom() - 13.0),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    control(&mut child);
}

pub fn group(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.card_border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::symmetric(4, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

pub fn pill_button(ui: &mut Ui, p: &Palette, text: &str, selected: bool) -> Response {
    let fill = if selected { p.accent } else { p.control };
    let color = if selected { Color32::WHITE } else { p.text };
    ui.add(
        egui::Button::new(theme::rich(text, 11.5, color, selected))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
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
        Stroke::new(1.0, p.card_border),
    );
    let age = last_scan
        .map(|instant| {
            let secs = instant.elapsed().as_secs();
            if scanning {
                "正在刷新…".to_owned()
            } else if secs < 5 {
                "刚刚刷新".to_owned()
            } else if secs < 60 {
                format!("{} 秒前刷新", secs)
            } else {
                format!("{} 分钟前刷新", secs / 60)
            }
        })
        .unwrap_or_else(|| "等待首次扫描".to_owned());
    painter.text(
        Pos2::new(rect.left() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("每 {} 秒 · {}", poll_secs, age),
        FontId::new(10.5, egui::FontFamily::Proportional),
        p.tertiary,
    );
    let button_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 66.0, rect.center().y - 14.0),
        vec2(66.0, 28.0),
    );
    let response = ui.interact(button_rect, Id::new("footer.diagnostics"), Sense::click());
    painter.rect_filled(button_rect, CornerRadius::same(8), if response.hovered() { p.hover } else { p.control });
    painter.text(
        button_rect.center(),
        Align2::CENTER_CENTER,
        "诊断",
        FontId::new(11.5, egui::FontFamily::Proportional),
        p.secondary,
    );
    response
}
