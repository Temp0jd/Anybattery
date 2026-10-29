use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Layout, Pos2,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2,
};
use std::time::{Duration, Instant};

use crate::icons;
use crate::theme::{self, Palette};
use anybattery_core::{
    BatteryReading, BatteryStatus, CoarseLevel, Confidence, DeviceSnapshot, age_text,
};

pub const CARD_HEIGHT: f32 = 68.0;
/// Gap between two cards in the home list; the list height budget uses the same value.
pub const CARD_GAP: f32 = 6.0;
/// Edge length of one icon/text choice button. Nine of them plus gaps are exactly 284px wide.
pub const CHOICE_SIZE: f32 = 28.0;

/// Animations stay short: feedback, never decoration.
const ANIMATION_SECS: f32 = 0.15;
/// Refresh ring cadence while scanning; eight steps make one full turn.
const SCAN_REPAINT_SECS: f32 = 0.12;

/// Height of the custom interval slider, including its endpoint labels.
pub const SLIDER_HEIGHT: f32 = 34.0;
/// Eight-step ring used by the refresh control.
pub const REFRESH_RING_STEPS: usize = 8;

/// Map a value in `min..=max` to a 0..1 logarithmic position.
pub fn log_slider_t(value: f32, min: f32, max: f32) -> f32 {
    if !(min > 0.0 && max > min) || !value.is_finite() {
        return 0.0;
    }
    ((value.clamp(min, max) / min).ln() / (max / min).ln()).clamp(0.0, 1.0)
}

/// Map a 0..1 position back to a value in `min..=max`, logarithmically.
pub fn log_slider_value(t: f32, min: f32, max: f32) -> f32 {
    if !(min > 0.0 && max > min) {
        return min;
    }
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    min * (max / min).powf(t)
}

/// Current step of the refresh ring. Frozen at zero when animations are off.
pub fn refresh_spin_step(time: f64, animations: bool) -> usize {
    if !animations || !time.is_finite() {
        return 0;
    }
    ((time / f64::from(SCAN_REPAINT_SECS)) as i64).rem_euclid(REFRESH_RING_STEPS as i64) as usize
}

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

/// 28px 设备图标按钮，用于手动指定设备类型。图标、底色和边框与设备卡片一致。
pub fn device_icon_button(
    ui: &mut Ui,
    p: &Palette,
    kind: anybattery_core::DeviceKind,
    selected: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(CHOICE_SIZE, CHOICE_SIZE), Sense::click());
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
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        kind,
        color,
    );
    response.on_hover_text(kind.label_zh())
}

/// 24px 只读图标块，用于展示设备当前图标。内联选择器现在只用文字摘要，因此这个只读块
/// 保留了给设置/摘要视图复用。
#[allow(dead_code)]
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

/// 28px 文本选项按钮（“自动”“收起”）：字形与内边距都收在 28px 内，不会撑破图标行。
pub fn text_choice_button(ui: &mut Ui, p: &Palette, label: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(CHOICE_SIZE, CHOICE_SIZE), Sense::click());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    let fill = if selected {
        p.accent
    } else {
        theme::lerp_color(p.control, p.hover, hover)
    };
    let border = if selected { p.accent } else { p.card_border };
    icons::pixel_frame(ui.painter(), rect, fill, border);
    let color = if selected {
        p.on_accent
    } else {
        theme::lerp_color(p.secondary, p.text, hover)
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        fit_text(ui, label, 10.5, true, rect.width() - 2.0),
        FontId::new(10.5, FontFamily::Name("bold".into())),
        color,
    );
    response
}

