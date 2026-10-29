use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Layout, Pos2,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2,
};
use std::time::{Duration, Instant};

use crate::icons;
use crate::theme::{self, Palette};
use anybattery_core::{BatteryReading, BatteryStatus, CoarseLevel, DeviceSnapshot, age_text};

pub const CARD_HEIGHT: f32 = 78.0;

/// Animations stay short: feedback, never decoration.
const ANIMATION_SECS: f32 = 0.15;
/// Refresh spinner step while scanning, and the repaint cadence that drives it.
const SCAN_REPAINT_SECS: f32 = 0.2;

/// Animation time used by this crate's custom transitions.
/// Zero when the user turned animations off, so every value jumps straight to its target.
fn animation_secs(ui: &Ui) -> f32 {
    if ui.style().animation_time > 0.0 {
        ANIMATION_SECS
    } else {
        0.0
    }
}

/// Shortest text that still fits `max_width`, measured with the real font at `size`.
///
/// Used for every long label so fixed character counts can never let mixed CJK/Latin text
/// spill into a neighbouring column.
pub fn fit_text(ui: &Ui, value: &str, size: f32, bold: bool, max_width: f32) -> String {
    let family = if bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    let font = FontId::new(size, family);
    let width = |text: &str| {
        ui.ctx().fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    if !max_width.is_finite() || max_width < 4.0 {
        return String::new();
    }
    if width(value) <= max_width {
        return value.to_owned();
    }
    let ellipsis = "…";
    let budget = max_width - width(ellipsis);
    let mut chars: Vec<char> = value.chars().collect();
    while !chars.is_empty() && width(&chars.iter().collect::<String>()) > budget {
        chars.pop();
    }
    let mut out: String = chars.into_iter().collect();
    out.push_str(ellipsis);
    out
}

/// Character-count shortening for non-layout strings (tray tooltips). Areas that must not
/// overlap a neighbour use [`fit_text`], which measures the real font.
pub fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_owned()
    } else {
        let mut out: String = value.chars().take(limit.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn section_title(ui: &mut Ui, p: &Palette, value: &str) {
    ui.add_space(2.0);
    ui.label(theme::rich(value, 12.0, p.tertiary, true));
    ui.add_space(2.0);
}

/// 32px 设备图标按钮，用于手动指定设备类型。图标、底色和边框与设备卡片一致。
pub fn device_icon_button(
    ui: &mut Ui,
    p: &Palette,
    kind: anybattery_core::DeviceKind,
    selected: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let fill = if selected { p.accent } else { p.control };
    let border = if selected { p.accent } else { p.card_border };
    let color = if selected {
        p.on_accent
    } else {
        icons::device_accent(kind)
    };
    icons::pixel_frame(ui.painter(), rect, fill, border);
    icons::draw_device_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
        kind,
        color,
    );
    response.on_hover_text(kind.label_zh())
}

/// 24px 只读图标块，用于设置页展示设备当前图标。
pub fn device_icon_tile(ui: &mut Ui, p: &Palette, kind: anybattery_core::DeviceKind) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    icons::pixel_frame(
        ui.painter(),
        rect,
        icons::device_accent(kind),
        p.card_border,
    );
    icons::draw_device_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        kind,
        p.on_accent,
    );
    response
}

