//! 托盘悬浮窗：hover 状态机、屏幕内定位与像素风自绘。
//!
//! 这个模块不接触 tray-icon 事件源。`tray` 把
//! `TrayIconEvent::Enter/Move/Leave` 翻译成 [`TrayHoverEvent`] 后喂给
//! [`TrayPopupState`]，每帧用 [`TrayPopupModel`] + [`show`] 画一次。
//!
//! 视觉规则与主面板一致：零圆角、2px 硬边框、3px 像素投影、8 格电量条、
//! 像素字体；没有渐变、圆角和动画。

use std::time::{Duration, Instant};

use eframe::egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Painter, Pos2, Rect, Ui,
    UiBuilder, Vec2, vec2,
};

use anybattery_core::{
    BatteryReading, BatteryStatus, CoarseLevel, Confidence, DeviceId, DeviceSnapshot,
};
use anybattery_windows::monitor::MonitorGeometry;

use crate::icons;
use crate::theme::{self, Palette};
use crate::widgets;

/// 悬浮窗宽度（points）。宽度固定，行数只影响高度。
pub const POPUP_WIDTH: f32 = 320.0;
/// 没有任何设备行时的高度（points）。
pub const BASE_HEIGHT: f32 = 146.0;
/// 单行设备占用的高度（points）。
pub const ROW_HEIGHT: f32 = 32.0;
/// 悬浮窗最多列出的设备数；其余只报数量。
pub const MAX_ROWS: usize = 4;
/// 悬停多久之后才真正显示，避免掠过图标时闪一下。
pub const SHOW_DELAY: Duration = Duration::from_millis(250);
/// 鼠标离开后保留多久再收起，容忍一点点手抖。
pub const LEAVE_GRACE: Duration = Duration::from_millis(180);
/// 悬浮窗最长存活时间；即使一直悬停也会自动收起。
pub const MAX_AGE: Duration = Duration::from_secs(8);

/// 悬浮窗与图标矩形之间至少留出的缝隙（points）。
const ANCHOR_GAP: f32 = 8.0;

// 内部栅格（points）。高度固定为 `BASE_HEIGHT + rows * ROW_HEIGHT`，所以这些
// 数值必须排得进去：标题 22 + 间隙 + 分隔 2 + 汇总 14 + 设备行 + 底部提示。
const PAD: f32 = 10.0;
const TITLE_HEIGHT: f32 = 22.0;
const GAP: f32 = 4.0;
const SUMMARY_HEIGHT: f32 = 14.0;
const REMAINDER_HEIGHT: f32 = 13.0;
const BAR_WIDTH: f32 = 140.0;
const BAR_HEIGHT: f32 = 7.0;
const NAME_MAX_WIDTH: f32 = 190.0;

// 32px 行内的竖直栅格：名称居中于 y+10，电量条从 y+20 起（高 7），徽标中心压在
// y+25，三者在行内不碰撞，也不会压到下一行的分隔线。
const ROW_NAME_Y: f32 = 10.0;
const ROW_BAR_Y: f32 = 20.0;
const ROW_BADGE_Y: f32 = 25.0;
/// 底部两行提示：第一行基线距内边距底边 7px，第二行再往上 14px。
const HINT_BASELINE: f32 = 7.0;
const HINT_LINE_HEIGHT: f32 = 14.0;

/// 托盘图标在屏幕上的物理像素矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrayAnchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TrayAnchor {
    /// `tray-icon` 偶尔会给出 0 尺寸矩形（图标尚未布局完成），这时退化成
    /// 以该点为中心、与托盘图标同尺寸的 16x16 方块。
    pub fn new(x: f64, y: f64, width: u32, height: u32) -> Self {
        if width == 0 || height == 0 {
            const FALLBACK: f64 = 16.0;
            Self {
                x: x - FALLBACK / 2.0,
                y: y - FALLBACK / 2.0,
                width: FALLBACK,
                height: FALLBACK,
            }
        } else {
            Self {
                x,
                y,
                width: f64::from(width),
                height: f64::from(height),
            }
        }
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
}

/// 托盘图标的悬停事件，由 `tray` 从 `TrayIconEvent` 翻译而来。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayHoverEvent {
    Enter(TrayAnchor),
    Move(TrayAnchor),
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayPopupPhase {
    Hidden,
    Pending,
    Visible,
}

/// 悬停状态机。时间完全由调用方传入，所以这里可以确定性测试，也不需要计时线程。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrayPopupState {
    phase: TrayPopupPhase,
    anchor: Option<TrayAnchor>,
    show_at: Option<Instant>,
    expires_at: Option<Instant>,
    leave_deadline: Option<Instant>,
    /// 用户点过托盘图标之后，鼠标微动不应该重新拉起悬浮窗；只有全新的 Enter 才恢复。
    suppress_moves: bool,
}

impl Default for TrayPopupState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrayPopupState {
    pub fn new() -> Self {
        Self {
            phase: TrayPopupPhase::Hidden,
            anchor: None,
            show_at: None,
            expires_at: None,
            leave_deadline: None,
            suppress_moves: false,
        }
    }

    pub fn phase(&self) -> TrayPopupPhase {
        self.phase
    }

    pub fn anchor(&self) -> Option<TrayAnchor> {
        self.anchor
    }

    pub fn is_visible(&self) -> bool {
        self.phase == TrayPopupPhase::Visible
    }

