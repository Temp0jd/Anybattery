use anybattery_core::{now_unix, BatteryStatus, DeviceSnapshot, Settings, Theme};
use anybattery_windows::{scan, ScanResult, system};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{
    self, Align, Align2, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, ViewportCommand, vec2,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::icons;
use crate::storage;
use crate::theme::{self, Palette};
use crate::tray::TrayHandle;
use crate::widgets;

pub enum UiCommand {
    Toggle,
    Show,
    Refresh,
    Menu(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Devices,
    Settings,
    Diagnostics,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Bluetooth,
    Wireless,
    Controllers,
}

impl Filter {
    fn index(self) -> usize {
        match self {
            Self::All => 0,
            Self::Bluetooth => 1,
            Self::Wireless => 2,
            Self::Controllers => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Bluetooth,
            2 => Self::Wireless,
            3 => Self::Controllers,
            _ => Self::All,
        }
    }

    fn matches(self, device: &DeviceSnapshot) -> bool {
        match self {
            Self::All => true,
            Self::Bluetooth => device.transport.is_bluetooth(),
            Self::Wireless => device.transport == anybattery_core::Transport::Usb24g,
            Self::Controllers => device.kind == anybattery_core::DeviceKind::Controller,
        }
    }
}

struct DeviceService {
    requests: Sender<Settings>,
    results: Receiver<ScanResult>,
}

impl DeviceService {
    fn start() -> Self {
        let (requests, request_rx) = unbounded::<Settings>();
        let (result_tx, results) = unbounded::<ScanResult>();
        std::thread::Builder::new()
            .name("anybattery-scan".to_owned())
            .spawn(move || {
                while let Ok(settings) = request_rx.recv() {
                    let result = scan(&settings);
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self { requests, results }
    }
}

pub struct AnyBatteryApp {
    settings: Settings,
    devices: Vec<DeviceSnapshot>,
    diagnostics: Vec<String>,
    page: Page,
    filter: Filter,
    scanning: bool,
    last_scan: Option<Instant>,
    service: DeviceService,
    commands: Receiver<UiCommand>,
    tray: Option<TrayHandle>,
    dark: bool,
    applied_dark: Option<bool>,
    applied_animations: Option<bool>,
    last_theme_check: Instant,
    hidden: bool,
    quitting: bool,
    toast: Option<(String, Instant)>,
    counts: (usize, usize, usize),
    alert: Option<String>,
}

impl AnyBatteryApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        commands: Receiver<UiCommand>,
        tray: Option<TrayHandle>,
        wake: Arc<Mutex<Option<egui::Context>>>,
    ) -> Self {
        *wake.lock().unwrap() = Some(cc.egui_ctx.clone());
        let settings = storage::load();
        let dark = theme::resolve_dark(settings.theme);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_visuals(&cc.egui_ctx, theme::palette(dark), settings.animations);
        if let Some(tray) = &tray {
            tray.autostart.set_checked(settings.autostart);
        }
        let service = DeviceService::start();
        let _ = service.requests.send(settings.clone());
        Self {
            settings,
            devices: Vec::new(),
            diagnostics: vec!["正在扫描设备…".to_owned()],
            page: Page::Devices,
            filter: Filter::All,
            scanning: true,
            last_scan: None,
            service,
            commands,
            tray,
            dark,
            applied_dark: Some(dark),
            applied_animations: Some(true),
            last_theme_check: Instant::now(),
            hidden: false,
            quitting: false,
            toast: None,
            counts: (0, 0, 0),
            alert: None,
        }
    }

    fn request_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        if self.service.requests.send(self.settings.clone()).is_err() {
            self.scanning = false;
            self.toast("扫描线程已停止".to_owned());
        }
    }

    fn poll_results(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.service.results.try_recv() {
            self.scanning = false;
            self.last_scan = Some(Instant::now());
            self.devices = result.devices;
            let mut diagnostics = Vec::with_capacity(result.diagnostics.len() + 2);
            diagnostics.push(format!(
                "扫描完成：{} 台设备，{} 个蓝牙节点，{} 个 HID 接口",
                self.devices.len(),
                result.bluetooth_count,
                result.hid_count
            ));
            diagnostics.extend(result.diagnostics);
            self.diagnostics = diagnostics;
            self.update_counts();
            self.update_tray();
            ctx.request_repaint();
        }
    }

    fn update_counts(&mut self) {
        let mut bluetooth = 0;
        let mut wireless = 0;
        let mut controllers = 0;
        for device in &self.devices {
            if device.transport.is_bluetooth() {
                bluetooth += 1;
            }
            if device.transport == anybattery_core::Transport::Usb24g {
                wireless += 1;
            }
            if device.kind == anybattery_core::DeviceKind::Controller {
                controllers += 1;
            }
        }
        self.counts = (bluetooth, wireless, controllers);
        self.alert = self
            .devices
            .iter()
            .filter(|device| device.status == BatteryStatus::Available)
            .filter_map(|device| device.percent().map(|level| (level, device)))
            .filter(|(level, _)| *level <= self.settings.low_threshold)
            .min_by_key(|(level, _)| *level)
            .map(|(level, device)| format!("{} 电量低（{}%），建议充电", device.name, level));
    }

    fn update_tray(&mut self) {
        let Some(tray) = &self.tray else {
            return;
        };
        let live: Vec<&DeviceSnapshot> = self
            .devices
            .iter()
            .filter(|device| device.status.is_live() && device.percent().is_some())
            .collect();
        let lowest = live.iter().min_by_key(|device| device.percent().unwrap_or(255));
        let tooltip = match lowest {
            Some(device) => format!(
                "AnyBattery · {} 台设备 · {} {}%",
                self.devices.len(),
                widgets::truncate(&device.name, 18),
                device.percent().unwrap_or(0)
            ),
            None => format!("AnyBattery · {} 台设备", self.devices.len()),
        };
        let _ = tray.icon.set_tooltip(Some(tooltip));
        if let Some(device) = lowest {
            if let Some(icon) = icons::tray_icon(device.percent(), device.battery.charging() == Some(true)) {
                let _ = tray.icon.set_icon(Some(icon));
            }
        }
    }

    fn toast(&mut self, message: String) {
        self.toast = Some((message, Instant::now()));
    }

    fn save_settings(&mut self) {
        if !storage::save(&self.settings) {
            self.toast(format!("无法保存设置到 {}", storage::data_dir().display()));
        }
        if let Some(tray) = &self.tray {
            tray.autostart.set_checked(self.settings.autostart);
        }
        self.applied_dark = None;
    }

    fn process_commands(&mut self, ctx: &egui::Context) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                UiCommand::Show => self.show(ctx),
                UiCommand::Toggle => {
                    if self.hidden {
                        self.show(ctx);
                    } else {
                        self.hide(ctx);
                    }
                }
                UiCommand::Refresh => self.request_scan(),
                UiCommand::Menu(id) => match id.as_str() {
                    "show" => self.show(ctx),
                    "refresh" => {
                        self.show(ctx);
                        self.request_scan();
                    }
                    "settings" => {
                        self.page = Page::Settings;
                        self.show(ctx);
                    }
                    "diagnostics" => {
                        self.page = Page::Diagnostics;
                        self.show(ctx);
                    }
                    "autostart" => {
                        self.settings.autostart = !self.settings.autostart;
                        if !system::set_autostart(self.settings.autostart) {
                            self.settings.autostart = !self.settings.autostart;
                            self.toast("无法修改开机自启（注册表被拒绝）".to_owned());
                        }
                        self.save_settings();
                    }
                    "quit" => {
                        self.quitting = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    _ => {}
                },
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.hidden = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    fn maybe_auto_scan(&mut self) {
        if self.scanning {
            return;
        }
        let due = self
            .last_scan
            .map(|last| last.elapsed() >= self.settings.poll_interval())
            .unwrap_or(true);
        if due {
            self.request_scan();
        }
    }

    fn maybe_refresh_theme(&mut self, ctx: &egui::Context) {
        let mut dark = self.dark;
        if self.settings.theme == Theme::System && self.last_theme_check.elapsed() > Duration::from_secs(2) {
            self.last_theme_check = Instant::now();
            dark = theme::resolve_dark(self.settings.theme);
        }
        if self.applied_dark != Some(dark) || self.applied_animations != Some(self.settings.animations) {
            self.dark = dark;
            self.applied_dark = Some(dark);
            self.applied_animations = Some(self.settings.animations);
            theme::apply_visuals(ctx, theme::palette(dark), self.settings.animations);
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(if dark {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            }));
        }
    }

    fn paint_background(&self, ui: &mut Ui, p: &Palette) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg);
        let strip = Rect::from_min_size(rect.min, vec2(rect.width(), 112.0));
        painter.rect_filled(strip, CornerRadius::ZERO, p.bg_glow);
        // Dithered seam, like a printed cartridge label.
        let mut x = rect.left() + 4.0;
        while x < rect.right() {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, strip.bottom() - 4.0), vec2(2.0, 2.0)),
                CornerRadius::ZERO,
                p.card_border.gamma_multiply(0.45),
            );
            x += 6.0;
        }
        painter.rect_stroke(
            rect.shrink(1.0),
            CornerRadius::ZERO,
            Stroke::new(2.0, p.card_border),
            StrokeKind::Inside,
        );
    }

    fn header(&mut self, ui: &mut Ui, p: &Palette) {
        let width = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
        let mut right = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    Pos2::new(bar.right() - 78.0, bar.top()),
                    vec2(78.0, bar.height()),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        if widgets::icon_button(&mut right, p, widgets::Glyph::Settings, "设置").clicked() {
            self.page = if self.page == Page::Settings { Page::Devices } else { Page::Settings };
        }
        if widgets::icon_button(&mut right, p, widgets::Glyph::Refresh, "立即刷新").clicked() {
            self.request_scan();
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.page != Page::Devices
                && widgets::icon_button(ui, p, widgets::Glyph::Back, "返回").clicked()
            {
                self.page = Page::Devices;
            }
            let (title, subtitle) = match self.page {
                Page::Devices => (
                    "电池".to_owned(),
                    if self.devices.is_empty() {
                        if self.scanning {
                            "正在扫描设备…".to_owned()
                        } else {
                            "未发现带电池的设备".to_owned()
                        }
                    } else {
                        format!(
                            "{} 台设备 · 蓝牙 {} · 2.4G {} · 手柄 {}",
                            self.devices.len(),
                            self.counts.0,
                            self.counts.1,
                            self.counts.2
                        )
                    },
                ),
                Page::Settings => ("设置".to_owned(), "刷新、通知与外观".to_owned()),
                Page::Diagnostics => ("诊断".to_owned(), "扫描结果与设备接口".to_owned()),
            };
            ui.vertical(|ui| {
                ui.label(theme::rich(title, 24.0, p.text, true));
                ui.add_space(1.0);
                ui.label(theme::rich(subtitle, 12.0, p.secondary, false));
            });
        });
    }

    fn devices_page(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(8.0);
        if self.settings.notify_low_battery {
            if let Some(alert) = &self.alert {
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                icons::draw_pixel_shadow(ui.painter(), rect, p.shadow);
                icons::pixel_frame(ui.painter(), rect, p.orange.gamma_multiply(0.18), p.card_border);
                ui.painter().rect_filled(
                    Rect::from_min_size(Pos2::new(rect.left() + 10.0, rect.center().y - 4.0), vec2(8.0, 8.0)),
                    CornerRadius::ZERO,
                    p.orange,
                );
                ui.painter().text(
                    Pos2::new(rect.left() + 28.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    widgets::truncate(alert, 40),
                    FontId::new(12.0, egui::FontFamily::Proportional),
                    p.orange,
                );
                ui.add_space(8.0);
            }
        }
        let labels = ["全部", "蓝牙", "2.4G", "手柄"];
        if let widgets::SegmentResult::Selected(index) =
            widgets::segment(ui, p, &labels, self.filter.index())
        {
            self.filter = Filter::from_index(index);
        }
        ui.add_space(10.0);
        let list_height = (ui.available_height() - 50.0).max(80.0);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let now = now_unix();
                let filtered: Vec<&DeviceSnapshot> = self
                    .devices
                    .iter()
                    .filter(|device| self.filter.matches(device))
                    .collect();
                if filtered.is_empty() {
                    empty_state(ui, p, self.scanning);
                } else {
                    for device in filtered {
                        widgets::device_card(ui, p, device, now);
                        ui.add_space(8.0);
                    }
                }
                ui.add_space(4.0);
            });
        ui.add_space(2.0);
        if widgets::footer(ui, p, self.settings.poll_secs, self.last_scan, self.scanning).clicked() {
            self.page = Page::Diagnostics;
        }
    }

    fn settings_page(&mut self, ui: &mut Ui, p: &Palette) {
        let available = ui.available_height() - 12.0;
        egui::ScrollArea::vertical()
            .max_height(available)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(10.0);
                widgets::section_title(ui, p, "刷新");
                widgets::group(ui, p, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(theme::rich("刷新间隔", 13.5, p.text, false));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(10.0);
                            ui.label(theme::rich(
                                format_interval(self.settings.poll_secs),
                                12.5,
                                p.secondary,
                                true,
                            ));
                        });
                    });
                    ui.spacing_mut().slider_width = (ui.available_width() - 28.0).max(120.0);
                    let mut poll = self.settings.poll_secs as f32;
                    let slider = ui.add(
                        egui::Slider::new(&mut poll, 15.0..=1800.0)
                            .logarithmic(true)
                            .show_value(false),
                    );
                    if slider.changed() {
                        self.settings.poll_secs = poll.round() as u64;
                        self.settings.validate();
                        self.save_settings();
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        for (label, value) in [("30 秒", 30u64), ("1 分钟", 60), ("5 分钟", 300), ("10 分钟", 600)] {
                            if widgets::pill_button(ui, p, label, self.settings.poll_secs == value).clicked() {
                                self.settings.poll_secs = value;
                                self.save_settings();
                            }
                        }
                    });
                    ui.add_space(8.0);
                });

                ui.add_space(14.0);
                widgets::section_title(ui, p, "通知");
                widgets::group(ui, p, |ui| {
                    let mut value = self.settings.notify_low_battery;
                    let mut changed = false;
                    widgets::settings_row(ui, p, "低电量提醒", "面板顶部与托盘图标变化", |ui| {
                        changed = widgets::toggle(ui, p, &mut value);
                    });
                    if changed {
                        self.settings.notify_low_battery = value;
                        self.save_settings();
                    }
                });

                ui.add_space(14.0);
                widgets::section_title(ui, p, "外观");
                widgets::group(ui, p, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(theme::rich("主题", 13.5, p.text, false));
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let selected = match self.settings.theme {
                            Theme::System => 0,
                            Theme::Light => 1,
                            Theme::Dark => 2,
                        };
                        let width = (ui.available_width() - 10.0).max(120.0);
                        let (seg_rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                        let mut child = ui.new_child(
                            UiBuilder::new()
                                .max_rect(seg_rect)
                                .layout(Layout::top_down(Align::LEFT)),
                        );
                        if let widgets::SegmentResult::Selected(index) =
                            widgets::segment(&mut child, p, &["跟随系统", "浅色", "深色"], selected)
                        {
                            self.settings.theme = match index {
                                1 => Theme::Light,
                                2 => Theme::Dark,
                                _ => Theme::System,
                            };
                            self.dark = theme::resolve_dark(self.settings.theme);
                            self.applied_dark = None;
                            self.save_settings();
                        }
                    });
                    ui.add_space(8.0);
                    let mut animations = self.settings.animations;
                    let mut changed = false;
                    widgets::settings_row(ui, p, "动画", "面板与电量的过渡效果", |ui| {
                        changed = widgets::toggle(ui, p, &mut animations);
                    });
                    if changed {
                        self.settings.animations = animations;
                        self.save_settings();
                    }
                });

                ui.add_space(14.0);
                widgets::section_title(ui, p, "系统");
                widgets::group(ui, p, |ui| {
                    let mut autostart = self.settings.autostart;
                    let mut changed = false;
                    widgets::settings_row(ui, p, "开机自启", "登录 Windows 后自动运行", |ui| {
                        changed = widgets::toggle(ui, p, &mut autostart);
                    });
                    if changed {
                        if system::set_autostart(autostart) {
                            self.settings.autostart = autostart;
                            self.save_settings();
                        } else {
                            self.toast("无法修改开机自启（注册表被拒绝）".to_owned());
                        }
                    }
                    let mut show_unknown = self.settings.show_unknown;
                    let mut changed = false;
                    widgets::settings_row(ui, p, "显示无电池设备", "没有电量或未匹配协议时也列出", |ui| {
                        changed = widgets::toggle(ui, p, &mut show_unknown);
                    });
                    if changed {
                        self.settings.show_unknown = show_unknown;
                        self.save_settings();
                        self.request_scan();
                    }
                });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if widgets::pill_button(ui, p, "打开配置目录", false).clicked() {
                        let _ = std::process::Command::new("explorer")
                            .arg(storage::data_dir())
                            .spawn();
                    }
                    if widgets::pill_button(ui, p, "恢复默认", false).clicked() {
                        self.settings = Settings::default();
                        system::set_autostart(false);
                        self.save_settings();
                        self.request_scan();
                    }
                });
                ui.add_space(18.0);
            });
    }

    fn diagnostics_page(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::pill_button(ui, p, "刷新", false).clicked() {
                self.request_scan();
            }
            if widgets::pill_button(ui, p, "复制诊断信息", false).clicked() {
                ui.ctx().copy_text(self.diagnostics.join("\n"));
                self.toast("诊断信息已复制".to_owned());
            }
            if widgets::pill_button(ui, p, "打开配置目录", false).clicked() {
                let _ = std::process::Command::new("explorer")
                    .arg(storage::data_dir())
                    .spawn();
            }
        });
        ui.add_space(8.0);
        ui.label(theme::rich(
            format!(
                "设备 {} · 蓝牙 {} · 2.4G {} · 手柄 {} · 每 {} 秒刷新",
                self.devices.len(),
                self.counts.0,
                self.counts.1,
                self.counts.2,
                self.settings.poll_secs
            ),
            10.5,
            p.tertiary,
            false,
        ));
        ui.add_space(6.0);
        let height = (ui.available_height() - 12.0).max(80.0);
        egui::Frame::new()
            .fill(p.card)
            .stroke(Stroke::new(2.0, p.card_border))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(height - 24.0)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for line in &self.diagnostics {
                            ui.label(
                                RichText::new(line)
                                    .size(12.0)
                                    .color(p.secondary)
                                    .monospace(),
                            );
                        }
                    });
            });
    }

    fn paint_toast(&self, ui: &mut Ui, p: &Palette) {
        let Some((message, created)) = &self.toast else {
            return;
        };
        if created.elapsed() > Duration::from_secs(3) {
            return;
        }
        let rect = ui.max_rect();
        let text_width = message.chars().count() as f32 * 12.0;
        let width = text_width.min(rect.width() - 60.0).max(120.0);
        let toast = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.bottom() - 58.0),
            vec2(width, 32.0),
        );
        icons::draw_pixel_shadow(ui.painter(), toast, p.shadow);
        icons::pixel_frame(ui.painter(), toast, p.card_solid, p.card_border);
        ui.painter().text(
            toast.center(),
            Align2::CENTER_CENTER,
            widgets::truncate(message, 32),
            FontId::new(12.0, egui::FontFamily::Proportional),
            p.text,
        );
    }
}