pub fn device_card(ui: &mut Ui, p: &Palette, device: &DeviceSnapshot, now: u64) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    let painter = ui.painter();

    icons::draw_pixel_shadow(painter, rect, p.shadow);
    let fill = theme::lerp_color(p.card, p.hover, hover);
    icons::pixel_frame(painter, rect, fill, p.card_border);

    let accent = icons::device_accent(device.kind);
    let tile = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.top() + 12.0),
        vec2(32.0, 32.0),
    );
    icons::pixel_frame(painter, tile, accent, p.card_border);
    icons::draw_device_icon(
        painter,
        Rect::from_center_size(tile.center(), vec2(20.0, 20.0)),
        device.kind,
        p.on_accent,
    );

    // Right column: the reading. Its width is measured once and shared by all rows.
    let right = rect.right() - 12.0;
    let reading_width = match device.battery {
        BatteryReading::Percent { level, .. } => percent_text_width(ui, level).max(BAR_WIDTH),
        BatteryReading::Coarse { level } => ui
            .painter()
            .layout_no_wrap(
                level.label_zh().to_owned(),
                FontId::new(18.0, FontFamily::Name("bold".into())),
                p.secondary,
            )
            .size()
            .x
            .max(BAR_WIDTH),
        BatteryReading::Unknown => BAR_WIDTH,
    };
    let left = rect.left() + 56.0;
    let text_width = (right - reading_width - 10.0 - left).max(16.0);

    let title_font = FontId::new(12.0, FontFamily::Name("bold".into()));
    painter.text(
        Pos2::new(left, rect.top() + 26.0),
        Align2::LEFT_CENTER,
        fit_text(ui, &device.name, 12.0, true, text_width),
        title_font.clone(),
        p.text,
    );
    // States that are easy to confuse with a plain reading get a small text tag that does not
    // rely on colour alone.
    if let Some(label) = status_badge(device.status) {
        let font = FontId::new(10.0, FontFamily::Proportional);
        let badge = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0, rect.top() + 50.0),
            vec2(30.0, 14.0),
        );
        painter.rect_filled(badge, CornerRadius::ZERO, p.control);
        painter.rect_stroke(
            badge,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.card_border),
            StrokeKind::Inside,
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            label,
            font,
            p.secondary,
        );
    }
    painter.text(
        Pos2::new(left, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        fit_text(ui, &device_detail(device, now), 12.0, false, text_width),
        FontId::new(12.0, FontFamily::Proportional),
        p.secondary,
    );

    // The percentage label is the true target value; only the cell fill eases.
    match device.battery {
        BatteryReading::Percent { level, .. } => {
            let color = theme::battery_color_for_level(*p, level);
            painter.text(
                Pos2::new(right, rect.top() + 24.0),
                Align2::RIGHT_CENTER,
                format!("{}%", level),
                FontId::new(24.0, FontFamily::Name("bold".into())),
                p.text,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - BAR_WIDTH, rect.top() + 48.0),
                vec2(BAR_WIDTH, 12.0),
            );
            let target = level.min(100) as f32 / 100.0;
            // Keyed by device, not list position, so sorting or filtering cannot hand one
            // device's animation state to another.
            let fill_id = Id::new(("device-percent-fill", device.id.0.as_str()));
            let shown = ui
                .ctx()
                .animate_value_with_time(fill_id, target, animation_secs(ui));
            pixel_battery(painter, p, bar, shown, color);
        }
        BatteryReading::Coarse { level } => {
            painter.text(
                Pos2::new(right, rect.top() + 26.0),
                Align2::RIGHT_CENTER,
                level.label_zh(),
                FontId::new(18.0, FontFamily::Name("bold".into())),
                p.secondary,
            );
            let bar = Rect::from_min_size(
                Pos2::new(right - BAR_WIDTH, rect.top() + 48.0),
                vec2(BAR_WIDTH, 12.0),
            );
            coarse_pixel_battery(painter, p, bar, level);
        }
        BatteryReading::Unknown => {
            painter.text(
                Pos2::new(right, rect.top() + 26.0),
                Align2::RIGHT_CENTER,
                "---",
                FontId::new(20.0, FontFamily::Name("bold".into())),
                p.tertiary,
            );
            painter.text(
                Pos2::new(right, rect.top() + 50.0),
                Align2::RIGHT_CENTER,
                "无电量",
                FontId::new(12.0, FontFamily::Proportional),
                p.tertiary,
            );
        }
    }

    let device = device.clone();
    response.on_hover_ui(move |ui| {
        device_tooltip(ui, p, &device, now);
    })
}

const BAR_WIDTH: f32 = 58.0;

/// Width of the largest three-digit percentage at the reading size, so a 100% label
/// never jumps the column wider while a device changes.
fn percent_text_width(ui: &Ui, level: u8) -> f32 {
    let sample = if level >= 100 { "100%" } else { "88%" };
    ui.painter()
        .layout_no_wrap(
            sample.to_owned(),
            FontId::new(24.0, FontFamily::Name("bold".into())),
            Color32::WHITE,
        )
        .size()
        .x
        .ceil()
}