    /// 处理一次悬停事件，返回状态或锚点是否变化（调用方据此决定是否重绘）。
    pub fn on_event(&mut self, event: TrayHoverEvent, now: Instant, main_hidden: bool) -> bool {
        let before = *self;

        // 主窗口可见时悬浮窗没有存在意义：直接收起。但不设置 suppress——
        // 那不是用户主动关闭，不该影响下一次悬停。
        if !main_hidden {
            self.hide();
            return *self != before;
        }

        match event {
            TrayHoverEvent::Enter(anchor) => {
                self.suppress_moves = false;
                match self.phase {
                    TrayPopupPhase::Hidden => {
                        self.anchor = Some(anchor);
                        self.phase = TrayPopupPhase::Pending;
                        self.show_at = Some(now + SHOW_DELAY);
                        self.expires_at = None;
                        self.leave_deadline = None;
                    }
                    // 已经在倒计时：只跟随图标位置，不重新计时，否则会永远不显示。
                    TrayPopupPhase::Pending => self.anchor = Some(anchor),
                    TrayPopupPhase::Visible => {
                        self.anchor = Some(anchor);
                        self.expires_at = Some(now + MAX_AGE);
                        self.leave_deadline = None;
                    }
                }
            }
            TrayHoverEvent::Move(anchor) => {
                if self.suppress_moves {
                    return false;
                }
                match self.phase {
                    TrayPopupPhase::Hidden => {}
                    TrayPopupPhase::Pending => self.anchor = Some(anchor),
                    TrayPopupPhase::Visible => {
                        self.anchor = Some(anchor);
                        self.expires_at = Some(now + MAX_AGE);
                    }
                }
            }
            TrayHoverEvent::Leave => match self.phase {
                // 还没显示就离开：立刻取消，不要闪一下再消失。
                TrayPopupPhase::Pending => self.hide(),
                TrayPopupPhase::Visible => {
                    // 重复的 Leave 不延长宽限，否则鼠标抖一下就多留 180ms。
                    if self.leave_deadline.is_none() {
                        self.leave_deadline = Some(now + LEAVE_GRACE);
                    }
                }
                TrayPopupPhase::Hidden => {}
            },
        }

        *self != before
    }

    /// 推进时间，返回是否发生了 Visible <-> 非 Visible 的转换。
    pub fn tick(&mut self, now: Instant, main_hidden: bool) -> bool {
        if !main_hidden {
            let was_visible = self.is_visible();
            self.hide();
            return was_visible;
        }

        match self.phase {
            TrayPopupPhase::Hidden => false,
            TrayPopupPhase::Pending => {
                let due = self.show_at.is_some_and(|show_at| now >= show_at);
                if due {
                    self.phase = TrayPopupPhase::Visible;
                    self.show_at = None;
                    self.expires_at = Some(now + MAX_AGE);
                    self.leave_deadline = None;
                    true
                } else {
                    false
                }
            }
            TrayPopupPhase::Visible => {
                let expired = self.expires_at.is_some_and(|at| now >= at)
                    || self.leave_deadline.is_some_and(|at| now >= at);
                if expired {
                    self.hide();
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 立即收起（用户点击图标或托盘菜单），并等待下一次 Enter 才允许重新显示。
    /// 返回原本是否处于 Pending/Visible。
    pub fn dismiss(&mut self) -> bool {
        let was_open = matches!(
            self.phase,
            TrayPopupPhase::Pending | TrayPopupPhase::Visible
        );
        self.hide();
        self.suppress_moves = true;
        was_open
    }

    /// 下一次需要调用 [`Self::tick`] 的时刻，供事件循环设置唤醒时间。
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            TrayPopupPhase::Hidden => None,
            TrayPopupPhase::Pending => self.show_at,
            TrayPopupPhase::Visible => match (self.leave_deadline, self.expires_at) {
                (Some(leave), Some(expires)) => Some(leave.min(expires)),
                (leave, expires) => leave.or(expires),
            },
        }
    }

    fn hide(&mut self) {
        self.phase = TrayPopupPhase::Hidden;
        self.anchor = None;
        self.show_at = None;
        self.expires_at = None;
        self.leave_deadline = None;
    }
}

/// 悬浮窗里的一行设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayPopupDevice {
    pub name: String,
    /// 只有真实百分比读数才有值；粗略等级不会编造出数字。
    pub percent: Option<u8>,
    pub coarse: Option<CoarseLevel>,
    pub charging: bool,
    pub unverified: bool,
    pub lowest: bool,
}

#[derive(Clone, Debug)]
pub struct TrayPopupModel {
    pub dark: bool,
    pub rows: Vec<TrayPopupDevice>,
    pub remainder: usize,
    pub total: usize,
    pub valid: usize,
    pub scanning: bool,
    pub stalled: bool,
    pub stopped: bool,
    pub last_scan: Option<Instant>,
}

impl TrayPopupModel {
    pub fn from_devices(
        dark: bool,
        devices: &[DeviceSnapshot],
        scanning: bool,
        stalled: bool,
        stopped: bool,
        last_scan: Option<Instant>,
    ) -> Self {
        // 只有「在线且确实读到电量」的设备才占一行；过期、休眠、未支持、
        // 读取失败和未知都不会伪装成一条有效读数。
        let mut candidates: Vec<&DeviceSnapshot> = devices
            .iter()
            .filter(|device| device.status.is_live() && device.battery.is_known())
            .collect();
        candidates.sort_by(|a, b| {
            let key = |device: &DeviceSnapshot| {
                (
                    device.percent().is_none(),
                    device.percent().unwrap_or(0),
                    device.name.to_lowercase(),
                )
            };
            key(a).cmp(&key(b))
        });

        // 最低电量标记只看真实百分比，且不信任未验证/缓存的来源。
        let lowest_id: Option<DeviceId> = candidates
            .iter()
            .find(|device| {
                device.percent().is_some()
                    && !matches!(
                        device.source.verification,
                        Confidence::Unverified | Confidence::Cached
                    )
            })
            .map(|device| device.id.clone());

        let rows: Vec<TrayPopupDevice> = candidates
            .iter()
            .take(MAX_ROWS)
            .map(|device| TrayPopupDevice {
                name: device.name.clone(),
                percent: device.percent(),
                coarse: match device.battery {
                    BatteryReading::Coarse { level } => Some(level),
                    _ => None,
                },
                // 未知充电状态不算「未充电」，所以只认明确的 true 或 Charging 状态。
                charging: device.battery.charging() == Some(true)
                    || device.status == BatteryStatus::Charging,
                unverified: device.source.verification == Confidence::Unverified,
                lowest: lowest_id.as_ref().is_some_and(|id| *id == device.id),
            })
            .collect();

        let valid = candidates.len();
        Self {
            dark,
            rows,
            remainder: valid.saturating_sub(MAX_ROWS),
            total: devices.len(),
            valid,
            scanning,
            stalled,
            stopped,
            last_scan,
        }
    }

