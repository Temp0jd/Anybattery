use crate::icons;
use crate::protocol_wizard::{ProtocolWizard, WizardAction};
use crate::storage;
use crate::theme::{self, Palette};
use crate::tray::TrayHandle;
use crate::tray_popup::{TrayHoverEvent, TrayPopupModel, TrayPopupState};
use crate::widgets;
use anybattery_core::{
    BatteryStatus, DeviceKind, DeviceSnapshot, Settings, Theme, UserProfileCatalog, now_unix,
};
use anybattery_windows::{
    HidInterface, ScanEvent, ScanResult, monitor, scan_with_profiles_and_progress, system,
};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use eframe::egui::{
    self, Align, Align2, CornerRadius, CursorIcon, FontId, Id, Layout, PointerButton, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, ViewportCommand, vec2,
};
use std::cmp::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub enum UiCommand {
    Toggle,
    Show,
    Menu(String),
    TrayHover(TrayHoverEvent),
    DismissTrayPopup,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Devices,
    Settings,
    Diagnostics,
    ProtocolWizard,
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
const SCAN_STALL_TIMEOUT: Duration = Duration::from_secs(20);

const DEVICE_ICON_CHOICES: [DeviceKind; 8] = [
    DeviceKind::Mouse,
    DeviceKind::Keyboard,
    DeviceKind::Headset,
    DeviceKind::Earbuds,
    DeviceKind::Speaker,
    DeviceKind::Controller,
    DeviceKind::Pen,
    DeviceKind::Unknown,
];

struct ScanRequest {
    id: u64,
    settings: Settings,
    profiles: Arc<UserProfileCatalog>,
}

enum ScanMessage {
    Progress(u64),
    Partial {
        id: u64,
        devices: Vec<DeviceSnapshot>,
    },
    Finished {
        id: u64,
        result: ScanResult,
    },
}

struct DeviceService {
    requests: Sender<ScanRequest>,
    results: Receiver<ScanMessage>,
}
impl DeviceService {
    fn start() -> Self {
        let (requests, request_rx) = unbounded::<ScanRequest>();
        let (result_tx, results) = unbounded::<ScanMessage>();
        std::thread::Builder::new()
            .name("anybattery-scan".to_owned())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let id = request.id;
                    let progress_tx = result_tx.clone();
                    let result = {
                        let mut on_event = |event: ScanEvent| {
                            let message = match event {
                                ScanEvent::Progress => ScanMessage::Progress(id),
                                ScanEvent::Partial(devices) => ScanMessage::Partial { id, devices },
                            };
                            let _ = progress_tx.send(message);
                        };
                        scan_with_profiles_and_progress(
                            &request.settings,
                            &request.profiles,
                            &mut on_event,
                        )
                    };
                    if result_tx
                        .send(ScanMessage::Finished { id, result })
                        .is_err()
                    {
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
    /// Monotonic id for the request currently allowed to update the UI.
    active_scan_id: Option<u64>,
    next_scan_id: u64,
    /// Last progress/partial message from the active scan; drives the stall watchdog.
    last_progress: Instant,
    /// A scan exceeded its idle-progress budget; a manual refresh restarts the worker.
    scan_stalled: bool,
    /// A scan was requested while another one was running; run exactly one more when it ends.
    rescan_pending: bool,
    /// Set when the scan worker or its result channel has failed. Further work is suppressed
    /// so a dead service cannot trigger a retry loop or a per-frame error toast.
    scan_service_stopped: bool,
    /// True while the refresh-interval slider is being dragged, so the config is written once
    /// at the end of the gesture instead of on every pixel.
    poll_dragging: bool,
    last_scan: Option<Instant>,
    service: DeviceService,
    commands: Receiver<UiCommand>,
    tray: Option<TrayHandle>,
    dark: bool,
    applied_dark: Option<bool>,
    applied_animations: Option<bool>,
    animations: bool,
    last_theme_check: Instant,
    hidden: bool,
    quitting: bool,
    toast: Option<(String, Instant)>,
    counts: (usize, usize, usize),
    alert: Option<String>,
    /// Last icon tuple pushed to the tray, to avoid shell churn every scan.
    last_tray: Option<(Option<u8>, bool)>,
    tray_popup: TrayPopupState,
    profiles: Arc<UserProfileCatalog>,
    profile_store: storage::ProfileStore,
    profile_diagnostics: Vec<String>,
    hid_interfaces: Vec<HidInterface>,
    wizard: ProtocolWizard,
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
        let animations = settings.animations;
        let service = DeviceService::start();
        let profile_store = storage::ProfileStore::new();
        let profile_load = profile_store.load();
        let profiles = Arc::new(profile_load.catalog);
        let profile_diagnostics = profile_load.diagnostics;
        let mut app = Self {
            settings,
            devices: Vec::new(),
            diagnostics: vec!["正在扫描设备…".to_owned()],
            page: Page::Devices,
            filter: Filter::All,
            scanning: false,
            active_scan_id: None,
            next_scan_id: 0,
            last_progress: Instant::now(),
            scan_stalled: false,
            rescan_pending: false,
            scan_service_stopped: false,
            poll_dragging: false,
            last_scan: None,
            service,
            commands,
            tray,
            dark,
            applied_dark: Some(dark),
            applied_animations: Some(animations),
            animations,
            last_theme_check: Instant::now(),
            hidden: false,
            quitting: false,
            toast: None,
            counts: (0, 0, 0),
            alert: None,
            last_tray: None,
            tray_popup: TrayPopupState::new(),
            profiles,
            profile_store,
            profile_diagnostics,
            hid_interfaces: Vec::new(),
            wizard: ProtocolWizard::new(),
        };
        // The initial scan request is the first thing the worker thread receives.
        app.start_scan();
        app.update_tray();
        app
    }
    /// Refresh now. While a scan is already running this is a no-op, so an impatient click can
    /// never stack more expensive HID scans. Display-setting changes use queue_rescan.
    fn request_scan(&mut self) {
        if self.wizard.is_capturing() {
            self.toast("被动捕获进行中，等捕获结束后再刷新".to_owned());
            return;
        }
        if self.scan_stalled {
            // The old worker may still be blocked in a synchronous HID call.
            // Give the user an explicit retry that abandons its channel.
            self.restart_scan_service();
            return;
        }
        if self.scan_service_stopped {
            self.toast("扫描线程已停止，请重新启动应用".to_owned());
            return;
        }
        if self.scanning {
            return;
        }
        self.start_scan();
    }
    /// Record at most one follow-up scan for a change that affects what the scan must return
    /// (for example "show devices without a battery"). When idle it starts immediately.
    fn queue_rescan(&mut self) {
        if self.wizard.is_capturing() {
            return;
        }
        if self.scan_stalled {
            self.restart_scan_service();
            return;
        }
        if self.scan_service_stopped {
            self.toast("扫描线程已停止，请重新启动应用".to_owned());
            return;
        }
        if self.scanning {
            self.rescan_pending = true;
        } else {
            self.start_scan();
        }
    }
    fn start_scan(&mut self) {
        self.next_scan_id = self.next_scan_id.wrapping_add(1);
        let id = self.next_scan_id;
        let request = ScanRequest {
            id,
            settings: self.settings.clone(),
            profiles: self.profiles.clone(),
        };
        match self.service.requests.send(request) {
            Ok(()) => {
                self.scanning = true;
                self.scan_stalled = false;
                self.active_scan_id = Some(id);
                self.last_progress = Instant::now();
            }
            Err(_) => self.mark_scan_service_stopped("发送扫描请求失败"),
        }
    }
    /// Abandon a stalled worker. The old thread may still be blocked in a
    /// synchronous HID call, but its result channel is dropped, so it cannot
    /// update this UI; a new worker gets a fresh channel and scan id.
    fn restart_scan_service(&mut self) {
        self.service = DeviceService::start();
        self.scan_service_stopped = false;
        self.scan_stalled = false;
        self.scanning = false;
        self.rescan_pending = false;
        self.active_scan_id = None;
        self.start_scan();
    }
    fn mark_scan_stalled(&mut self) {
        if self.scan_stalled {
            return;
        }
        self.scan_stalled = true;
        self.scanning = false;
        self.rescan_pending = false;
        self.toast("扫描超时：某个设备没有响应，点击刷新重试".to_owned());
        self.diagnostics.retain(|line| line != "正在扫描设备…");
        self.diagnostics.insert(
            0,
            format!(
                "扫描超时：{} 秒内没有进度；同步 HID 调用可能被驱动阻塞",
                SCAN_STALL_TIMEOUT.as_secs()
            ),
        );
    }
    fn check_scan_watchdog(&mut self) {
        if self.scanning && self.last_progress.elapsed() > SCAN_STALL_TIMEOUT {
            self.mark_scan_stalled();
        }
    }
    /// Leave the scanning state once, report it through the existing Chinese toast/diagnostics
    /// style, and stay stopped: no automatic or per-frame retry of a dead worker.
    fn mark_scan_service_stopped(&mut self, detail: &str) {
        if self.scan_service_stopped {
            return;
        }
        self.scan_service_stopped = true;
        self.scanning = false;
        self.scan_stalled = false;
        self.rescan_pending = false;
        self.active_scan_id = None;
        self.toast("扫描线程已停止，无法继续刷新设备".to_owned());
        self.diagnostics.retain(|line| line != "正在扫描设备…");
        self.diagnostics
            .insert(0, format!("扫描线程已停止：{}", detail));
    }
    fn poll_results(&mut self, ctx: &egui::Context) {
        if self.scan_service_stopped {
            return;
        }
        loop {
            match self.service.results.try_recv() {
                Ok(ScanMessage::Progress(id)) => {
                    if self.active_scan_id == Some(id) {
                        self.last_progress = Instant::now();
                        if self.scan_stalled {
                            self.scan_stalled = false;
                            self.scanning = true;
                        }
                    }
                }
                Ok(ScanMessage::Partial { id, devices }) => {
                    if self.active_scan_id == Some(id) {
                        self.last_progress = Instant::now();
                        if self.scan_stalled {
                            self.scan_stalled = false;
                            self.scanning = true;
                        }
                        self.merge_partial(devices);
                        ctx.request_repaint();
                    }
                }
                Ok(ScanMessage::Finished { id, result }) => {
                    if self.active_scan_id != Some(id) {
                        continue;
                    }
                    self.active_scan_id = None;
                    self.scanning = false;
                    self.scan_stalled = false;
                    self.last_scan = Some(Instant::now());
                    // Apply changes made during the scan once, with the settings that are current now.
                    if self.rescan_pending {
                        self.rescan_pending = false;
                        self.start_scan();
                    }
                    let ScanResult {
                        devices,
                        diagnostics: scan_diagnostics,
                        bluetooth_count,
                        hid_count,
                        hid_interfaces,
                    } = result;
                    self.devices = devices;
                    sort_devices_for_display(&mut self.devices);
                    let old_interfaces = std::mem::take(&mut self.hid_interfaces);
                    self.wizard
                        .on_interfaces_changed(&old_interfaces, &hid_interfaces);
                    self.hid_interfaces = hid_interfaces;
                    let mut diagnostics = Vec::with_capacity(scan_diagnostics.len() + 4);
                    diagnostics.push(format!(
                        "扫描完成：{} 台设备，{} 个蓝牙节点，{} 个 HID 接口",
                        self.devices.len(),
                        bluetooth_count,
                        hid_count
                    ));
                    diagnostics.extend(scan_diagnostics);
                    // Profile load errors must survive the next scan, which replaces
                    // the whole diagnostics vector.
                    diagnostics.extend(self.profile_diagnostics.clone());
                    self.diagnostics = diagnostics;
                    self.update_counts();
                    self.update_tray();
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.mark_scan_service_stopped("结果通道已断开，扫描线程可能异常退出");
                    ctx.request_repaint();
                    break;
                }
            }
        }
    }
    fn reload_profiles(&mut self) {
        let load = self.profile_store.load();
        self.profiles = Arc::new(load.catalog);
        self.profile_diagnostics = load.diagnostics;
    }
    fn protocol_wizard_page(&mut self, ui: &mut Ui, p: &Palette) {
        let busy = self.scanning || self.scan_stalled;
        let action = self.wizard.show(
            ui,
            p,
            &self.hid_interfaces,
            self.profiles.profiles(),
            &self.profile_diagnostics,
            busy,
        );
        match action {
            WizardAction::None => {}
            WizardAction::Close => {
                self.wizard.cancel_capture();
                self.page = Page::Settings;
            }
            WizardAction::Save(profile) => {
                // Reconfiguring the same interface replaces its previous profile
                // instead of creating an ambiguous pair that load() must disable.
                if let Some(existing) = self
                    .profiles
                    .profiles()
                    .iter()
                    .find(|existing| existing.binding.same_interface(&profile.binding))
                {
                    if existing.spec.id != profile.spec.id {
                        let _ = self.profile_store.delete(&existing.spec.id);
                    }
                }
                match self.profile_store.save(&profile) {
                    Ok(()) => {
                        self.reload_profiles();
                        self.wizard.reset_after_save();
                        self.queue_rescan();
                        self.toast("手动适配配置已保存（未验证）".to_owned());
                    }
                    Err(err) => self.toast(err),
                }
            }
            WizardAction::Toggle { id, enabled } => {
                match self.profile_store.set_enabled(&id, enabled) {
                    Ok(()) => {
                        self.reload_profiles();
                        self.queue_rescan();
                    }
                    Err(err) => self.toast(err),
                }
            }
            WizardAction::Delete(id) => match self.profile_store.delete(&id) {
                Ok(()) => {
                    self.reload_profiles();
                    self.queue_rescan();
                    self.toast("用户配置已删除".to_owned());
                }
                Err(err) => self.toast(err),
            },
        }
    }
    /// Merge a partial scan into the visible list without dropping devices that
    /// have not been reached by this scan yet. The final result replaces the list.
    fn merge_partial(&mut self, devices: Vec<DeviceSnapshot>) {
        merge_device_list(&mut self.devices, devices);
        self.update_counts();
    }
    fn effective_kind(&self, device: &DeviceSnapshot) -> DeviceKind {
        self.settings
            .icon_overrides
            .get(&device.id.0)
            .copied()
            .unwrap_or(device.kind)
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
            if self.effective_kind(device) == DeviceKind::Controller {
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
        let lowest = live
            .iter()
            .min_by_key(|device| device.percent().unwrap_or(255));
        // The custom hover popup replaces the native tooltip: keeping both would
        // draw two overlapping summaries. Only the icon changes here.
        let (level, charging) = match lowest {
            Some(device) => (device.percent(), device.battery.charging() == Some(true)),
            None => (None, false),
        };
        let next = (level, charging);
        if self.last_tray.as_ref() == Some(&next) {
            return;
        }
        if let Some(icon) = icons::tray_icon(level, charging) {
            if tray.icon.set_icon(Some(icon)).is_ok() {
                self.last_tray = Some(next);
            }
        }
    }
    fn toast(&mut self, message: String) {
        self.toast = Some((message, Instant::now()));
    }
    /// Persist settings. Theme and animation switches are pushed by `maybe_refresh_theme`
    /// through their own `applied_*` tracking, so saving here never forces a theme reapply.
    fn save_settings(&mut self) {
        if !storage::save(&self.settings) {
            self.toast(format!("无法保存设置到 {}", storage::data_dir().display()));
        }
        if let Some(tray) = &self.tray {
            tray.autostart.set_checked(self.settings.autostart);
        }
    }
    fn process_commands(&mut self, ctx: &egui::Context) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                UiCommand::Show => self.show(ctx),
                UiCommand::Toggle => {
                    let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
                    if self.hidden || minimized {
                        self.show(ctx);
                    } else {
                        self.hide(ctx);
                    }
                }
                UiCommand::TrayHover(event) => self.on_tray_hover(ctx, event),
                UiCommand::DismissTrayPopup => self.dismiss_tray_popup(ctx),
                UiCommand::Menu(id) => {
                    self.dismiss_tray_popup(ctx);
                    match id.as_str() {
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
                            self.apply_autostart(!self.settings.autostart);
                            self.save_settings();
                        }
                        "quit" => {
                            self.quitting = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    /// Change the autostart setting only if the registry accepted the change, so the stored
    /// value always matches the real registry state and the tray check item.
    fn apply_autostart(&mut self, enabled: bool) -> bool {
        if system::set_autostart(enabled) {
            self.settings.autostart = enabled;
            true
        } else {
            self.toast("无法修改开机自启（注册表被拒绝）".to_owned());
            false
        }
    }
    fn show(&mut self, ctx: &egui::Context) {
        self.dismiss_tray_popup(ctx);
        self.hidden = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        // A tray click must bring a minimized window back, not leave it in the taskbar.
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.request_repaint();
    }
    fn hide(&mut self, ctx: &egui::Context) {
        self.dismiss_tray_popup(ctx);
        self.hidden = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }
    fn on_tray_hover(&mut self, ctx: &egui::Context, event: TrayHoverEvent) {
        let was_visible = self.tray_popup.is_visible();
        let old_anchor = self.tray_popup.anchor();
        let changed = self.tray_popup.on_event(event, Instant::now(), self.hidden);
        if !changed {
            return;
        }
        let is_visible = self.tray_popup.is_visible();
        let anchor_changed = old_anchor != self.tray_popup.anchor();
        // A Move refreshes the 8s safety deadline; that alone must not force a
        // full hidden-window pass on every mouse event.
        if self.hidden && (was_visible != is_visible || anchor_changed) {
            ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
        }
        ctx.request_repaint();
    }
    fn dismiss_tray_popup(&mut self, ctx: &egui::Context) {
        let was_visible = self.tray_popup.is_visible();
        if self.tray_popup.dismiss() {
            if self.hidden && was_visible {
                ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint();
        }
    }
    fn tick_tray_popup(&mut self, ctx: &egui::Context) {
        let was_visible = self.tray_popup.is_visible();
        let now = Instant::now();
        let toggled = self.tray_popup.tick(now, self.hidden);
        let is_visible = self.tray_popup.is_visible();
        if toggled || was_visible != is_visible {
            if self.hidden {
                // Creating or removing a deferred child needs one complete pass,
                // even while the root window is hidden.
                ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint();
        }
        if let Some(deadline) = self.tray_popup.next_deadline() {
            let wait = deadline.saturating_duration_since(now);
            ctx.request_repaint_after(wait.max(Duration::from_millis(16)));
        }
        if is_visible && self.hidden {
            // The popup holds an immutable snapshot, so a full pass is what
            // republishes new scan results into the deferred callback.
            ctx.send_viewport_cmd(ViewportCommand::RequestPaintWhileHidden);
            // Keep “N 秒前完成扫描” current without a 1 Hz root repaint when idle.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
    fn sync_tray_popup(&mut self, ctx: &egui::Context) {
        if !self.hidden || !self.tray_popup.is_visible() {
            return;
        }
        let Some(anchor) = self.tray_popup.anchor() else {
            return;
        };
        let monitor = monitor::work_area_at(anchor.center_x(), anchor.center_y())
            .unwrap_or_else(|| fallback_monitor(ctx));
        let zoom = ctx.zoom_factor();
        let pixels_per_point = (monitor.scale * zoom).max(0.5);
        let model = Arc::new(TrayPopupModel::from_devices(
            self.dark,
            &self.devices,
            self.scanning,
            self.scan_stalled,
            self.scan_service_stopped,
            self.last_scan,
        ));
        let size = egui::vec2(crate::tray_popup::POPUP_WIDTH, model.height());
        let position = crate::tray_popup::place_popup(anchor, size, monitor, pixels_per_point);
        let builder = egui::ViewportBuilder::default()
            .with_title("AnyBattery")
            .with_decorations(false)
            .with_resizable(false)
            .with_active(false)
            .with_taskbar(false)
            .with_mouse_passthrough(true)
            .with_clamp_size_to_monitor_size(false)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_position(position)
            .with_window_level(egui::WindowLevel::AlwaysOnTop);
        let viewport_id = egui::ViewportId::from_hash_of("anybattery-tray-popup");
        ctx.show_viewport_deferred(viewport_id, builder, move |ui, _class| {
            crate::tray_popup::show(ui, &model);
        });
    }
    fn maybe_auto_scan(&mut self) {
        if self.wizard.is_capturing() {
            return;
        }
        if self.scan_service_stopped || self.scan_stalled || self.scanning {
            return;
        }
        let due = self
            .last_scan
            .map(|last| last.elapsed() >= self.settings.poll_interval())
            .unwrap_or(true);
        if due {
            self.start_scan();
        }
    }
    fn maybe_refresh_theme(&mut self, ctx: &egui::Context) {
        let mut dark = self.dark;
        if self.settings.theme == Theme::System
            && self.last_theme_check.elapsed() > Duration::from_secs(2)
        {
            self.last_theme_check = Instant::now();
            dark = theme::resolve_dark(self.settings.theme);
        }
        if self.applied_dark != Some(dark)
            || self.applied_animations != Some(self.settings.animations)
        {
            self.dark = dark;
            self.applied_dark = Some(dark);
            self.applied_animations = Some(self.settings.animations);
            self.animations = self.settings.animations;
            theme::apply_visuals(ctx, theme::palette(dark), self.settings.animations);
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(if dark {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            }));
        }
    }
    /// Custom title bar: it replaces the native caption while keeping the OS drag loop
    /// (`StartDrag`) so Windows snap layouts and the normal move animation still work.
    fn titlebar(&mut self, ui: &mut Ui, p: &Palette, maximized: bool) {
        let outer = ui.max_rect();
        let bar = Rect::from_min_max(
            Pos2::new(outer.left() + 2.0, outer.top() + 2.0),
            Pos2::new(outer.right() - 2.0, outer.top() + widgets::TITLEBAR_HEIGHT),
        );
        let painter = ui.painter();
        painter.rect_filled(bar, CornerRadius::ZERO, p.bg_glow);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(bar.left(), bar.bottom() - 2.0),
                vec2(bar.width(), 2.0),
            ),
            CornerRadius::ZERO,
            p.card_border,
        );

        let icon_size = 20.0;
        let icon = Rect::from_center_size(
            Pos2::new(bar.left() + 8.0 + icon_size / 2.0, bar.center().y),
            vec2(icon_size, icon_size),
        );
        icons::draw_battery_mark(painter, icon, p);
        painter.text(
            Pos2::new(icon.right() + 7.0, bar.center().y),
            Align2::LEFT_CENTER,
            "AnyBattery",
            FontId::new(13.0, egui::FontFamily::Name("bold".into())),
            p.text,
        );

        let layout = widgets::caption_button_layout(bar);
        if widgets::caption_button(
            ui,
            p,
            layout.minimize,
            widgets::Glyph::Minimize,
            "最小化",
            false,
        )
        .clicked()
        {
            ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        let maximize_glyph = if maximized {
            widgets::Glyph::Restore
        } else {
            widgets::Glyph::Maximize
        };
        if widgets::caption_button(
            ui,
            p,
            layout.maximize,
            maximize_glyph,
            if maximized { "还原" } else { "最大化" },
            false,
        )
        .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            ui.ctx().request_repaint();
        }
        if widgets::caption_button(
            ui,
            p,
            layout.close,
            widgets::Glyph::Close,
            "关闭到托盘",
            true,
        )
        .clicked()
        {
            self.hide(ui.ctx());
        }

        // Leave the button cluster out of the drag target so a press on a caption button can
        // never start a window move on the same frame.
        let drag_rect = Rect::from_min_max(
            Pos2::new(bar.left() + 4.0, bar.top() + 4.0),
            Pos2::new(layout.right_edge() - 6.0, bar.bottom() - 2.0),
        );
        let drag = ui.interact(
            drag_rect,
            Id::new("anybattery.titlebar.drag"),
            Sense::click_and_drag(),
        );
        if drag.double_clicked_by(PointerButton::Primary) {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            ui.ctx().request_repaint();
        } else if drag.drag_started_by(PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }

    /// Resize handles for a decorated=false window. They are registered after the page so they
    /// win hit-testing in the outer margin, where no page content is drawn.
    fn resize_handles(&self, ui: &Ui, maximized: bool) {
        if maximized {
            return;
        }
        use egui::viewport::ResizeDirection;

        let rect = ui.max_rect();
        let edge = 6.0;
        let corner = 12.0;
        let edges = [
            (
                ResizeDirection::North,
                Rect::from_min_max(
                    Pos2::new(rect.left() + corner, rect.top()),
                    Pos2::new(rect.right() - corner, rect.top() + edge),
                ),
                CursorIcon::ResizeNorth,
            ),
            (
                ResizeDirection::South,
                Rect::from_min_max(
                    Pos2::new(rect.left() + corner, rect.bottom() - edge),
                    Pos2::new(rect.right() - corner, rect.bottom()),
                ),
                CursorIcon::ResizeSouth,
            ),
            (
                ResizeDirection::West,
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + corner),
                    Pos2::new(rect.left() + edge, rect.bottom() - corner),
                ),
                CursorIcon::ResizeWest,
            ),
            (
                ResizeDirection::East,
                Rect::from_min_max(
                    Pos2::new(rect.right() - edge, rect.top() + corner),
                    Pos2::new(rect.right(), rect.bottom() - corner),
                ),
                CursorIcon::ResizeEast,
            ),
            (
                ResizeDirection::NorthWest,
                Rect::from_min_max(
                    rect.min,
                    Pos2::new(rect.left() + corner, rect.top() + corner),
                ),
                CursorIcon::ResizeNorthWest,
            ),
            (
                ResizeDirection::NorthEast,
                Rect::from_min_max(
                    Pos2::new(rect.right() - corner, rect.top()),
                    Pos2::new(rect.right(), rect.top() + corner),
                ),
                CursorIcon::ResizeNorthEast,
            ),
            (
                ResizeDirection::SouthWest,
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.bottom() - corner),
                    Pos2::new(rect.left() + corner, rect.bottom()),
                ),
                CursorIcon::ResizeSouthWest,
            ),
            (
                ResizeDirection::SouthEast,
                Rect::from_min_max(
                    Pos2::new(rect.right() - corner, rect.bottom() - corner),
                    Pos2::new(rect.right(), rect.bottom()),
                ),
                CursorIcon::ResizeSouthEast,
            ),
        ];
        for (index, (direction, area, cursor)) in edges.into_iter().enumerate() {
            let response = ui.interact(area, Id::new(("anybattery.resize", index)), Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(cursor);
                if ui.input(|input| input.pointer.button_pressed(PointerButton::Primary)) {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            }
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
        let (bar, _) = ui.allocate_exact_size(vec2(width, 50.0), Sense::hover());
        let mut right = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    Pos2::new(bar.right() - 70.0, bar.top() + 11.0),
                    vec2(70.0, 28.0),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        if widgets::icon_button(&mut right, p, widgets::Glyph::Settings, "设置").clicked() {
            self.page = if self.page == Page::Settings {
                Page::Devices
            } else {
                Page::Settings
            };
        }
        let scanning = self.scanning;
        if widgets::refresh_button(&mut right, p, scanning, self.animations).clicked() && !scanning
        {
            self.request_scan();
        }

        let mut title_left = bar.left();
        if self.page != Page::Devices {
            let back_rect =
                Rect::from_min_size(Pos2::new(bar.left(), bar.top() + 11.0), vec2(30.0, 28.0));
            let mut back = ui.new_child(
                UiBuilder::new()
                    .max_rect(back_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if widgets::icon_button(&mut back, p, widgets::Glyph::Back, "返回").clicked() {
                self.wizard.cancel_capture();
                self.page = if self.page == Page::ProtocolWizard {
                    Page::Settings
                } else {
                    Page::Devices
                };
            }
            title_left += 36.0;
        }
        let (title, subtitle) = match self.page {
            Page::Devices => (
                "电池".to_owned(),
                if self.devices.is_empty() {
                    if self.scanning {
                        "正在扫描设备…".to_owned()
                    } else {
                        "蓝牙与 2.4G 接收器".to_owned()
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
            Page::ProtocolWizard => (
                "手动适配".to_owned(),
                "被动监听 2.4G 接口并校准电量".to_owned(),
            ),
        };
        let title_width = (bar.right() - 78.0 - title_left).max(40.0);
        let painter = ui.painter();
        painter.text(
            Pos2::new(title_left, bar.top() + 15.0),
            Align2::LEFT_CENTER,
            widgets::fit_text(ui, &title, 28.0, true, title_width),
            FontId::new(28.0, egui::FontFamily::Name("bold".into())),
            p.text,
        );
        painter.text(
            Pos2::new(title_left, bar.top() + 42.0),
            Align2::LEFT_CENTER,
            widgets::fit_text(ui, &subtitle, 11.0, false, title_width),
            FontId::new(11.0, egui::FontFamily::Proportional),
            p.secondary,
        );
    }
    fn devices_page(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(8.0);
        if self.settings.notify_low_battery {
            if let Some(alert) = &self.alert {
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                icons::pixel_frame(ui.painter(), rect, p.warning_bg, p.card_border);
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left() + 10.0, rect.center().y - 6.0),
                        vec2(12.0, 12.0),
                    ),
                    CornerRadius::ZERO,
                    p.accent,
                );
                ui.painter().text(
                    Pos2::new(rect.left() + 30.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    widgets::fit_text(ui, alert, 12.0, true, ui.available_width() - 40.0),
                    FontId::new(12.0, egui::FontFamily::Name("bold".into())),
                    p.warning_text,
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
        let now = now_unix();
        // Manual icon choices also drive the filter and header counts, so a
        // receiver the user reclassifies as a mouse moves to the right bucket.
        let mut visible = self.devices.clone();
        for device in &mut visible {
            device.kind = self.effective_kind(device);
        }
        let filtered: Vec<&DeviceSnapshot> = visible
            .iter()
            .filter(|device| self.filter.matches(device))
            .collect();
        // Reserve exactly what the cards need instead of the whole page. With few devices the
        // footer follows the last card; with many it stays outside a capped scroll area.
        let content_height = filtered.len() as f32 * (widgets::CARD_HEIGHT + 8.0) + 4.0;
        let available = (ui.available_height() - 50.0).max(80.0);
        let list_height = if filtered.is_empty() {
            available
        } else {
            content_height.min(available).max(80.0)
        };
        let empty = if self.scanning {
            EmptyState::Scanning
        } else if self.devices.is_empty() {
            EmptyState::NoDevices
        } else {
            EmptyState::Filtered
        };
        let mut show_all = false;
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if filtered.is_empty() {
                    // Three distinct situations: a scan in flight, nothing found at all, or
                    // only the current filter came up empty.
                    if empty_state(ui, p, empty) {
                        show_all = true;
                    }
                } else {
                    for device in &filtered {
                        widgets::device_card(ui, p, device, now);
                        ui.add_space(8.0);
                    }
                }
                ui.add_space(4.0);
            });
        if show_all {
            self.filter = Filter::All;
        }
        ui.add_space(2.0);
        if widgets::footer(
            ui,
            p,
            self.settings.poll_secs,
            self.last_scan,
            self.scanning,
        )
        .clicked()
        {
            self.page = Page::Diagnostics;
        }
    }
    fn icon_settings(&mut self, ui: &mut Ui, p: &Palette) {
        if self.devices.is_empty() {
            ui.label(theme::rich(
                "完成一次扫描后，可在这里为设备指定图标。",
                11.0,
                p.secondary,
                false,
            ));
            return;
        }
        if !self.settings.icon_overrides.is_empty() {
            ui.horizontal(|ui| {
                ui.label(theme::rich(
                    format!("手动指定 {} 台", self.settings.icon_overrides.len()),
                    10.5,
                    p.tertiary,
                    false,
                ));
                if widgets::pill_button(ui, p, "全部恢复自动", false).clicked() {
                    self.settings.icon_overrides.clear();
                    self.save_settings();
                    self.update_counts();
                }
            });
        }

        let devices = self.devices.clone();
        for device in devices {
            let id = device.id.0.clone();
            let auto = device.kind;
            let current = self.settings.icon_overrides.get(&id).copied();
            let effective = current.unwrap_or(auto);

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                widgets::device_icon_tile(ui, p, effective);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let width = (ui.available_width() - 8.0).max(80.0);
                    ui.label(theme::rich(
                        widgets::fit_text(ui, &device.name, 12.0, true, width),
                        12.0,
                        p.text,
                        true,
                    ));
                    let detail = match current {
                        Some(kind) => format!("手动：{}", kind.label_zh()),
                        None => format!("自动：{}", auto.label_zh()),
                    };
                    ui.label(theme::rich(detail, 10.0, p.tertiary, false));
                });
            });

            let mut choice: Option<Option<DeviceKind>> = None;
            ui.horizontal_wrapped(|ui| {
                if widgets::pill_button(ui, p, "自动", current.is_none()).clicked() {
                    choice = Some(None);
                }
                for kind in DEVICE_ICON_CHOICES {
                    if widgets::device_icon_button(ui, p, kind, current == Some(kind)).clicked() {
                        choice = Some(Some(kind));
                    }
                }
            });
            if let Some(choice) = choice {
                match choice {
                    Some(kind) => {
                        self.settings.icon_overrides.insert(id, kind);
                    }
                    None => {
                        self.settings.icon_overrides.remove(&id);
                    }
                }
                self.save_settings();
                self.update_counts();
            }
        }
        ui.add_space(4.0);
        ui.label(theme::rich(
            "自动按产品名与协议识别；无法判断时选一个最接近的内置像素图标。",
            10.0,
            p.tertiary,
            false,
        ));
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
                                widgets::interval_text(self.settings.poll_secs),
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
                    let holding = slider.is_pointer_button_down_on();
                    if slider.changed() {
                        // Value and label follow the drag immediately; the file is touched
                        // once, when the drag ends (or on a keyboard/other non-drag change).
                        self.settings.poll_secs = poll.round() as u64;
                        self.settings.validate();
                    }
                    let drag_ended = self.poll_dragging && !holding;
                    if drag_ended || (slider.changed() && !holding) {
                        self.save_settings();
                    }
                    self.poll_dragging = holding;
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        for (label, value) in [
                            ("30 秒", 30u64),
                            ("1 分钟", 60),
                            ("5 分钟", 300),
                            ("10 分钟", 600),
                        ] {
                            if widgets::pill_button(ui, p, label, self.settings.poll_secs == value)
                                .clicked()
                            {
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
                    widgets::settings_row(
                        ui,
                        p,
                        "低电量提醒",
                        "面板顶部与托盘图标变化",
                        |ui| {
                            changed = widgets::toggle(ui, p, &mut value);
                        },
                    );
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
                        let (seg_rect, _) =
                            ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
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
                    widgets::settings_row(
                        ui,
                        p,
                        "开机自启",
                        "登录 Windows 后自动运行",
                        |ui| {
                            changed = widgets::toggle(ui, p, &mut autostart);
                        },
                    );
                    if changed {
                        self.apply_autostart(autostart);
                        self.save_settings();
                    }
                    let mut show_unknown = self.settings.show_unknown;
                    let mut changed = false;
                    widgets::settings_row(
                        ui,
                        p,
                        "显示无电池设备",
                        "没有电量或未匹配协议时也列出",
                        |ui| {
                            changed = widgets::toggle(ui, p, &mut show_unknown);
                        },
                    );
                    if changed {
                        self.settings.show_unknown = show_unknown;
                        self.save_settings();
                        self.queue_rescan();
                    }
                });
                ui.add_space(14.0);
                widgets::section_title(ui, p, "设备图标");
                widgets::group(ui, p, |ui| {
                    self.icon_settings(ui, p);
                });
                ui.add_space(14.0);
                widgets::section_title(ui, p, "手动适配");
                widgets::group(ui, p, |ui| {
                    ui.label(theme::rich(
                        format!(
                            "用户配置 {} 个（启用 {} 个）· 只读被动监听，不发送查询/写入",
                            self.profiles.profiles().len(),
                            self.profiles.enabled_profiles().count()
                        ),
                        10.5,
                        p.secondary,
                        false,
                    ));
                    if widgets::pill_button(ui, p, "打开手动适配向导", false).clicked() {
                        self.page = Page::ProtocolWizard;
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
                        // Turning autostart off is part of "defaults"; if the registry rejects
                        // it, keep every current setting instead of pretending it is off.
                        if self.apply_autostart(false) {
                            self.settings = Settings::default();
                            self.save_settings();
                            self.queue_rescan();
                        }
                    }
                });
                ui.add_space(18.0);
            });
    }
    fn diagnostics_page(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let refresh_label = if self.scanning {
                "扫描中…"
            } else {
                "刷新"
            };
            if widgets::pill_button_enabled(ui, p, refresh_label, false, !self.scanning).clicked() {
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
            if widgets::pill_button(ui, p, "打开日志目录", false).clicked() {
                let _ = std::process::Command::new("explorer")
                    .arg(crate::logging::log_dir())
                    .spawn();
            }
        });
        ui.add_space(8.0);
        ui.label(theme::rich(
            format!(
                "设备 {} · 蓝牙 {} · 2.4G {} · 手柄 {} · 每 {} 刷新",
                self.devices.len(),
                self.counts.0,
                self.counts.1,
                self.counts.2,
                widgets::interval_text(self.settings.poll_secs)
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
    fn paint_toast(&mut self, ui: &mut Ui, p: &Palette) {
        const TOAST_LIFE_SECS: f32 = 3.0;
        let Some((message, created)) = &self.toast else {
            return;
        };
        let elapsed = created.elapsed().as_secs_f32();
        if elapsed > TOAST_LIFE_SECS {
            self.toast = None;
            return;
        }
        // Fade in over the first 120 ms and out over the last 180 ms. With animations off the
        // toast is fully opaque and a single repaint is scheduled for its expiry, so it cannot
        // linger until the next one-second application tick or cause continuous repainting.
        let shown = if self.animations {
            let rise = (elapsed / 0.12).clamp(0.0, 1.0);
            let fall = ((TOAST_LIFE_SECS - elapsed) / 0.18).clamp(0.0, 1.0);
            // Animate the fades with short frames, but sleep until the fade-out begins.
            let until_fade_out = TOAST_LIFE_SECS - 0.18 - elapsed;
            let next = if elapsed < 0.12 {
                0.016
            } else if until_fade_out > 0.0 {
                until_fade_out
            } else {
                0.016
            };
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(next.max(0.016)));
            rise.min(fall)
        } else {
            let remaining = (TOAST_LIFE_SECS - elapsed).max(0.016);
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(remaining));
            1.0
        };
        let rect = ui.max_rect();
        let text = widgets::fit_text(ui, message, 12.0, false, rect.width() - 76.0);
        let text_width = ui
            .painter()
            .layout_no_wrap(
                text.clone(),
                FontId::new(12.0, egui::FontFamily::Proportional),
                p.text,
            )
            .size()
            .x;
        let width = (text_width + 32.0).min(rect.width() - 60.0).max(120.0);
        let toast = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.bottom() - 58.0 + (1.0 - shown) * 6.0),
            vec2(width, 32.0),
        );
        icons::draw_pixel_shadow(ui.painter(), toast, p.shadow.gamma_multiply(shown));
        icons::pixel_frame(
            ui.painter(),
            toast,
            p.card_solid.gamma_multiply(shown),
            p.card_border.gamma_multiply(shown),
        );
        ui.painter().text(
            toast.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(12.0, egui::FontFamily::Proportional),
            p.text.gamma_multiply(shown),
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
        if self.page != Page::ProtocolWizard && self.wizard.is_capturing() {
            self.wizard.cancel_capture();
        }
        self.wizard.poll_capture();
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        if self.quitting {
            return;
        }
        self.poll_results(ctx);
        self.check_scan_watchdog();
        self.tick_tray_popup(ctx);
        self.maybe_auto_scan();
        self.maybe_refresh_theme(ctx);
        // While hidden to the tray, wake at the next poll deadline instead of at 1 Hz.
        // Tray/command wake closures still request an immediate repaint.
        let repaint = if self.hidden && !self.scanning && !self.scan_stalled {
            let since = self
                .last_scan
                .map(|last| last.elapsed())
                .unwrap_or_default();
            self.settings
                .poll_interval()
                .saturating_sub(since)
                .max(Duration::from_secs(1))
        } else {
            Duration::from_secs(1)
        };
        ctx.request_repaint_after(repaint);
    }
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.quitting {
            return;
        }
        let p = theme::palette(self.dark);
        let maximized = ui
            .ctx()
            .input(|input| input.viewport().maximized.unwrap_or(false));
        self.paint_background(ui, &p);
        self.titlebar(ui, &p, maximized);
        let rect = ui.max_rect();
        let content = Rect::from_min_max(
            Pos2::new(
                rect.left() + 16.0,
                rect.top() + widgets::TITLEBAR_HEIGHT + 12.0,
            ),
            Pos2::new(rect.right() - 16.0, rect.bottom() - 12.0),
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
            Page::ProtocolWizard => self.protocol_wizard_page(&mut child, &p),
        }
        self.paint_toast(&mut child, &p);
        self.sync_tray_popup(child.ctx());
        self.resize_handles(ui, maximized);
    }
}
/// Fallback when Win32 monitor lookup fails: use the root window's monitor size
/// in physical pixels, anchored at the virtual-desktop origin.
fn fallback_monitor(ctx: &egui::Context) -> monitor::MonitorGeometry {
    let scale = ctx.pixels_per_point().max(0.5);
    let size = ctx
        .input(|input| input.viewport().monitor_size)
        .unwrap_or_else(|| egui::vec2(1920.0, 1080.0));
    monitor::MonitorGeometry {
        work_left: 0.0,
        work_top: 0.0,
        work_right: f64::from(size.x * scale),
        work_bottom: f64::from(size.y * scale),
        scale,
    }
}

/// Final display order required by DESIGN 8.2: lowest known percentage first, then the most
/// recently updated device, then the name. The scanner already applies a coarse
/// `DeviceSnapshot::sort_key` order, but its status group does not express "most recent first"
/// and would put some no-percentage readings ahead of known percentages, so the UI re-sorts the
/// final list. Readings without a percentage never get an invented level.
fn compare_devices_for_display(a: &DeviceSnapshot, b: &DeviceSnapshot) -> Ordering {
    match (a.percent(), b.percent()) {
        (Some(a_level), Some(b_level)) => a_level
            .cmp(&b_level)
            .then_with(|| {
                b.updated_at_unix
                    .unwrap_or(0)
                    .cmp(&a.updated_at_unix.unwrap_or(0))
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b
            .updated_at_unix
            .unwrap_or(0)
            .cmp(&a.updated_at_unix.unwrap_or(0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name)),
    }
}

fn sort_devices_for_display(devices: &mut [DeviceSnapshot]) {
    devices.sort_by(compare_devices_for_display);
}

/// Merge newly read cards by device id. Devices that this scan phase has not
/// reached yet stay visible until the final result replaces the list.
fn merge_device_list(devices: &mut Vec<DeviceSnapshot>, partial: Vec<DeviceSnapshot>) {
    for device in partial {
        if let Some(existing) = devices
            .iter_mut()
            .find(|existing| existing.id.0 == device.id.0)
        {
            *existing = device;
        } else {
            devices.push(device);
        }
    }
    sort_devices_for_display(devices);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EmptyState {
    Scanning,
    NoDevices,
    Filtered,
}

/// Draws the placeholder for an empty device list. Returns `true` when the user asked to
/// leave the current filter, so the caller can switch back to "全部".
fn empty_state(ui: &mut Ui, p: &Palette, empty: EmptyState) -> bool {
    let mut show_all = false;
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
        let (title, hint) = match empty {
            EmptyState::Scanning => (
                "正在扫描设备…",
                "扫描完成后会在这里列出能读取电量的设备",
            ),
            EmptyState::NoDevices => (
                "未发现可读取电量的设备",
                "确认设备已连接并处于开启状态；2.4G 接收器需插好，休眠设备需先唤醒。可打开设置调整显示范围",
            ),
            EmptyState::Filtered => (
                "该分类暂无设备",
                "当前筛选下没有设备，其他分类仍可能有数据",
            ),
        };
        ui.label(theme::rich(title, 18.0, p.text, true));
        ui.add_space(4.0);
        ui.label(theme::rich(hint, 12.0, p.secondary, false));
        if empty == EmptyState::Filtered {
            ui.add_space(10.0);
            show_all = widgets::pill_button(ui, p, "查看全部", false).clicked();
        }
    });
    show_all
}

#[cfg(test)]
mod tests {
    use super::*;
    use anybattery_core::{
        BatteryReading, BatterySource, CoarseLevel, Confidence, DeviceId, DeviceKind, Transport,
    };

    fn sample(name: &str, percent: Option<u8>, updated: u64) -> DeviceSnapshot {
        let mut device = DeviceSnapshot::new(
            DeviceId::new("test", name),
            name,
            DeviceKind::Mouse,
            Transport::BluetoothClassic,
            BatterySource::new("test", Confidence::Verified, ""),
        );
        device.battery = match percent {
            Some(level) => BatteryReading::percent(level, None),
            None => BatteryReading::Unknown,
        };
        device.updated_at_unix = Some(updated);
        device
    }

    fn coarse(name: &str, level: CoarseLevel, updated: u64) -> DeviceSnapshot {
        let mut device = sample(name, None, updated);
        device.battery = BatteryReading::Coarse { level };
        device
    }

    fn names(devices: &[DeviceSnapshot]) -> Vec<&str> {
        devices.iter().map(|device| device.name.as_str()).collect()
    }

    #[test]
    fn devices_sort_by_low_percent_then_recent_then_name() {
        let mut devices = vec![
            sample("Zeta", Some(50), 100),
            sample("Alpha", Some(50), 100),
            sample("Old", Some(20), 10),
            sample("New", Some(20), 20),
            sample("High", Some(90), 50),
            sample("Unknown", None, 999),
        ];
        sort_devices_for_display(&mut devices);
        assert_eq!(
            names(&devices),
            ["New", "Old", "Alpha", "Zeta", "High", "Unknown"]
        );
    }

    #[test]
    fn partial_merge_keeps_unseen_devices_and_updates_existing() {
        let mut devices = vec![sample("Alpha", Some(50), 1), sample("Beta", Some(80), 2)];
        merge_device_list(
            &mut devices,
            vec![sample("Alpha", Some(40), 3), sample("Gamma", Some(90), 4)],
        );
        assert_eq!(names(&devices), ["Alpha", "Beta", "Gamma"]);
        let alpha = devices
            .iter()
            .find(|device| device.name == "Alpha")
            .expect("alpha present");
        assert_eq!(alpha.percent(), Some(40));
        assert_eq!(alpha.updated_at_unix, Some(3));
    }

    #[test]
    fn readings_without_percentage_stay_after_percentages() {
        let mut devices = vec![
            coarse("Coarse Low", CoarseLevel::Low, 100),
            sample("Ninety", Some(90), 5),
            sample("Unknown", None, 200),
            sample("One", Some(1), 5),
        ];
        sort_devices_for_display(&mut devices);
        assert_eq!(names(&devices), ["One", "Ninety", "Unknown", "Coarse Low"]);
    }
}