/// Non-colour status label for states a reader must be able to tell apart.
fn status_badge(status: BatteryStatus) -> Option<&'static str> {
    match status {
        BatteryStatus::Asleep => Some("休眠"),
        BatteryStatus::Stale => Some("过期"),
        BatteryStatus::Error => Some("错误"),
        BatteryStatus::Unsupported | BatteryStatus::Unknown => Some("未知"),
        BatteryStatus::Available | BatteryStatus::Charging | BatteryStatus::Full => None,
    }
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
        ui.label(RichText::new(path).size(12.0).color(p.tertiary).monospace());
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

pub(crate) fn pixel_battery(
    painter: &egui::Painter,
    p: &Palette,
    rect: Rect,
    fill: f32,
    color: Color32,
) {
    let cells = 8;
    let gap = 1.5;
    let cell_width = (rect.width() - gap * (cells as f32 - 1.0)) / cells as f32;
    let filled = (fill.clamp(0.0, 1.0) * cells as f32).ceil() as usize;
    for i in 0..cells {
        let x = rect.left() + i as f32 * (cell_width + gap);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(cell_width, rect.height()));
        let cell = theme::snap_rect(painter.ctx().pixels_per_point(), cell);
        let fill = if i < filled { color } else { p.track };
        painter.rect_filled(cell, CornerRadius::ZERO, fill);
        painter.rect_stroke(
            cell,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.card_border),
            StrokeKind::Inside,
        );
    }
}

/// Coarse reading: three segments meaning low / medium / full, matching the protocol's
/// three levels exactly. No percentage is invented from them.
pub(crate) fn coarse_pixel_battery(
    painter: &egui::Painter,
    p: &Palette,
    rect: Rect,
    level: CoarseLevel,
) {
    let cells = 3;
    let gap = 2.0;
    let filled = match level {
        CoarseLevel::Low => 1,
        CoarseLevel::Medium => 2,
        CoarseLevel::Full => 3,
    };
    let cell_width = (rect.width() - gap * (cells as f32 - 1.0)) / cells as f32;
    for i in 0..cells {
        let x = rect.left() + i as f32 * (cell_width + gap);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(cell_width, rect.height()));
        let cell = theme::snap_rect(painter.ctx().pixels_per_point(), cell);
        painter.rect_filled(
            cell,
            CornerRadius::ZERO,
            if i < filled { p.secondary } else { p.track },
        );
        painter.rect_stroke(
            cell,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.card_border),
            StrokeKind::Inside,
        );
    }
}

pub enum SegmentResult {
    None,
    Selected(usize),
}

pub fn segment(ui: &mut Ui, p: &Palette, labels: &[&str], selected: usize) -> SegmentResult {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let painter = ui.painter();
    icons::draw_pixel_shadow(painter, rect, p.shadow);
    icons::pixel_frame(painter, rect, p.control, p.card_border);
    let seg_width = (rect.width() - 4.0) / labels.len() as f32;
    let marker_id = ui.id().with("segment-indicator");
    let shown = ui
        .ctx()
        .animate_value_with_time(marker_id, selected as f32, animation_secs(ui));
    let marker = Rect::from_min_size(
        Pos2::new(rect.left() + 2.0 + shown * seg_width, rect.top() + 2.0),
        vec2(seg_width, rect.height() - 4.0),
    );
    painter.rect_filled(
        theme::snap_rect(ui.ctx().pixels_per_point(), marker),
        CornerRadius::ZERO,
        p.accent,
    );
    let mut result = SegmentResult::None;
    for (index, label) in labels.iter().enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(
                rect.left() + 2.0 + index as f32 * seg_width,
                rect.top() + 2.0,
            ),
            vec2(seg_width, rect.height() - 4.0),
        );
        let response = ui.interact(seg, Id::new(("segment", label, index)), Sense::click());
        let on_accent = index == selected;
        let hover = if on_accent {
            1.0
        } else {
            ui.ctx()
                .animate_bool(response.id.with("hover"), response.hovered())
        };
        painter.text(
            theme::snap_rect(ui.ctx().pixels_per_point(), seg).center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::new(
                12.0,
                if on_accent {
                    FontFamily::Name("bold".into())
                } else {
                    FontFamily::Proportional
                },
            ),
            if on_accent {
                p.on_accent
            } else {
                theme::lerp_color(p.secondary, p.text, hover)
            },
        );
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
    Back,
    Minimize,
    Maximize,
    Restore,
    Close,
}