    pub fn height(&self) -> f32 {
        BASE_HEIGHT + self.rows.len() as f32 * ROW_HEIGHT
    }

    pub fn scan_line(&self) -> String {
        if self.stopped {
            return "扫描服务已停止".to_owned();
        }
        if self.stalled {
            return "扫描超时 · 显示上次结果".to_owned();
        }
        if self.scanning {
            return "正在扫描…".to_owned();
        }
        match self.last_scan {
            Some(at) => {
                let elapsed = Instant::now().saturating_duration_since(at).as_secs();
                if elapsed < 60 {
                    format!("{elapsed} 秒前完成扫描")
                } else {
                    format!("{} 分钟前完成扫描", elapsed / 60)
                }
            }
            None => "尚未完成首次扫描".to_owned(),
        }
    }
}

/// 计算悬浮窗左上角（egui points）。
///
/// 全部用物理像素推演、最后除以 `pixels_per_point`：候选顺序是
/// 「图标上方 / 下方 / 右侧 / 左侧」，取第一个完整落在显示器工作区内的；都不完整时
/// 取可见面积最大的，再按工作区 clamp。工作区原点是真实的（可能是负数），
/// 所以这里绝不能把坐标 clamp 到 0。
pub fn place_popup(
    anchor: TrayAnchor,
    size_points: Vec2,
    monitor: MonitorGeometry,
    pixels_per_point: f32,
) -> Pos2 {
    let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        f64::from(pixels_per_point)
    } else {
        1.0
    };
    let width = f64::from(size_points.x.max(0.0)) * scale;
    let height = f64::from(size_points.y.max(0.0)) * scale;
    let gap = f64::from(ANCHOR_GAP) * scale;

    // 显示器几何理论上总是有序的；这里只是不信任外部数据。
    let (left, right) = if monitor.work_left <= monitor.work_right {
        (monitor.work_left, monitor.work_right)
    } else {
        (monitor.work_right, monitor.work_left)
    };
    let (top, bottom) = if monitor.work_top <= monitor.work_bottom {
        (monitor.work_top, monitor.work_bottom)
    } else {
        (monitor.work_bottom, monitor.work_top)
    };

    let centred_x = anchor.center_x() - width / 2.0;
    let centred_y = anchor.center_y() - height / 2.0;
    let above_y = anchor.y - gap - height;
    let below_y = anchor.y + anchor.height + gap;
    let right_x = anchor.x + anchor.width + gap;
    let left_x = anchor.x - gap - width;

    let candidates = [
        (centred_x, above_y),
        (centred_x, below_y),
        (right_x, centred_y),
        (left_x, centred_y),
    ];

    let fits = |x: f64, y: f64| x >= left && y >= top && x + width <= right && y + height <= bottom;
    let visible_area = |(x, y): (f64, f64)| {
        let visible_width = (x + width).min(right) - x.max(left);
        let visible_height = (y + height).min(bottom) - y.max(top);
        visible_width.max(0.0) * visible_height.max(0.0)
    };

    // 第一个完整可见的候选（上方优先）；都不行就取可见面积最大的那个。
    let chosen = match candidates.iter().copied().find(|(x, y)| fits(*x, *y)) {
        Some(candidate) => candidate,
        None => {
            let mut best = candidates[0];
            let mut best_area = visible_area(best);
            for candidate in candidates.iter().skip(1).copied() {
                let area = visible_area(candidate);
                if area > best_area {
                    best = candidate;
                    best_area = area;
                }
            }
            best
        }
    };

    let clamp = |value: f64, low: f64, high: f64| value.clamp(low, high.max(low));
    let mut x = clamp(chosen.0, left, right - width);
    let mut y = clamp(chosen.1, top, bottom - height);

    // clamp 有可能把悬浮窗推到图标上面；只要某个方向还放得下，就挪开而不是压住图标。
    let covers_anchor = |x: f64, y: f64| {
        x < anchor.x + anchor.width + gap
            && anchor.x - gap < x + width
            && y < anchor.y + anchor.height + gap
            && anchor.y - gap < y + height
    };
    if covers_anchor(x, y) {
        if above_y >= top && above_y + height <= bottom {
            y = above_y;
        } else if below_y >= top && below_y + height <= bottom {
            y = below_y;
        } else if left_x >= left && left_x + width <= right {
            x = left_x;
        } else if right_x >= left && right_x + width <= right {
            x = right_x;
        }
    }

    Pos2::new((x / scale) as f32, (y / scale) as f32)
}

/// 与 `theme::rich` 相同的字体族选择：粗体来自项目像素字体的 `bold` 家族。
/// 这里返回 [`FontId`] 是因为所有文字都直接画在 painter 上，落在固定像素栅格里，
/// 不参与 egui 的布局流。
fn text_font(size: f32, bold: bool) -> FontId {
    let family = if bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    FontId::new(size, family)
}

fn text(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    value: impl Into<String>,
    size: f32,
    color: Color32,
    bold: bool,
) {
    painter.text(pos, align, value.into(), text_font(size, bold), color);
}

fn line(painter: &Painter, left: f32, y: f32, width: f32, height: f32, color: Color32) {
    let rect = theme::snap_rect(
        painter.ctx().pixels_per_point(),
        Rect::from_min_size(Pos2::new(left, y), vec2(width, height)),
    );
    painter.rect_filled(rect, CornerRadius::ZERO, color);
}