/// Text wrapped to at most `max_rows` rows, so a narrow banner or toast can use two lines and
/// elide instead of spilling sideways. Uses the real font, so the caller can size to it.
pub fn wrapped_galley(
    ui: &Ui,
    text: &str,
    size: f32,
    bold: bool,
    max_width: f32,
    max_rows: usize,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: max_width.max(16.0),
            max_rows,
            break_anywhere: false,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: FontId::new(
                size,
                if bold {
                    FontFamily::Name("bold".into())
                } else {
                    FontFamily::Proportional
                },
            ),
            color,
            ..Default::default()
        },
    );
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
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
    let tile = card_icon_rect(rect);
    icons::pixel_frame(painter, tile, accent, p.card_border);
    icons::draw_device_icon(
        painter,
        Rect::from_center_size(tile.center(), vec2(20.0, 20.0)),
        device.kind,
        p.on_accent,
    );

    // Right column: the reading. Its width is measured once and shared by all rows.
    let reading_width = match device.battery {
        BatteryReading::Percent { level, .. } => percent_text_width(ui, level).max(BAR_WIDTH),
        BatteryReading::Coarse { level } => ui
            .painter()
            .layout_no_wrap(
                level.label_zh().to_owned(),
                FontId::new(16.0, FontFamily::Name("bold".into())),
                p.secondary,
            )
            .size()
            .x
            .max(BAR_WIDTH),
        BatteryReading::Unknown => BAR_WIDTH,
    };
    let left = card_text_left(rect);
    let right = card_right_edge(rect);
    let text_width = card_text_width(rect, reading_width);
    let density = DetailDensity::for_text_width(text_width);

    painter.text(
        Pos2::new(left, card_name_y(rect)),
        Align2::LEFT_CENTER,
        fit_text(ui, &device.name, 12.0, true, text_width),
        FontId::new(12.0, FontFamily::Name("bold".into())),
        p.text,
    );
    // States that are easy to confuse with a plain reading get a small text tag that does not
    // rely on colour alone.
    if let Some(label) = status_badge(device.status) {
        let badge = card_badge_rect(rect);
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
            FontId::new(11.0, FontFamily::Proportional),
            p.secondary,
        );
    }
    painter.text(
        Pos2::new(left, card_detail_y(rect)),
        Align2::LEFT_CENTER,
        fit_text(
            ui,
            &device_detail(device, now, density),
            12.0,
            false,
            text_width,
        ),
        FontId::new(12.0, FontFamily::Proportional),
        p.secondary,
    );

    // The percentage label is the true target value; only the cell fill eases.
    match device.battery {
        BatteryReading::Percent { level, .. } => {
            let color = theme::battery_color_for_level(*p, level);
            painter.text(
                Pos2::new(right, card_name_y(rect)),
                Align2::RIGHT_CENTER,
                format!("{}%", level),
                FontId::new(22.0, FontFamily::Name("bold".into())),
                p.text,
            );
            let bar = card_bar_rect(rect);
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
                Pos2::new(right, card_name_y(rect)),
                Align2::RIGHT_CENTER,
                level.label_zh(),
                FontId::new(16.0, FontFamily::Name("bold".into())),
                p.secondary,
            );
            coarse_pixel_battery(painter, p, card_bar_rect(rect), level);
        }
        BatteryReading::Unknown => {
            let bar = card_bar_rect(rect);
            painter.text(
                Pos2::new(right, card_name_y(rect)),
                Align2::RIGHT_CENTER,
                "---",
                FontId::new(18.0, FontFamily::Name("bold".into())),
                p.tertiary,
            );
            painter.text(
                Pos2::new(right, bar.center().y),
                Align2::RIGHT_CENTER,
                "无电量",
                FontId::new(11.0, FontFamily::Proportional),
                p.tertiary,
            );
        }
    }

    let device = device.clone();
    response.on_hover_ui(move |ui| {
        device_tooltip(ui, p, &device, now);
    })
}

const BAR_WIDTH: f32 = 54.0;
const BAR_HEIGHT: f32 = 10.0;
const PERCENT_CELLS: usize = 8;
const PERCENT_CELL_GAP: f32 = 1.5;
const COARSE_CELLS: usize = 3;
const COARSE_CELL_GAP: f32 = 2.0;
/// Card inset on every side; the icon and text columns start inside it.
const CARD_EDGE: f32 = 10.0;
const CARD_TEXT_LEFT: f32 = 52.0;
/// Minimum breathing room between the name column and the right reading column.
const CARD_COLUMN_GAP: f32 = 8.0;

fn card_icon_rect(card: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(card.left() + CARD_EDGE, card.top() + CARD_EDGE),
        vec2(32.0, 32.0),
    )
}

fn card_badge_rect(card: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(card.left() + CARD_EDGE, card.top() + 48.0),
        vec2(30.0, 12.0),
    )
}

fn card_bar_rect(card: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(card_right_edge(card) - BAR_WIDTH, card_detail_y(card)),
        vec2(BAR_WIDTH, BAR_HEIGHT),
    )
}

fn card_name_y(card: Rect) -> f32 {
    card.top() + 21.0
}

fn card_detail_y(card: Rect) -> f32 {
    card.top() + 43.0
}

fn card_text_left(card: Rect) -> f32 {
    card.left() + CARD_TEXT_LEFT
}

fn card_right_edge(card: Rect) -> f32 {
    card.right() - CARD_EDGE
}

/// Width left for the name/detail column: the reading column and an 8px gap are reserved first.
fn card_text_width(card: Rect, reading_width: f32) -> f32 {
    (card_right_edge(card) - reading_width - CARD_COLUMN_GAP - card_text_left(card)).max(16.0)
}

/// How much of the device detail the name column can afford at the current width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailDensity {
    Full,
    Compact,
    Minimal,
}

impl DetailDensity {
    /// 150px fits transport, age and source; 100px drops the age; below that only the
    /// state words survive, so a status can never be elided away by a narrow window.
    pub fn for_text_width(text_width: f32) -> Self {
        if text_width >= 150.0 {
            Self::Full
        } else if text_width >= 100.0 {
            Self::Compact
        } else {
            Self::Minimal
        }
    }
}