pub const TITLEBAR_HEIGHT: f32 = 36.0;
pub const CAPTION_BUTTON_SIZE: f32 = 30.0;
const CAPTION_BUTTON_GAP: f32 = 2.0;
const CAPTION_RIGHT_MARGIN: f32 = 6.0;

#[derive(Clone, Copy)]
pub struct CaptionButtonLayout {
    pub minimize: Rect,
    pub maximize: Rect,
    pub close: Rect,
}

impl CaptionButtonLayout {
    pub fn right_edge(self) -> f32 {
        self.minimize.left()
    }
}

/// Square caption buttons, laid out from the right edge of the custom title bar.
pub fn caption_button_layout(bar: Rect) -> CaptionButtonLayout {
    let top = bar.center().y - CAPTION_BUTTON_SIZE / 2.0;
    let size = vec2(CAPTION_BUTTON_SIZE, CAPTION_BUTTON_SIZE);
    let close_left = bar.right() - CAPTION_RIGHT_MARGIN - CAPTION_BUTTON_SIZE;
    let maximize_left = close_left - CAPTION_BUTTON_GAP - CAPTION_BUTTON_SIZE;
    let minimize_left = maximize_left - CAPTION_BUTTON_GAP - CAPTION_BUTTON_SIZE;
    CaptionButtonLayout {
        minimize: Rect::from_min_size(Pos2::new(minimize_left, top), size),
        maximize: Rect::from_min_size(Pos2::new(maximize_left, top), size),
        close: Rect::from_min_size(Pos2::new(close_left, top), size),
    }
}

/// Draggable area of the title bar. It stops short of the caption cluster so pressing a
/// caption button can never start a window move on the same frame.
pub fn titlebar_drag_rect(bar: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(bar.left() + 4.0, bar.top() + 4.0),
        Pos2::new(
            caption_button_layout(bar).right_edge() - 6.0,
            bar.bottom() - 2.0,
        ),
    )
}

/// A caption button at an explicit rectangle. Kept separate from [`icon_button`] because the
/// title bar owns its own row and the close button needs a destructive hover colour.
pub fn caption_button(
    ui: &mut Ui,
    p: &Palette,
    rect: Rect,
    glyph: Glyph,
    tooltip: &str,
    destructive: bool,
) -> Response {
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let response = ui.interact(rect, Id::new(("caption", tooltip)), Sense::click());
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    let pressed = response.is_pointer_button_down_on();
    let hover_fill = if destructive {
        p.red.gamma_multiply(0.85)
    } else {
        p.hover
    };
    let fill = if pressed {
        Some(if destructive { p.red } else { p.accent })
    } else if hover > 0.0 {
        Some(theme::lerp_color(Color32::TRANSPARENT, hover_fill, hover))
    } else {
        None
    };
    if let Some(fill) = fill {
        icons::pixel_frame(ui.painter(), rect, fill, p.card_border);
    }
    let color = if pressed || (destructive && hover > 0.25) {
        p.on_accent
    } else {
        theme::lerp_color(p.secondary, p.text, hover)
    };
    draw_glyph(ui.painter(), rect, glyph, color);
    response
        .on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut Ui, p: &Palette, glyph: Glyph, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::click());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    if hover > 0.0 {
        icons::pixel_frame(
            ui.painter(),
            rect,
            p.hover.gamma_multiply(hover),
            p.card_border,
        );
    }
    draw_glyph(
        ui.painter(),
        rect,
        glyph,
        theme::lerp_color(
            p.secondary,
            p.text,
            hover.max(if response.is_pointer_button_down_on() {
                1.0
            } else {
                0.0
            }),
        ),
    );
    response.on_hover_text(tooltip)
}