/// 画一帧悬浮窗。调用方负责把窗口尺寸设成 [`TrayPopupModel::height`]。
pub fn show(ui: &mut Ui, model: &TrayPopupModel) {
    let p: Palette = theme::palette(model.dark);
    let ppp = ui.ctx().pixels_per_point();

    // Fill the whole viewport first: the 4pt margin around the card must not
    // expose the native window clear colour (black on some drivers).
    let window = theme::snap_rect(ppp, ui.max_rect());
    ui.painter()
        .rect_filled(window, CornerRadius::ZERO, p.card_solid);

    // 外层和主面板同一套：2px 硬边框 + 3px 像素投影，四角留白给投影。
    let outer = theme::snap_rect(ppp, ui.max_rect().shrink(4.0));
    {
        let painter = ui.painter();
        icons::draw_pixel_shadow(painter, outer, p.shadow);
        icons::pixel_frame(painter, outer, p.card_solid, p.card_border);
    }

    let inner = theme::snap_rect(ppp, outer.shrink(PAD));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.set_clip_rect(inner);
    let painter = child.painter().clone();

    let left = inner.left();
    let right = inner.right();
    let mut y = inner.top();

    // 标题行：品牌电量标 + 名称 + 设备总数。
    let mark_size = 20.0;
    let mark = Rect::from_min_size(
        Pos2::new(left, y + (TITLE_HEIGHT - mark_size) / 2.0),
        vec2(mark_size, mark_size),
    );
    icons::draw_battery_mark(&painter, mark, &p);
    text(
        &painter,
        Pos2::new(mark.right() + 8.0, y + TITLE_HEIGHT / 2.0),
        Align2::LEFT_CENTER,
        "AnyBattery",
        15.0,
        p.text,
        true,
    );
    text(
        &painter,
        Pos2::new(right, y + TITLE_HEIGHT / 2.0),
        Align2::RIGHT_CENTER,
        format!("{} 台", model.total),
        10.5,
        p.tertiary,
        false,
    );
    y += TITLE_HEIGHT + GAP;

    line(&painter, left, y, inner.width(), 2.0, p.card_border);
    y += 2.0 + GAP;

    // 汇总行：总设备数与真正有读数的条数分开报，避免「N 台设备」被读成都有电量。
    text(
        &painter,
        Pos2::new(left, y + SUMMARY_HEIGHT / 2.0),
        Align2::LEFT_CENTER,
        format!("{} 台设备 · {} 条有效读数", model.total, model.valid),
        10.5,
        p.secondary,
        false,
    );
    y += SUMMARY_HEIGHT + GAP;

    if model.rows.is_empty() {
        text(
            &painter,
            Pos2::new(left, y + ROW_HEIGHT / 2.0),
            Align2::LEFT_CENTER,
            "暂无有效电量读数",
            11.0,
            p.secondary,
            false,
        );
        y += ROW_HEIGHT;
    } else {
        for (index, row) in model.rows.iter().enumerate() {
            if index > 0 {
                line(&painter, left, y, inner.width(), 1.0, p.track);
            }
            draw_row(&child, &painter, &p, row, left, right, y);
            y += ROW_HEIGHT;
        }
    }

    if model.remainder > 0 {
        text(
            &painter,
            Pos2::new(left, y + REMAINDER_HEIGHT / 2.0),
            Align2::LEFT_CENTER,
            format!("另有 {} 台", model.remainder),
            10.0,
            p.tertiary,
            false,
        );
    }

    // 底部两行贴住内边距底边向上生长，中间多出来的空间留给设备行。
    let hint_y = inner.bottom() - HINT_BASELINE;
    text(
        &painter,
        Pos2::new(left, hint_y),
        Align2::LEFT_CENTER,
        "单击托盘图标打开面板",
        9.5,
        p.tertiary,
        false,
    );
    let scan_line = widgets::fit_text(&child, &model.scan_line(), 10.0, false, inner.width());
    text(
        &painter,
        Pos2::new(left, hint_y - HINT_LINE_HEIGHT),
        Align2::LEFT_CENTER,
        scan_line,
        10.0,
        p.tertiary,
        false,
    );

    // 「N 秒前完成扫描」这类文案需要每秒重画一次。
    ui.ctx().request_repaint_after(Duration::from_secs(1));
}