/// Width of the largest three-digit percentage at the reading size, so a 100% label
/// never jumps the column wider while a device changes.
fn percent_text_width(ui: &Ui, level: u8) -> f32 {
    let sample = if level >= 100 { "100%" } else { "88%" };
    ui.painter()
        .layout_no_wrap(
            sample.to_owned(),
            FontId::new(22.0, FontFamily::Name("bold".into())),
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

fn device_detail(device: &DeviceSnapshot, now: u64, density: DetailDensity) -> String {
    let transport = device.transport.label_zh();
    let age = age_text(device.updated_at_unix, now);
    let source = device.source.verification.label_zh();
    // Priority: abnormal state or unmatched protocol, charging, unverified source, transport,
    // age, source. Density only decides how much of that tail survives.
    let with_transport = |state: &'static str| -> String {
        // The abnormal state leads: it is the one word that must survive truncation.
        match density {
            DetailDensity::Full => format!("{} / {} / {}", state, transport, age),
            DetailDensity::Compact => format!("{} / {}", state, transport),
            DetailDensity::Minimal => state.to_owned(),
        }
    };
    match device.status {
        BatteryStatus::Unsupported => match density {
            DetailDensity::Minimal => "未匹配协议".to_owned(),
            _ => "已连接 / 未匹配电量协议".to_owned(),
        },
        BatteryStatus::Asleep => with_transport("休眠"),
        BatteryStatus::Stale => with_transport("可能不准"),
        BatteryStatus::Error => with_transport("读取失败"),
        BatteryStatus::Unknown => with_transport("等待数据"),
        BatteryStatus::Available | BatteryStatus::Charging | BatteryStatus::Full => {
            let charging = device.battery.charging() == Some(true);
            let unverified = device.source.verification == Confidence::Unverified;
            let mut parts: Vec<&str> = Vec::new();
            if charging {
                parts.push("充电中");
            }
            // Must never be cut: it says the reading is a guess.
            if unverified {
                parts.push("未验证");
            }
            parts.push(transport);
            match density {
                DetailDensity::Full => {
                    parts.push(&age);
                    if !unverified {
                        parts.push(source);
                    }
                }
                DetailDensity::Compact => {
                    if !unverified {
                        parts.push(source);
                    }
                }
                DetailDensity::Minimal => {}
            }
            parts.join(" / ")
        }
    }
}

/// Cell rectangles for the pixel battery bars. Both bar styles use it, so the 8-cell
/// percentage bar and the 3-cell coarse bar cannot drift apart.
fn cell_rects(rect: Rect, cells: usize, gap: f32) -> Vec<Rect> {
    let cell_width = (rect.width() - gap * cells.saturating_sub(1) as f32) / cells as f32;
    (0..cells)
        .map(|index| {
            Rect::from_min_size(
                Pos2::new(rect.left() + index as f32 * (cell_width + gap), rect.top()),
                vec2(cell_width, rect.height()),
            )
        })
        .collect()
}

pub(crate) fn pixel_battery(
    painter: &egui::Painter,
    p: &Palette,
    rect: Rect,
    fill: f32,
    color: Color32,
) {
    let filled = (fill.clamp(0.0, 1.0) * PERCENT_CELLS as f32).ceil() as usize;
    for (index, cell) in cell_rects(rect, PERCENT_CELLS, PERCENT_CELL_GAP)
        .into_iter()
        .enumerate()
    {
        let cell = theme::snap_rect(painter.ctx().pixels_per_point(), cell);
        let fill = if index < filled { color } else { p.track };
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
    let filled = match level {
        CoarseLevel::Low => 1,
        CoarseLevel::Medium => 2,
        CoarseLevel::Full => 3,
    };
    for (index, cell) in cell_rects(rect, COARSE_CELLS, COARSE_CELL_GAP)
        .into_iter()
        .enumerate()
    {
        let cell = theme::snap_rect(painter.ctx().pixels_per_point(), cell);
        painter.rect_filled(
            cell,
            CornerRadius::ZERO,
            if index < filled { p.secondary } else { p.track },
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

/// Outer height of a [`segment`] control. The selected layer is inset by [`SEGMENT_INSET`],
/// but the click target always covers the full height so the control is easy to hit.
pub const SEGMENT_HEIGHT: f32 = 30.0;
/// Inset of the selected layer inside the segment frame.
pub const SEGMENT_INSET: f32 = 2.0;

/// Equal-width column of one segment option.
pub fn segment_column_rect(frame: Rect, index: usize, count: usize) -> Rect {
    let count = count.max(1) as f32;
    let seg_width = (frame.width() - SEGMENT_INSET * 2.0) / count;
    Rect::from_min_size(
        Pos2::new(
            frame.left() + SEGMENT_INSET + index as f32 * seg_width,
            frame.top(),
        ),
        vec2(seg_width, frame.height()),
    )
}

/// Click target and text anchor: the full 30px column, not the 26px selected layer.
pub fn segment_click_rect(frame: Rect, index: usize, count: usize) -> Rect {
    segment_column_rect(frame, index, count)
}

/// Selected (and hover/indicator) layer, inset 2px on every side.
pub fn segment_marker_rect(frame: Rect, index: usize, count: usize) -> Rect {
    segment_column_rect(frame, index, count).shrink2(vec2(0.0, SEGMENT_INSET))
}

pub fn segment(ui: &mut Ui, p: &Palette, labels: &[&str], selected: usize) -> SegmentResult {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, SEGMENT_HEIGHT), Sense::hover());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let painter = ui.painter();
    icons::draw_pixel_shadow(painter, rect, p.shadow);
    icons::pixel_frame(painter, rect, p.control, p.card_border);
    let count = labels.len().max(1);
    let seg_width = (rect.width() - SEGMENT_INSET * 2.0) / count as f32;
    let marker_id = ui.id().with("segment-indicator");
    let shown = ui
        .ctx()
        .animate_value_with_time(marker_id, selected as f32, animation_secs(ui));
    let marker = Rect::from_min_size(
        Pos2::new(
            rect.left() + SEGMENT_INSET + shown * seg_width,
            rect.top() + SEGMENT_INSET,
        ),
        vec2(seg_width, rect.height() - SEGMENT_INSET * 2.0),
    );
    painter.rect_filled(
        theme::snap_rect(ui.ctx().pixels_per_point(), marker),
        CornerRadius::ZERO,
        p.accent,
    );
    let mut result = SegmentResult::None;
    for (index, label) in labels.iter().enumerate() {
        let seg = segment_marker_rect(rect, index, count);
        let hit = segment_click_rect(rect, index, count);
        let response = ui.interact(hit, Id::new(("segment", label, index)), Sense::click());
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
        // An eight-block pixel ring: the gap and the two bright blocks rotate clockwise.
        // One step per 120 ms keeps the cost bounded; animations off freezes the ring.
        let step = refresh_spin_step(ui.input(|i| i.time), animations);
        if animations {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(SCAN_REPAINT_SECS));
        }
        draw_refresh_ring(painter, rect, color, Some(step));
    } else {
        draw_refresh_ring(painter, rect, color, None);
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

/// Eight square blocks on a circle. `spin` is `Some(step)` while scanning: the block at
/// `step` becomes the gap, and the two blocks after it stay bright so the rotation is visible.
fn draw_refresh_ring(painter: &egui::Painter, rect: Rect, color: Color32, spin: Option<usize>) {
    let c = rect.center();
    let radius = 7.0;
    let block = 3.0;
    let ppp = painter.ctx().pixels_per_point();
    for index in 0..REFRESH_RING_STEPS {
        if spin.is_some_and(|step| step % REFRESH_RING_STEPS == index) {
            continue;
        }
        let angle = -std::f32::consts::FRAC_PI_2
            + index as f32 * std::f32::consts::TAU / REFRESH_RING_STEPS as f32;
        let center = Pos2::new(c.x + angle.cos() * radius, c.y + angle.sin() * radius);
        let block_color = match spin {
            None => color,
            Some(step) => {
                let distance = (index + REFRESH_RING_STEPS - step) % REFRESH_RING_STEPS;
                if distance <= 2 {
                    color
                } else {
                    color.gamma_multiply(0.38)
                }
            }
        };
        let cell = Rect::from_center_size(center, vec2(block, block));
        painter.rect_filled(theme::snap_rect(ppp, cell), CornerRadius::ZERO, block_color);
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

/// Toggle width: the visual track and the click target share it.
pub const TOGGLE_WIDTH: f32 = 44.0;
/// Click target height. Taller than the track so the switch matches the control column.
pub const TOGGLE_HIT_HEIGHT: f32 = SETTINGS_CONTROL_HEIGHT;
/// Visual track height, vertically centred inside the hit rectangle.
pub const TOGGLE_TRACK_HEIGHT: f32 = 24.0;

/// Track of a toggle, centred in its 44x28 hit rectangle so the two can never drift apart.
pub fn toggle_track_rect(hit: Rect) -> Rect {
    Rect::from_center_size(hit.center(), vec2(TOGGLE_WIDTH, TOGGLE_TRACK_HEIGHT))
}

pub fn toggle(ui: &mut Ui, p: &Palette, value: &mut bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(TOGGLE_WIDTH, TOGGLE_HIT_HEIGHT), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool(response.id, *value);
    let track = theme::snap_rect(ui.ctx().pixels_per_point(), toggle_track_rect(rect));
    let track_color = theme::lerp_color(p.control, p.green, t);
    icons::pixel_frame(ui.painter(), track, track_color, p.card_border);
    let knob = Rect::from_min_size(
        Pos2::new(
            track.left() + 3.0 + t * (track.width() - 22.0),
            track.top() + 3.0,
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

/// One-line settings row height. Two text baselines (title + detail) and a 28px control
/// column still fit, so settings pages stay scrollable in a 320x480 window.
pub const SETTINGS_ROW_HEIGHT: f32 = 44.0;
/// Fixed width of the control column at the right edge of a settings row.
pub const SETTINGS_CONTROL_WIDTH: f32 = 56.0;
/// Height of that control column: one 28px widget, vertically centred in the row.
pub const SETTINGS_CONTROL_HEIGHT: f32 = 28.0;
/// Left inset shared by the title and detail text.
pub const SETTINGS_ROW_INSET: f32 = 12.0;
/// Right inset of the control column, and the gap between text and control.
pub const SETTINGS_CONTROL_GAP: f32 = 8.0;

/// Text column width for a settings row of `row_width`.
pub fn settings_text_width(row_width: f32) -> f32 {
    (row_width - SETTINGS_ROW_INSET - SETTINGS_CONTROL_WIDTH - SETTINGS_CONTROL_GAP).max(40.0)
}

/// Right-aligned control column: 48x28, vertically centred in the 44px row.
pub fn settings_control_rect(row: Rect) -> Rect {
    let inset = (SETTINGS_ROW_HEIGHT - SETTINGS_CONTROL_HEIGHT) / 2.0;
    Rect::from_min_max(
        Pos2::new(row.right() - SETTINGS_CONTROL_WIDTH, row.top() + inset),
        Pos2::new(row.right() - SETTINGS_CONTROL_GAP, row.bottom() - inset),
    )
}

pub fn settings_row(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    detail: &str,
    control: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, SETTINGS_ROW_HEIGHT), Sense::hover());
    // Control column is reserved first; the text column gets whatever is left.
    let text_width = settings_text_width(rect.width());
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left() + SETTINGS_ROW_INSET, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        fit_text(ui, title, 12.0, false, text_width),
        FontId::new(12.0, FontFamily::Proportional),
        p.text,
    );
    if !detail.is_empty() {
        painter.text(
            Pos2::new(rect.left() + SETTINGS_ROW_INSET, rect.top() + 32.0),
            Align2::LEFT_CENTER,
            fit_text(ui, detail, 11.0, false, text_width),
            FontId::new(11.0, FontFamily::Proportional),
            p.tertiary,
        );
    }
    let control_rect = settings_control_rect(rect);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(control_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    control(&mut child);
}

/// Default group padding. Roomy enough for mixed rows, still compact.
pub const GROUP_PADDING: i8 = 6;
/// Tight group padding, reserved for rows with an exact width budget (the 284px icon
/// picker row) and other deliberately compact rows.
pub const GROUP_COMPACT_PADDING: i8 = 4;
const GROUP_BORDER: f32 = 2.0;

/// Inner content width of a group whose outer width is `outer_width`. Used by width-budget
/// helpers and tests, which is why it is not part of the rendering path.
#[cfg_attr(not(test), allow(dead_code))]
pub fn group_inner_width(outer_width: f32, padding: i8) -> f32 {
    (outer_width - 2.0 * (GROUP_BORDER + f32::from(padding))).max(0.0)
}

pub fn group(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    framed_group(ui, p, GROUP_PADDING, add);
}

pub fn group_compact(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    framed_group(ui, p, GROUP_COMPACT_PADDING, add);
}

fn framed_group(ui: &mut Ui, p: &Palette, padding: i8, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(GROUP_BORDER, p.card_border))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(egui::Margin::symmetric(padding, padding))
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

/// Pixel interval slider: a framed 12px track, accent fill, 14x22 knob and endpoint labels.
/// Values map logarithmically, matching the old egui slider's range.
pub fn pixel_slider(ui: &mut Ui, p: &Palette, value: &mut f32, min: f32, max: f32) -> Response {
    let width = ui.available_width();
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width, SLIDER_HEIGHT), Sense::click_and_drag());
    let rect = theme::snap_rect(ui.ctx().pixels_per_point(), rect);
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        Pos2::new(rect.right() - 8.0, rect.top() + 18.0),
    );

    if response.clicked() {
        response.request_focus();
    }
    let mut changed = false;
    if let Some(pointer) = response.interact_pointer_pos() {
        if response.clicked() || response.dragged() {
            let t = if track.width() > 0.0 {
                ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let next = log_slider_value(t, min, max);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }
    }
    if response.has_focus() {
        let (left, right, shift) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
                input.modifiers.shift,
            )
        });
        if left || right {
            let step = if shift { 60.0 } else { 15.0 };
            let next = if left { *value - step } else { *value + step };
            let next = next.clamp(min, max);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }
    }
    if changed {
        response.mark_changed();
    }

    let t = log_slider_t(*value, min, max);
    let painter = ui.painter();
    icons::pixel_frame(painter, track, p.control, p.card_border);
    let inner = track.shrink(2.0);
    if t > 0.0 {
        let fill = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.left() + inner.width() * t, inner.bottom()),
        );
        painter.rect_filled(fill, CornerRadius::ZERO, p.accent);
    }
    // Preset notches stay visible over the fill and make the scale legible.
    for seconds in [30.0_f32, 60.0, 300.0, 600.0] {
        let x = track.left() + track.width() * log_slider_t(seconds, min, max);
        painter.rect_filled(
            theme::snap_rect(
                ui.ctx().pixels_per_point(),
                Rect::from_min_size(Pos2::new(x, track.top() - 3.0), vec2(1.0, 3.0)),
            ),
            CornerRadius::ZERO,
            p.tertiary,
        );
    }
    let knob_center = Pos2::new(track.left() + track.width() * t, track.center().y);
    let knob = Rect::from_center_size(knob_center, vec2(14.0, 22.0));
    let knob_border = if response.hovered() || response.dragged() {
        p.accent
    } else {
        p.card_border
    };
    icons::pixel_frame(painter, knob, p.card_solid, knob_border);
    painter.rect_filled(
        theme::snap_rect(
            ui.ctx().pixels_per_point(),
            Rect::from_center_size(knob_center, vec2(2.0, 8.0)),
        ),
        CornerRadius::ZERO,
        p.accent,
    );
    painter.text(
        Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        "15 秒",
        FontId::new(9.0, FontFamily::Proportional),
        p.tertiary,
    );
    painter.text(
        Pos2::new(rect.right() - 2.0, rect.bottom() - 2.0),
        Align2::RIGHT_BOTTOM,
        "30 分钟",
        FontId::new(9.0, FontFamily::Proportional),
        p.tertiary,
    );
    response.on_hover_text("拖动调整刷新间隔；方向键微调")
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
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let painter = ui.painter();
    // 1px dotted seam (2px dash, 2px gap): a printed divider, not a heavy rule.
    let mut x = rect.left();
    while x < rect.right() {
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, rect.top() + 1.0), vec2(2.0, 1.0)),
            CornerRadius::ZERO,
            p.track,
        );
        x += 4.0;
    }
    let button_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 64.0, rect.center().y - 14.0),
        vec2(64.0, 28.0),
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
mod slider_tests {
    use super::*;