/// Refresh control: the glyph steps while a scan is running and settles as soon as it stops.
pub fn refresh_button(ui: &mut Ui, p: &Palette, scanning: bool, animations: bool) -> Response {
    // While scanning the control is a status indicator, not a button: no click sense and no
    // pointing-hand cursor, so it cannot promise an action that intentionally does nothing.
    let sense = if scanning {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 28.0), sense);
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let hover = ui
        .ctx()
        .animate_bool(response.id.with("hover"), response.hovered());
    if hover > 0.0 {
        icons::pixel_frame(
            ui.painter(),
            rect,
            p.hover.gamma_multiply(hover),
            p.card_border,
        );
    }
    let color = if scanning {
        p.accent
    } else {
        theme::lerp_color(p.secondary, p.text, hover)
    };
    let painter = ui.painter();
    if scanning {
        // Four-step rotation around the frame; one step per 200 ms, so idle cost is bounded
        // and disabling animations drops straight to the static glyph.
        let step = if animations {
            let t = ui.input(|i| i.time);
            let step = ((t / SCAN_REPAINT_SECS as f64) as i64).rem_euclid(4);
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(SCAN_REPAINT_SECS));
            step
        } else {
            0
        };
        let c = rect.center();
        let r = 5.0;
        let corners = [
            Pos2::new(c.x - r, c.y - r),
            Pos2::new(c.x + r, c.y - r),
            Pos2::new(c.x + r, c.y + r),
            Pos2::new(c.x - r, c.y + r),
        ];
        let start = corners[step as usize];
        let mid = corners[((step + 1) % 4) as usize];
        painter.line_segment([start, mid], Stroke::new(2.0, color));
        painter.line_segment(
            [start, corners[((step + 3) % 4) as usize]],
            Stroke::new(2.0, color.gamma_multiply(0.45)),
        );
    } else {
        draw_glyph(painter, rect, Glyph::Refresh, color);
    }
    let response = response.on_hover_text(if scanning {
        "正在扫描…"
    } else {
        "立即刷新"
    });
    if scanning {
        response
    } else {
        response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

fn draw_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(2.0, color);
    match glyph {
        Glyph::Refresh => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 6.0, c.y - 3.0),
                    Pos2::new(c.x - 6.0, c.y + 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 6.0, c.y - 3.0),
                    Pos2::new(c.x + 5.0, c.y - 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 5.0, c.y - 3.0),
                    Pos2::new(c.x + 5.0, c.y + 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 5.0, c.y + 3.0),
                    Pos2::new(c.x - 6.0, c.y + 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + 1.0, c.y), Pos2::new(c.x + 6.0, c.y)],
                stroke,
            );
        }
        Glyph::Settings => {
            for (offset, knob) in [(-5.0f32, -2.0f32), (0.0, 3.0), (5.0, -3.0)] {
                let y = c.y + offset;
                painter.line_segment(
                    [Pos2::new(c.x - 7.0, y), Pos2::new(c.x + 7.0, y)],
                    Stroke::new(2.0, color),
                );
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(c.x + knob, y), vec2(4.0, 6.0)),
                    CornerRadius::ZERO,
                    color,
                );
            }
        }
        Glyph::Back => {
            painter.line_segment(
                [Pos2::new(c.x - 2.0, c.y - 6.0), Pos2::new(c.x - 8.0, c.y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 8.0, c.y), Pos2::new(c.x - 2.0, c.y + 6.0)],
                stroke,
            );
        }
        Glyph::Minimize => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 5.0, c.y + 4.0),
                    Pos2::new(c.x + 5.0, c.y + 4.0),
                ],
                stroke,
            );
        }
        Glyph::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(12.0, 12.0)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Inside,
            );
        }
        Glyph::Restore => {
            painter.rect_stroke(
                Rect::from_center_size(c + vec2(-1.5, -1.5), vec2(10.0, 10.0)),
                CornerRadius::ZERO,
                Stroke::new(1.5, color),
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_center_size(c + vec2(1.5, 1.5), vec2(10.0, 10.0)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Inside,
            );
        }
        Glyph::Close => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 5.0, c.y - 5.0),
                    Pos2::new(c.x + 5.0, c.y + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 5.0, c.y - 5.0),
                    Pos2::new(c.x - 5.0, c.y + 5.0),
                ],
                stroke,
            );
        }
    }
}