fn draw_row(
    ui: &Ui,
    painter: &Painter,
    p: &Palette,
    row: &TrayPopupDevice,
    left: f32,
    right: f32,
    top: f32,
) {
    let name_width = (right - left - 76.0).clamp(24.0, NAME_MAX_WIDTH);
    let name = widgets::fit_text(ui, &row.name, 12.0, true, name_width);
    text(
        painter,
        Pos2::new(left, top + ROW_NAME_Y),
        Align2::LEFT_CENTER,
        name,
        12.0,
        p.text,
        true,
    );

    let bar = Rect::from_min_size(
        Pos2::new(left, top + ROW_BAR_Y),
        vec2(BAR_WIDTH, BAR_HEIGHT),
    );
    match (row.percent, row.coarse) {
        (Some(level), _) => {
            let color = theme::battery_color_for_level(*p, level);
            text(
                painter,
                Pos2::new(right, top + ROW_NAME_Y),
                Align2::RIGHT_CENTER,
                format!("{level}%"),
                13.0,
                color,
                true,
            );
            widgets::pixel_battery(painter, p, bar, f32::from(level) / 100.0, color);
        }
        (None, Some(level)) => {
            text(
                painter,
                Pos2::new(right, top + ROW_NAME_Y),
                Align2::RIGHT_CENTER,
                level.label_zh(),
                13.0,
                p.secondary,
                true,
            );
            widgets::coarse_pixel_battery(painter, p, bar, level);
        }
        (None, None) => {}
    }

    // 徽标右对齐串联，顺序固定：最低 · 充电中 · 未验证。
    let mut badges: Vec<(&str, Color32)> = Vec::new();
    if row.lowest {
        badges.push(("最低", p.tertiary));
    }
    if row.charging {
        badges.push(("充电中", p.accent));
    }
    if row.unverified {
        badges.push(("未验证", p.orange));
    }
    if badges.is_empty() {
        return;
    }

    let font = text_font(10.0, false);
    let widths: Vec<f32> = badges
        .iter()
        .map(|(label, color)| {
            painter
                .layout_no_wrap((*label).to_owned(), font.clone(), *color)
                .size()
                .x
        })
        .collect();
    let separator = painter
        .layout_no_wrap("·".to_owned(), font.clone(), p.tertiary)
        .size()
        .x;
    let gaps = separator + 8.0;
    let total: f32 = widths.iter().sum::<f32>() + gaps * badges.len().saturating_sub(1) as f32;

    let badge_y = top + ROW_BADGE_Y;
    let mut x = right - total;
    for (index, ((label, color), width)) in badges.iter().zip(widths.iter()).enumerate() {
        text(
            painter,
            Pos2::new(x, badge_y),
            Align2::LEFT_CENTER,
            *label,
            10.0,
            *color,
            false,
        );
        x += width;
        if index + 1 < badges.len() {
            text(
                painter,
                Pos2::new(x + 4.0 + separator / 2.0, badge_y),
                Align2::CENTER_CENTER,
                "·",
                10.0,
                p.tertiary,
                false,
            );
            x += gaps;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anybattery_core::{
        BatterySource, Confidence, DeviceKind, DeviceSnapshot as Snapshot, Transport,
    };

    const MONITOR: MonitorGeometry = MonitorGeometry {
        work_left: 0.0,
        work_top: 0.0,
        work_right: 1920.0,
        work_bottom: 1040.0,
        scale: 1.0,
    };

    fn anchor(x: f64, y: f64) -> TrayAnchor {
        TrayAnchor::new(x, y, 16, 16)
    }

    fn device(
        name: &str,
        battery: BatteryReading,
        status: BatteryStatus,
        verification: Confidence,
    ) -> Snapshot {
        let mut snapshot = Snapshot::new(
            DeviceId::new("test", name),
            name,
            DeviceKind::Mouse,
            Transport::BluetoothClassic,
            BatterySource::new("test", verification, ""),
        );
        snapshot.battery = battery;
        snapshot.status = status;
        snapshot
    }

    fn percent_device(name: &str, level: u8, verification: Confidence) -> Snapshot {
        device(
            name,
            BatteryReading::percent(level, None),
            BatteryStatus::Available,
            verification,
        )
    }

    // ---- 状态机 ----

    #[test]
    fn enter_waits_for_the_delay_and_move_does_not_restart_it() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert!(!state.tick(t0, true));
        assert_eq!(state.next_deadline(), None);

        assert!(state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true));
        assert_eq!(state.phase(), TrayPopupPhase::Pending);
        assert_eq!(state.next_deadline(), Some(t0 + SHOW_DELAY));

        assert!(!state.tick(t0 + Duration::from_millis(249), true));
        assert_eq!(state.phase(), TrayPopupPhase::Pending);

        // 移动只跟随图标，不重置 show_at：显示时刻仍然是最初的 +250ms。
        assert!(state.on_event(
            TrayHoverEvent::Move(anchor(200.0, 100.0)),
            t0 + Duration::from_millis(200),
            true
        ));
        assert_eq!(state.anchor(), Some(anchor(200.0, 100.0)));
        assert_eq!(state.next_deadline(), Some(t0 + SHOW_DELAY));

        assert!(state.tick(t0 + SHOW_DELAY, true));
        assert_eq!(state.phase(), TrayPopupPhase::Visible);
        assert!(state.is_visible());
        assert_eq!(state.next_deadline(), Some(t0 + SHOW_DELAY + MAX_AGE));
    }

    #[test]
    fn leave_grace_outlives_a_short_exit_and_reentry_cancels_it() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();
        state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true);
        state.tick(t0 + SHOW_DELAY, true);
        let visible_at = t0 + SHOW_DELAY;

        assert!(state.on_event(TrayHoverEvent::Leave, visible_at, true));
        assert!(state.is_visible());
        assert_eq!(state.next_deadline(), Some(visible_at + LEAVE_GRACE));

        // 重复 Leave 不延长宽限。
        state.on_event(
            TrayHoverEvent::Leave,
            visible_at + Duration::from_millis(100),
            true,
        );
        assert_eq!(state.next_deadline(), Some(visible_at + LEAVE_GRACE));

        assert!(!state.tick(visible_at + Duration::from_millis(179), true));
        assert!(state.is_visible());

        // 宽限内回到图标：取消关闭，并按 MAX_AGE 重新计时。
        let back = visible_at + Duration::from_millis(170);
        assert!(state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), back, true));
        assert!(state.is_visible());
        assert_eq!(state.next_deadline(), Some(back + MAX_AGE));
        assert!(!state.tick(visible_at + LEAVE_GRACE, true));
        assert!(state.is_visible());
    }

    #[test]
    fn leave_while_pending_cancels_immediately() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();
        state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true);
        assert!(state.on_event(TrayHoverEvent::Leave, t0 + Duration::from_millis(10), true));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert_eq!(state.anchor(), None);
        assert_eq!(state.next_deadline(), None);
        assert!(!state.tick(t0 + SHOW_DELAY, true));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
    }

    #[test]
    fn dismiss_needs_a_fresh_enter_to_reopen() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();
        state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true);
        state.tick(t0 + SHOW_DELAY, true);
        assert!(state.is_visible());

        assert!(state.dismiss());
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert!(!state.dismiss());

        assert!(!state.on_event(
            TrayHoverEvent::Move(anchor(120.0, 100.0)),
            t0 + SHOW_DELAY + Duration::from_millis(10),
            true
        ));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert!(!state.tick(t0 + MAX_AGE * 2, true));

        let enter = t0 + SHOW_DELAY + Duration::from_millis(50);
        assert!(state.on_event(TrayHoverEvent::Enter(anchor(120.0, 100.0)), enter, true));
        assert_eq!(state.phase(), TrayPopupPhase::Pending);
        assert!(state.tick(enter + SHOW_DELAY, true));
        assert!(state.is_visible());
    }

    #[test]
    fn main_window_visible_keeps_the_popup_hidden_without_suppressing() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();

        assert!(!state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, false));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert!(!state.tick(t0, false));

        // 从 Visible 回到主窗口：立刻收起。
        state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true);
        state.tick(t0 + SHOW_DELAY, true);
        assert!(state.is_visible());
        assert!(state.tick(t0 + SHOW_DELAY + Duration::from_millis(10), false));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);

        // 主窗口收起后，普通 Enter 仍然可用（主窗口不该触发 suppress）。
        let later = t0 + Duration::from_secs(30);
        assert!(state.on_event(TrayHoverEvent::Enter(anchor(300.0, 200.0)), later, true));
        assert_eq!(state.phase(), TrayPopupPhase::Pending);
    }

    #[test]
    fn visible_hides_itself_after_max_age() {
        let t0 = Instant::now();
        let mut state = TrayPopupState::new();
        state.on_event(TrayHoverEvent::Enter(anchor(100.0, 100.0)), t0, true);
        state.tick(t0 + SHOW_DELAY, true);
        let visible_at = t0 + SHOW_DELAY;

        assert!(!state.tick(visible_at + MAX_AGE - Duration::from_millis(1), true));
        assert!(state.is_visible());
        assert!(state.tick(visible_at + MAX_AGE, true));
        assert_eq!(state.phase(), TrayPopupPhase::Hidden);
        assert_eq!(state.anchor(), None);
    }

    #[test]
    fn anchor_zero_size_falls_back_to_a_centred_icon_box() {
        let anchor = TrayAnchor::new(100.0, 200.0, 0, 0);
        assert_eq!(anchor.width, 16.0);
        assert_eq!(anchor.height, 16.0);
        assert_eq!(anchor.x, 92.0);
        assert_eq!(anchor.y, 192.0);
        assert_eq!(anchor.center_x(), 100.0);
        assert_eq!(anchor.center_y(), 200.0);

        let partial = TrayAnchor::new(10.0, 20.0, 0, 24);
        assert_eq!(partial.width, 16.0);
        assert_eq!(partial.height, 16.0);
        assert_eq!(partial.center_x(), 10.0);

        let real = TrayAnchor::new(5.0, 6.0, 24, 24);
        assert_eq!(real.center_x(), 17.0);
        assert_eq!(real.center_y(), 18.0);
    }

    // ---- 定位 ----

    fn assert_clear_of_anchor(anchor: TrayAnchor, pos: Pos2, size: Vec2) {
        let rect = Rect::from_min_size(pos, size);
        let anchor_rect = Rect::from_min_size(
            Pos2::new(anchor.x as f32, anchor.y as f32),
            vec2(anchor.width as f32, anchor.height as f32),
        );
        assert!(
            !rect.intersects(anchor_rect),
            "popup {rect:?} covers tray icon {anchor_rect:?}"
        );
    }

    #[test]
    fn bottom_taskbar_places_the_popup_above_the_icon() {
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);
        let anchor = anchor(940.0, 1010.0);
        let pos = place_popup(anchor, size, MONITOR, 1.0);
        // 紧凑高度 146：1010 - 8 - 146。
        assert_eq!(pos, Pos2::new(788.0, 856.0));
        assert_clear_of_anchor(anchor, pos, size);
        assert!(pos.y + size.y <= 1010.0 - ANCHOR_GAP + 1e-3);
    }

    #[test]
    fn top_taskbar_places_the_popup_below_the_icon() {
        let monitor = MonitorGeometry {
            work_top: 40.0,
            ..MONITOR
        };
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);
        let anchor = anchor(940.0, 40.0);
        let pos = place_popup(anchor, size, monitor, 1.0);
        assert_eq!(pos, Pos2::new(788.0, 64.0));
        assert_clear_of_anchor(anchor, pos, size);
        assert!(pos.y >= 40.0 + 16.0 + ANCHOR_GAP - 1e-3);
    }

    #[test]
    fn screen_edges_flip_to_the_side_with_room() {
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);

        // 右边缘：上方/下方都会溢出，右侧没有空间，所以只能放到图标左边。
        let right_anchor = anchor(1900.0, 500.0);
        let right_pos = place_popup(right_anchor, size, MONITOR, 1.0);
        // 居中偏移用紧凑高度 146：508 - 73。
        assert_eq!(right_pos, Pos2::new(1572.0, 435.0));
        assert_clear_of_anchor(right_anchor, right_pos, size);

        // 左边缘：上方溢出，只能放到图标右边。
        let left_anchor = anchor(10.0, 500.0);
        let left_pos = place_popup(left_anchor, size, MONITOR, 1.0);
        assert_eq!(left_pos, Pos2::new(34.0, 435.0));
        assert_clear_of_anchor(left_anchor, left_pos, size);
    }

    #[test]
    fn positions_are_clamped_inside_a_negative_origin_monitor() {
        // 副屏在工作区原点左侧：坐标全是负数，且工作区比悬浮窗更窄。
        let monitor = MonitorGeometry {
            work_left: -1920.0,
            work_top: 0.0,
            work_right: -1620.0,
            work_bottom: 1080.0,
            scale: 1.0,
        };
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);
        let anchor = anchor(-1700.0, 500.0);
        let pos = place_popup(anchor, size, monitor, 1.0);

        // 压住图标后改用上方候选：500 - 8 - 146。
        assert_eq!(pos, Pos2::new(-1920.0, 346.0));
        // 负原点必须原样保留，不能被 clamp 到 0。
        assert!(pos.x < 0.0);
        assert!(pos.x >= -1920.0);
        assert_clear_of_anchor(anchor, pos, size);
    }

    #[test]
    fn clamping_never_leaves_the_popup_on_top_of_the_icon() {
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);
        // 任务栏左端、右下角等最容易撞上图标的位置都试一遍。
        for anchor in [
            anchor(100.0, 1010.0),
            anchor(900.0, 1010.0),
            anchor(1850.0, 1010.0),
            anchor(4.0, 1010.0),
            anchor(1900.0, 4.0),
        ] {
            let pos = place_popup(anchor, size, MONITOR, 1.0);
            assert!(pos.x >= 0.0 && pos.y >= 0.0, "{anchor:?} -> {pos:?}");
            assert_clear_of_anchor(anchor, pos, size);
        }
    }

    #[test]
    fn placement_scales_with_pixels_per_point() {
        let monitor = MonitorGeometry {
            work_left: 0.0,
            work_top: 0.0,
            work_right: 3840.0,
            work_bottom: 2080.0,
            scale: 2.0,
        };
        let size = vec2(POPUP_WIDTH, BASE_HEIGHT);
        let corner = anchor(1880.0, 2020.0);
        let pos = place_popup(corner, size, monitor, 2.0);
        assert!((pos.x - 784.0).abs() < 0.01, "{pos:?}");
        assert!((pos.y - 856.0).abs() < 0.01, "{pos:?}");

        // 非法的 ppp 退化成 1.0，而不是产生 NaN。
        let fallback = place_popup(anchor(940.0, 1010.0), size, MONITOR, 0.0);
        assert_eq!(fallback, Pos2::new(788.0, 856.0));
        let nan = place_popup(anchor(940.0, 1010.0), size, MONITOR, f32::NAN);
        assert!(nan.x.is_finite() && nan.y.is_finite());
    }

    // ---- 模型 ----

    #[test]
    fn only_live_known_readings_take_a_row() {
        let devices = vec![
            device(
                "Stale",
                BatteryReading::percent(30, None),
                BatteryStatus::Stale,
                Confidence::Verified,
            ),
            device(
                "Asleep",
                BatteryReading::percent(40, None),
                BatteryStatus::Asleep,
                Confidence::Verified,
            ),
            device(
                "Unsupported",
                BatteryReading::Unknown,
                BatteryStatus::Unsupported,
                Confidence::Verified,
            ),
            device(
                "Errored",
                BatteryReading::Unknown,
                BatteryStatus::Error,
                Confidence::Verified,
            ),
            device(
                "UnknownBattery",
                BatteryReading::Unknown,
                BatteryStatus::Available,
                Confidence::Verified,
            ),
            percent_device("Live", 55, Confidence::Verified),
        ];
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        assert_eq!(model.total, 6);
        assert_eq!(model.valid, 1);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].name, "Live");
        assert_eq!(model.rows[0].percent, Some(55));
    }

    #[test]
    fn lowest_skips_unverified_and_cached_sources() {
        let devices = vec![
            percent_device("Unverified", 5, Confidence::Unverified),
            percent_device("Cached", 8, Confidence::Cached),
            percent_device("Community", 40, Confidence::Community),
        ];
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        assert_eq!(model.rows.len(), 3);
        assert_eq!(model.rows[0].name, "Unverified");
        assert!(model.rows[0].unverified);
        assert!(!model.rows[0].lowest);
        assert!(!model.rows[1].lowest);
        assert!(model.rows[2].lowest, "first trustworthy percent row wins");
    }

    #[test]
    fn coarse_rows_never_invent_a_percentage() {
        let devices = vec![
            device(
                "Coarse",
                BatteryReading::Coarse {
                    level: CoarseLevel::Low,
                },
                BatteryStatus::Available,
                Confidence::Coarse,
            ),
            percent_device("Percent", 60, Confidence::Verified),
        ];
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        assert_eq!(model.rows.len(), 2);
        // 有百分比的设备排在前面。
        assert_eq!(model.rows[0].name, "Percent");
        assert_eq!(model.rows[1].name, "Coarse");
        assert_eq!(model.rows[1].percent, None);
        assert_eq!(model.rows[1].coarse, Some(CoarseLevel::Low));
    }

    #[test]
    fn unknown_reading_never_becomes_zero_percent() {
        let devices = vec![device(
            "NoData",
            BatteryReading::Unknown,
            BatteryStatus::Available,
            Confidence::Verified,
        )];
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        assert!(model.rows.is_empty());
        assert_eq!(model.valid, 0);
        assert_eq!(model.height(), BASE_HEIGHT);
    }

    #[test]
    fn rows_are_sorted_and_capped_with_a_remainder() {
        let mut devices = Vec::new();
        for (name, level) in [
            ("Echo", 70u8),
            ("Alpha", 20),
            ("Delta", 50),
            ("charlie", 20),
            ("Bravo", 90),
            ("Foxtrot", 30),
        ] {
            devices.push(percent_device(name, level, Confidence::Verified));
        }
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        let names: Vec<&str> = model.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "charlie", "Foxtrot", "Delta"]);
        assert_eq!(model.rows.len(), MAX_ROWS);
        assert_eq!(model.remainder, 2);
        assert_eq!(model.valid, 6);
        assert_eq!(model.total, 6);
        assert_eq!(model.height(), BASE_HEIGHT + MAX_ROWS as f32 * ROW_HEIGHT);
        assert!(model.rows[0].lowest);
        assert!(!model.rows[1].lowest);
    }

    #[test]
    fn charging_is_only_reported_when_it_is_known() {
        let devices = vec![
            device(
                "UnknownCharging",
                BatteryReading::percent(50, None),
                BatteryStatus::Available,
                Confidence::Verified,
            ),
            device(
                "ChargingFlag",
                BatteryReading::percent(50, Some(true)),
                BatteryStatus::Available,
                Confidence::Verified,
            ),
            device(
                "ChargingStatus",
                BatteryReading::Coarse {
                    level: CoarseLevel::Medium,
                },
                BatteryStatus::Charging,
                Confidence::Coarse,
            ),
            device(
                "NotCharging",
                BatteryReading::percent(50, Some(false)),
                BatteryStatus::Available,
                Confidence::Verified,
            ),
        ];
        let model = TrayPopupModel::from_devices(false, &devices, false, false, false, None);
        let find = |name: &str| {
            model
                .rows
                .iter()
                .find(|row| row.name == name)
                .expect("row present")
        };
        assert!(!find("UnknownCharging").charging);
        assert!(find("ChargingFlag").charging);
        assert!(find("ChargingStatus").charging);
        assert!(!find("NotCharging").charging);
    }

    #[test]
    fn scan_line_covers_every_state() {
        let base = TrayPopupModel::from_devices(false, &[], false, false, false, None);
        assert_eq!(base.scan_line(), "尚未完成首次扫描");

        let scanning = TrayPopupModel {
            scanning: true,
            ..base.clone()
        };
        assert_eq!(scanning.scan_line(), "正在扫描…");

        let stalled = TrayPopupModel {
            scanning: true,
            stalled: true,
            ..base.clone()
        };
        assert_eq!(stalled.scan_line(), "扫描超时 · 显示上次结果");

        let stopped = TrayPopupModel {
            stalled: true,
            stopped: true,
            ..base.clone()
        };
        assert_eq!(stopped.scan_line(), "扫描服务已停止");

        let recent = TrayPopupModel {
            last_scan: Some(Instant::now() - Duration::from_secs(5)),
            ..base.clone()
        };
        assert_eq!(recent.scan_line(), "5 秒前完成扫描");

        let older = TrayPopupModel {
            last_scan: Some(Instant::now() - Duration::from_secs(90)),
            ..base.clone()
        };
        assert_eq!(older.scan_line(), "1 分钟前完成扫描");
    }

    // ---- 绘制 ----

    #[test]
    fn layout_budget_fits_every_row_count() {
        // 窗口高度由 height() 决定，内部栅格必须在这个高度里排得下，
        // 否则底部提示会压到最后一台设备上。
        // 紧凑栅格：BASE 146、ROW 32、PAD 10，标题 22 / 汇总 14 / 分隔 2 保持不变。
        assert_eq!(BASE_HEIGHT, 146.0);
        assert_eq!(ROW_HEIGHT, 32.0);
        assert_eq!(PAD, 10.0);
        let inner_height = |rows: usize| BASE_HEIGHT + rows as f32 * ROW_HEIGHT - 8.0 - 2.0 * PAD;
        let header_and_summary = TITLE_HEIGHT + GAP + 2.0 + GAP + SUMMARY_HEIGHT + GAP;
        // 底部两行提示：基线 7px、行距 14px，再加半行字高。
        let bottom = HINT_BASELINE + HINT_LINE_HEIGHT + 6.0;
        for rows in 0..=MAX_ROWS {
            let body = if rows == 0 {
                ROW_HEIGHT
            } else {
                rows as f32 * ROW_HEIGHT
                    + if rows == MAX_ROWS {
                        REMAINDER_HEIGHT
                    } else {
                        0.0
                    }
            };
            let required = header_and_summary + body + bottom;
            assert!(
                required <= inner_height(rows),
                "{rows} rows need {required}, only {} available",
                inner_height(rows)
            );
        }
    }

    /// 字体只能在 `Context::run()` 之后测量，所以先安静地跑一帧。
    fn measured_context() -> eframe::egui::Context {
        let ctx = eframe::egui::Context::default();
        theme::install_fonts(&ctx);
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |_ui| {});
        output.textures_delta.clear();
        ctx
    }

    #[test]
    fn row_text_bands_fit_the_compact_row() {
        let ctx = measured_context();
        let height = |size: f32, bold: bool| {
            ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap("电量".to_owned(), text_font(size, bold), Color32::WHITE)
                    .size()
                    .y
            })
        };
        let name = height(12.0, true);
        let badge = height(10.0, false);
        let name_top = ROW_NAME_Y - name / 2.0;
        let name_bottom = ROW_NAME_Y + name / 2.0;
        let badge_top = ROW_BADGE_Y - badge / 2.0;
        let badge_bottom = ROW_BADGE_Y + badge / 2.0;
        assert!(name_top >= 0.0, "名称从 {name_top} 开始，超出行顶");
        assert!(
            name_bottom <= badge_top,
            "名称底 {name_bottom} 压到徽标顶 {badge_top}"
        );
        assert!(
            badge_bottom <= ROW_HEIGHT,
            "徽标底 {badge_bottom} 溢出 {ROW_HEIGHT}px 行"
        );
        assert!(
            ROW_BAR_Y >= name_bottom,
            "电量条 {ROW_BAR_Y} 压到名称底 {name_bottom}"
        );
        assert!(ROW_BAR_Y + BAR_HEIGHT <= ROW_HEIGHT);
    }

    #[test]
    fn badges_never_reach_the_battery_bar() {
        let ctx = measured_context();
        let width = |text: &str| {
            ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(text.to_owned(), text_font(10.0, false), Color32::WHITE)
                    .size()
                    .x
            })
        };
        // draw_row 的最宽一串徽标：最低 · 充电中 · 未验证。
        let badges = ["最低", "充电中", "未验证"];
        let separator = width("·");
        let total = badges.iter().map(|label| width(label)).sum::<f32>()
            + (separator + 8.0) * (badges.len() - 1) as f32;
        let inner = POPUP_WIDTH - 8.0 - 2.0 * PAD;
        assert!(
            inner - total >= BAR_WIDTH,
            "徽标 {total}px 会压到 {BAR_WIDTH}px 电量条（内宽 {inner}）"
        );
    }

    #[test]
    fn show_paints_a_frame_for_both_themes() {
        let devices = vec![
            percent_device("低电量鼠标", 12, Confidence::Verified),
            device(
                "键盘",
                BatteryReading::percent(80, Some(true)),
                BatteryStatus::Charging,
                Confidence::Unverified,
            ),
            device(
                "粗略手柄",
                BatteryReading::Coarse {
                    level: CoarseLevel::Full,
                },
                BatteryStatus::Available,
                Confidence::Coarse,
            ),
        ];
        for dark in [false, true] {
            let ctx = eframe::egui::Context::default();
            theme::install_fonts(&ctx);
            let model = TrayPopupModel::from_devices(dark, &devices, false, false, false, None);
            for _ in 0..2 {
                let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
                    show(ui, &model);
                });
                assert!(!output.shapes.is_empty(), "nothing was painted");
                // 测试里没有 GPU 集成，纹理增量要显式消费掉（否则 Drop 会 debug_assert）。
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn show_handles_the_empty_and_remainder_states() {
        let ctx = eframe::egui::Context::default();
        theme::install_fonts(&ctx);

        let empty = TrayPopupModel::from_devices(false, &[], true, false, false, None);
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            show(ui, &empty);
        });
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();

        let mut devices = Vec::new();
        for index in 0..(MAX_ROWS + 3) {
            devices.push(percent_device(
                &format!("设备{index}"),
                10 + index as u8 * 10,
                Confidence::Verified,
            ));
        }
        let model = TrayPopupModel::from_devices(true, &devices, false, true, false, None);
        assert_eq!(model.remainder, 3);
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            show(ui, &model);
        });
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
    }
}