    #[test]
    fn logarithmic_slider_round_trips_the_supported_range() {
        for value in [15.0, 30.0, 60.0, 300.0, 600.0, 1800.0] {
            let t = log_slider_t(value, 15.0, 1800.0);
            let back = log_slider_value(t, 15.0, 1800.0);
            assert!((back - value).abs() < 0.01, "{value} -> {t} -> {back}");
        }
        assert_eq!(log_slider_t(15.0, 15.0, 1800.0), 0.0);
        assert_eq!(log_slider_t(1800.0, 15.0, 1800.0), 1.0);
        // A 60-second preset sits below the midpoint in log space, not above it.
        assert!(log_slider_t(60.0, 15.0, 1800.0) < 0.5);
    }

    #[test]
    fn slider_math_survives_invalid_values() {
        assert_eq!(log_slider_t(-1.0, 15.0, 1800.0), 0.0);
        assert_eq!(log_slider_t(f32::NAN, 15.0, 1800.0), 0.0);
        assert_eq!(log_slider_value(f32::NAN, 15.0, 1800.0), 15.0);
        assert_eq!(log_slider_value(0.0, 0.0, 10.0), 0.0);
        assert_eq!(log_slider_value(0.0, 18.0, 18.0), 18.0);
        assert_eq!(SLIDER_HEIGHT, 34.0);
    }