pub fn toggle(ui: &mut Ui, p: &Palette, value: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::click());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool(response.id, *value);
    let track_color = theme::lerp_color(p.control, p.green, t);
    icons::pixel_frame(ui.painter(), rect, track_color, p.card_border);
    let knob = Rect::from_min_size(
        Pos2::new(
            rect.left() + 3.0 + t * (rect.width() - 22.0),
            rect.top() + 3.0,
        ),
        vec2(16.0, 16.0),
    );
    ui.painter().rect_filled(
        theme::snap_rect(ui.ctx().pixels_per_point(), knob),
        CornerRadius::ZERO,
        p.card_solid,
    );
    ui.painter().rect_stroke(
        knob,
        CornerRadius::ZERO,
        Stroke::new(2.0, p.card_border),
        StrokeKind::Inside,
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

pub fn settings_row(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    detail: &str,
    control: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
    // Control column is reserved first; the text column gets whatever is left.
    let control_width = 120.0;
    let text_width = (rect.width() - 12.0 - control_width - 10.0).max(40.0);
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        fit_text(ui, title, 12.0, false, text_width),
        FontId::new(12.0, FontFamily::Proportional),
        p.text,
    );
    if !detail.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 12.0, rect.top() + 36.0),
            Align2::LEFT_CENTER,
            fit_text(ui, detail, 12.0, false, text_width),
            FontId::new(12.0, FontFamily::Proportional),
            p.tertiary,
        );
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - control_width, rect.top() + 14.0),
                Pos2::new(rect.right() - 8.0, rect.bottom() - 14.0),
            ))
            .layout(Layout::right_to_left(Align::Center)),
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
    pill_button_enabled(ui, p, text, selected, true)
}