impl eframe::App for AnyBatteryApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let p = theme::palette(self.dark);
        [
            p.bg.r() as f32 / 255.0,
            p.bg.g() as f32 / 255.0,
            p.bg.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_commands(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        if self.quitting {
            return;
        }
        self.poll_results(ctx);
        self.maybe_auto_scan();
        self.maybe_refresh_theme(ctx);
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.quitting {
            return;
        }
        let p = theme::palette(self.dark);
        self.paint_background(ui, &p);
        let rect = ui.max_rect();
        let content = Rect::from_min_max(
            Pos2::new(rect.left() + 20.0, rect.top() + 14.0),
            Pos2::new(rect.right() - 20.0, rect.bottom() - 12.0),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::LEFT)),
        );
        child.set_width(content.width());
        self.header(&mut child, &p);
        match self.page {
            Page::Devices => self.devices_page(&mut child, &p),
            Page::Settings => self.settings_page(&mut child, &p),
            Page::Diagnostics => self.diagnostics_page(&mut child, &p),
        }
        self.paint_toast(&mut child, &p);
    }
}

fn empty_state(ui: &mut Ui, p: &Palette, scanning: bool) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        icons::draw_pixel_shadow(ui.painter(), rect, p.shadow);
        icons::pixel_frame(ui.painter(), rect, p.card, p.card_border);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "?",
            FontId::new(30.0, egui::FontFamily::Name("bold".into())),
            p.tertiary,
        );
        ui.add_space(14.0);
        ui.label(theme::rich(
            if scanning { "正在扫描设备…" } else { "未发现带电池的设备" },
            18.0,
            p.text,
            true,
        ));
        ui.add_space(4.0);
        ui.label(theme::rich(
            "插入蓝牙或 2.4G 设备；有线设备不会显示",
            12.0,
            p.secondary,
            false,
        ));
    });
}

fn format_interval(seconds: u64) -> String {
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