    #[test]
    fn refresh_ring_uses_eight_wrapping_steps() {
        assert_eq!(REFRESH_RING_STEPS, 8);
        assert_eq!(refresh_spin_step(0.0, true), 0);
        assert_eq!(refresh_spin_step(0.13, true), 1);
        assert_eq!(refresh_spin_step(0.25, true), 2);
        assert!(refresh_spin_step(1.01, true) < REFRESH_RING_STEPS);
        assert_eq!(refresh_spin_step(99.0, false), 0);
        assert_eq!(refresh_spin_step(f64::NAN, true), 0);
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

#[cfg(test)]
mod card_tests {
    use super::*;
    use anybattery_core::{BatterySource, DeviceId, DeviceKind, Transport};

    fn card(width: f32) -> Rect {
        Rect::from_min_size(Pos2::new(20.0, 40.0), vec2(width, CARD_HEIGHT))
    }

    fn snapshot(
        status: BatteryStatus,
        battery: BatteryReading,
        verification: Confidence,
    ) -> DeviceSnapshot {
        let mut device = DeviceSnapshot::new(
            DeviceId::new("test", "设备"),
            "设备",
            DeviceKind::Mouse,
            Transport::Usb24g,
            BatterySource::new("test", verification, ""),
        );
        device.status = status;
        device.battery = battery;
        device.updated_at_unix = Some(1_000);
        device
    }

    /// Runs the closure inside a real frame so text measurement uses the installed pixel font.
    fn measured(f: impl Fn(&Ui) -> Vec<f32>) -> Vec<f32> {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let out = std::cell::RefCell::new(Vec::new());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            *out.borrow_mut() = f(ui);
        });
        output.textures_delta.clear();
        out.into_inner()
    }