/// Pill button with an explicit disabled state for actions that are intentionally inert while
/// a scan is running. Disabled buttons keep their muted text and the normal arrow cursor.
pub fn pill_button_enabled(
    ui: &mut Ui,
    p: &Palette,
    text: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    let fill = if selected && enabled {
        p.accent
    } else {
        p.control
    };
    let color = if !enabled {
        p.tertiary
    } else if selected {
        p.on_accent
    } else {
        p.text
    };
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(theme::rich(text, 12.0, color, selected))
            .fill(fill)
            .stroke(Stroke::new(2.0, p.card_border))
            .corner_radius(CornerRadius::ZERO)
            .min_size(vec2(0.0, 28.0)),
    );
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Footer: one natural-reading line for the interval and the freshness of the last scan.
pub fn footer(
    ui: &mut Ui,
    p: &Palette,
    poll_secs: u64,
    last_scan: Option<Instant>,
    scanning: bool,
) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let painter = ui.painter();
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.top() + 1.0),
            Pos2::new(rect.right(), rect.top() + 1.0),
        ],
        Stroke::new(2.0, p.card_border),
    );
    let button_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 74.0, rect.center().y - 14.0),
        vec2(74.0, 28.0),
    );
    let text_width = (button_rect.left() - (rect.left() + 2.0) - 10.0).max(40.0);
    painter.text(
        Pos2::new(rect.left() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        fit_text(
            ui,
            &format!(
                "每 {} / {}",
                interval_text(poll_secs),
                scan_age_text(last_scan, scanning)
            ),
            12.0,
            false,
            text_width,
        ),
        FontId::new(12.0, FontFamily::Proportional),
        if scanning { p.text } else { p.tertiary },
    );
    let response = ui.interact(button_rect, Id::new("footer.diagnostics"), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool(response.id.with("hover"), response.hovered());
    icons::pixel_frame(
        painter,
        button_rect,
        theme::lerp_color(p.control, p.hover, hover),
        p.card_border,
    );
    painter.text(
        button_rect.center(),
        Align2::CENTER_CENTER,
        "诊断",
        FontId::new(12.0, FontFamily::Name("bold".into())),
        p.text,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Interval in natural Chinese: "30 秒", "1 分钟", "1 分 30 秒", "30 分钟", "1 小时".
pub fn interval_text(seconds: u64) -> String {
    if seconds < 60 {
        format!("{} 秒", seconds)
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        let rest = seconds % 60;
        if rest == 0 {
            format!("{} 分钟", minutes)
        } else {
            format!("{} 分 {} 秒", minutes, rest)
        }
    } else {
        format!("{} 小时", seconds / 3600)
    }
}

fn scan_age_text(last_scan: Option<Instant>, scanning: bool) -> String {
    if scanning {
        return "正在扫描…".to_owned();
    }
    match last_scan.map(|instant| instant.elapsed().as_secs()) {
        None => "等待首次扫描".to_owned(),
        Some(secs) if secs < 5 => "刚刚刷新".to_owned(),
        Some(secs) if secs < 60 => format!("{} 秒前刷新", secs),
        Some(secs) if secs < 3600 => format!("{} 分钟前刷新", secs / 60),
        Some(secs) => format!("{} 小时前刷新", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_text_reads_naturally() {
        assert_eq!(interval_text(15), "15 秒");
        assert_eq!(interval_text(30), "30 秒");
        assert_eq!(interval_text(60), "1 分钟");
        assert_eq!(interval_text(90), "1 分 30 秒");
        assert_eq!(interval_text(300), "5 分钟");
        assert_eq!(interval_text(1800), "30 分钟");
        assert_eq!(interval_text(3600), "1 小时");
    }

    #[test]
    fn scan_age_distinguishes_scanning_and_fresh() {
        assert_eq!(scan_age_text(None, false), "等待首次扫描");
        assert_eq!(scan_age_text(None, true), "正在扫描…");
        assert_eq!(scan_age_text(Some(Instant::now()), false), "刚刚刷新");
    }
}

#[cfg(test)]
mod titlebar_tests {
    use super::*;

    #[test]
    fn caption_buttons_fit_the_minimum_width_without_overlap() {
        // The real bar is inset 2px on each side (width - 4) and starts 2px below
        // the window top, so it is TITLEBAR_HEIGHT - 2 tall.
        for width in [320.0, 360.0, 400.0, 430.0] {
            let bar = Rect::from_min_size(
                Pos2::new(0.0, 2.0),
                vec2(width - 4.0, TITLEBAR_HEIGHT - 4.0),
            );
            let layout = caption_button_layout(bar);
            for button in [layout.minimize, layout.maximize, layout.close] {
                assert_eq!(button.width(), CAPTION_BUTTON_SIZE);
                assert_eq!(button.height(), CAPTION_BUTTON_SIZE);
                assert!(
                    bar.contains_rect(button),
                    "button outside title bar at {width}"
                );
                assert_eq!(button.center().y, bar.center().y, "not centred at {width}");
            }
            assert_eq!(
                layout.maximize.left() - layout.minimize.right(),
                CAPTION_BUTTON_GAP
            );
            assert_eq!(
                layout.close.left() - layout.maximize.right(),
                CAPTION_BUTTON_GAP
            );
            assert!(layout.minimize.right() < layout.maximize.left());
            assert!(layout.maximize.right() < layout.close.left());
            assert_eq!(layout.close.right(), bar.right() - 6.0);

            // The drag target covers the rest of the bar and excludes every caption button.
            let drag = titlebar_drag_rect(bar);
            assert!(bar.contains_rect(drag), "drag area outside bar at {width}");
            assert!(drag.width() > 0.0, "empty drag area at {width}");
            assert!(drag.right() < layout.minimize.left());
            for button in [layout.minimize, layout.maximize, layout.close] {
                assert!(
                    !drag.intersects(button),
                    "drag area overlaps a caption button at {width}"
                );
            }
        }
    }
}