    #[test]
    fn card_elements_fit_the_68px_card() {
        assert_eq!(CARD_HEIGHT, 68.0);
        for width in [296.0, 368.0] {
            let card = card(width);
            let icon = card_icon_rect(card);
            assert_eq!((icon.width(), icon.height()), (32.0, 32.0));
            assert_eq!(icon.min, Pos2::new(card.left() + 10.0, card.top() + 10.0));
            let badge = card_badge_rect(card);
            assert_eq!((badge.width(), badge.height()), (30.0, 12.0));
            let bar = card_bar_rect(card);
            assert_eq!((bar.width(), bar.height()), (54.0, BAR_HEIGHT));
            for element in [icon, badge, bar] {
                assert!(
                    card.contains_rect(element),
                    "element outside the card at {width}: {element:?}"
                );
            }
            // The right column hugs the card inset; the text column keeps its 8px gap.
            assert_eq!(bar.right(), card.right() - 10.0);
            assert!(card_text_left(card) + card_text_width(card, BAR_WIDTH) <= bar.left() - 8.0);
            // Icon block and status badge both stay left of the text column.
            assert!(icon.right() < card_text_left(card));
            assert!(badge.right() < card_text_left(card));
            // 12px text is 8px from its centre, so both rows still sit inside 68px.
            assert!(card_name_y(card) + 8.0 <= card.bottom());
            assert!(card_detail_y(card) + 8.0 <= card.bottom());
            assert!(bar.bottom() + 8.0 <= card.bottom());
        }
    }

    #[test]
    fn percent_column_width_is_stable_and_fits_three_digits() {
        let values = measured(|ui| {
            let rendered = ui
                .painter()
                .layout_no_wrap(
                    "100%".to_owned(),
                    FontId::new(22.0, FontFamily::Name("bold".into())),
                    Color32::WHITE,
                )
                .size()
                .x;
            vec![
                percent_text_width(ui, 8),
                percent_text_width(ui, 88),
                percent_text_width(ui, 100),
                rendered,
            ]
        });
        let (two_digit, same_two_digit, three_digit, rendered) =
            (values[0], values[1], values[2], values[3]);
        assert_eq!(
            two_digit, same_two_digit,
            "the two-digit column must not jump between levels"
        );
        assert!(
            three_digit > two_digit,
            "100% must reserve a wider cell ({three_digit} vs {two_digit})"
        );
        assert!(
            three_digit >= rendered,
            "100% must not be truncated ({three_digit} < {rendered})"
        );
    }

    #[test]
    fn detail_density_switches_at_150_and_100() {
        assert_eq!(DetailDensity::for_text_width(99.0), DetailDensity::Minimal);
        assert_eq!(DetailDensity::for_text_width(100.0), DetailDensity::Compact);
        assert_eq!(DetailDensity::for_text_width(149.0), DetailDensity::Compact);
        assert_eq!(DetailDensity::for_text_width(150.0), DetailDensity::Full);
    }

    #[test]
    fn unverified_stays_visible_at_minimal_density() {
        let device = snapshot(
            BatteryStatus::Charging,
            BatteryReading::percent(40, Some(true)),
            Confidence::Unverified,
        );
        let detail = device_detail(&device, 1_000, DetailDensity::Minimal);
        assert!(detail.contains("未验证"), "{detail}");
        let parts: Vec<&str> = detail.split(" / ").collect();
        assert_eq!(parts[0], "充电中");
        assert_eq!(parts[1], "未验证");
        assert!(!detail.contains("  "), "empty fragment in {detail}");
    }

    #[test]
    fn unsupported_minimal_shortens_the_protocol_note() {
        let device = snapshot(
            BatteryStatus::Unsupported,
            BatteryReading::Unknown,
            Confidence::Unverified,
        );
        assert_eq!(
            device_detail(&device, 1_000, DetailDensity::Minimal),
            "未匹配协议"
        );
        assert_eq!(
            device_detail(&device, 1_000, DetailDensity::Compact),
            "已连接 / 未匹配电量协议"
        );
    }

    #[test]
    fn abnormal_state_leads_the_detail_at_every_density() {
        let device = snapshot(
            BatteryStatus::Asleep,
            BatteryReading::percent(30, None),
            Confidence::Verified,
        );
        for density in [
            DetailDensity::Full,
            DetailDensity::Compact,
            DetailDensity::Minimal,
        ] {
            let detail = device_detail(&device, 1_000, density);
            assert!(detail.starts_with("休眠"), "{density:?}: {detail}");
        }
    }

    #[test]
    fn charging_leads_the_compact_detail() {
        let device = snapshot(
            BatteryStatus::Charging,
            BatteryReading::percent(90, Some(true)),
            Confidence::Verified,
        );
        let detail = device_detail(&device, 1_000, DetailDensity::Compact);
        assert!(detail.starts_with("充电中"), "{detail}");
        assert!(detail.contains("2.4G"), "{detail}");
        assert_eq!(detail.matches("已验证").count(), 1, "{detail}");
    }

    #[test]
    fn nine_choices_fit_one_row_in_a_320px_window() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let p = theme::palette(false);
        let rects = std::cell::RefCell::new(Vec::new());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            // 320px window: 296px page content, then the group border and padding take 12px.
            ui.set_width(296.0);
            group_compact(ui, &p, |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.horizontal(|ui| {
                    let mut collect = |response: Response| rects.borrow_mut().push(response.rect);
                    collect(text_choice_button(ui, &p, "自动", false));
                    for kind in [
                        anybattery_core::DeviceKind::Mouse,
                        anybattery_core::DeviceKind::Keyboard,
                        anybattery_core::DeviceKind::Headset,
                        anybattery_core::DeviceKind::Earbuds,
                        anybattery_core::DeviceKind::Speaker,
                        anybattery_core::DeviceKind::Controller,
                        anybattery_core::DeviceKind::Pen,
                        anybattery_core::DeviceKind::Unknown,
                    ] {
                        collect(device_icon_button(ui, &p, kind, false));
                    }
                });
            });
        });
        output.textures_delta.clear();

        let rects = rects.into_inner();
        assert_eq!(rects.len(), 9);
        for rect in &rects {
            assert_eq!((rect.width(), rect.height()), (CHOICE_SIZE, CHOICE_SIZE));
            assert_eq!(
                rect.center().y,
                rects[0].center().y,
                "choice wrapped to a new row"
            );
        }
        let span = rects[8].right() - rects[0].left();
        assert!(
            (span - 284.0).abs() < 0.01,
            "row spans {span}px, expected 284"
        );
        // Everything stays inside the group content box (296 - 2px border - 4px padding).
        assert!(rects[8].right() <= rects[0].left() + 284.0 + 0.01);
    }

    #[test]
    fn battery_bars_keep_their_cell_counts() {
        let bar = card_bar_rect(card(296.0));
        let percent_cells = cell_rects(bar, PERCENT_CELLS, PERCENT_CELL_GAP);
        assert_eq!(percent_cells.len(), 8);
        let coarse_cells = cell_rects(bar, COARSE_CELLS, COARSE_CELL_GAP);
        assert_eq!(coarse_cells.len(), 3);
        for (cells, gap) in [
            (&percent_cells, PERCENT_CELL_GAP),
            (&coarse_cells, COARSE_CELL_GAP),
        ] {
            for (index, cell) in cells.iter().enumerate() {
                assert!(bar.contains_rect(*cell), "cell outside the bar: {cell:?}");
                if index > 0 {
                    let gap_seen = cell.left() - cells[index - 1].right();
                    assert!((gap_seen - gap).abs() < 0.01, "gap {gap_seen} != {gap}");
                }
            }
            assert!((cells[cells.len() - 1].right() - bar.right()).abs() < 0.01);
        }
    }
}

#[cfg(test)]
mod rhythm_tests {
    use super::*;

    #[test]
    fn settings_row_keeps_the_44px_rhythm() {
        assert_eq!(SETTINGS_ROW_HEIGHT, 44.0);
        assert_eq!(SETTINGS_CONTROL_WIDTH, 56.0);
        // At 320px the settings content is 296px wide; the text column keeps the rest.
        assert_eq!(settings_text_width(296.0), 296.0 - 12.0 - 56.0 - 8.0);
        assert_eq!(settings_text_width(40.0), 40.0, "never negative");

        let row = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(296.0, SETTINGS_ROW_HEIGHT));
        let control = settings_control_rect(row);
        assert_eq!((control.width(), control.height()), (48.0, 28.0));
        assert_eq!(control.right(), row.right() - 8.0);
        assert_eq!(control.center().y, row.center().y);
        assert!(row.contains_rect(control));
        // Title at +14 and detail at +32 both stay inside the 44px row.
        assert!(row.top() + 14.0 + 6.0 < row.bottom());
        assert!(row.top() + 32.0 + 6.0 <= row.bottom());

        // The toggle hit rectangle is 44x28 and fills the control column.
        assert_eq!(SETTINGS_CONTROL_HEIGHT, 28.0);
        assert_eq!(TOGGLE_HIT_HEIGHT, SETTINGS_CONTROL_HEIGHT);
        let hit = Rect::from_min_size(
            control.right_top() - vec2(TOGGLE_WIDTH, 0.0),
            vec2(TOGGLE_WIDTH, TOGGLE_HIT_HEIGHT),
        );
        assert_eq!((hit.width(), hit.height()), (44.0, 28.0));
        assert_eq!(hit.height(), control.height());
        assert!(control.contains_rect(hit));
        let track = toggle_track_rect(hit);
        assert_eq!((track.width(), track.height()), (44.0, 24.0));
        assert_eq!(track.center(), hit.center());
        assert!(hit.contains_rect(track));
    }

    #[test]
    fn segment_clicks_the_full_30px_column() {
        assert_eq!(SEGMENT_HEIGHT, 30.0);
        let frame = Rect::from_min_size(Pos2::new(4.0, 8.0), vec2(280.0, SEGMENT_HEIGHT));
        let count = 4;
        let column = (280.0 - SEGMENT_INSET * 2.0) / count as f32;
        let mut previous_right = frame.left() + SEGMENT_INSET;
        for index in 0..count {
            let click = segment_click_rect(frame, index, count);
            let marker = segment_marker_rect(frame, index, count);
            assert_eq!(click.height(), SEGMENT_HEIGHT, "click must be 30px tall");
            assert_eq!(marker.height(), SEGMENT_HEIGHT - 4.0);
            assert!((click.width() - column).abs() < 0.01, "uneven columns");
            assert!(
                (click.left() - previous_right).abs() < 0.01,
                "gap or overlap"
            );
            assert!(click.contains_rect(marker));
            assert_eq!(marker.center().y, click.center().y);
            assert_eq!(click.center().x, marker.center().x);
            previous_right = click.right();
        }
        assert!((previous_right - (frame.right() - SEGMENT_INSET)).abs() < 0.01);
    }

    #[test]
    fn group_padding_matches_the_layout_budget() {
        // Pure arithmetic first: 296px is the settings content width at a 320px window.
        assert_eq!(group_inner_width(296.0, GROUP_PADDING), 280.0);
        assert_eq!(group_inner_width(296.0, GROUP_COMPACT_PADDING), 284.0);
        assert_eq!(group_inner_width(8.0, GROUP_PADDING), 0.0, "no negative");

        // Then a real frame probe, so the constants cannot drift from egui's margin maths.
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let p = theme::palette(false);
        let widths = std::cell::RefCell::new(Vec::new());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(296.0);
            group(ui, &p, |ui| widths.borrow_mut().push(ui.available_width()));
            ui.set_width(296.0);
            group_compact(ui, &p, |ui| widths.borrow_mut().push(ui.available_width()));
        });
        output.textures_delta.clear();
        let widths = widths.into_inner();
        assert_eq!(widths.len(), 2);
        assert!(
            (widths[0] - 280.0).abs() < 0.01,
            "group inner {}",
            widths[0]
        );
        assert!(
            (widths[1] - 284.0).abs() < 0.01,
            "compact group inner {}",
            widths[1]
        );
        // The icon picker's 9-choice row needs exactly the compact budget.
        assert_eq!(widths[1], CHOICE_SIZE * 9.0 + 4.0 * 8.0);
    }
}
