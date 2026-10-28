//! 手动 2.4G 被动适配向导。
//!
//! 这里只做“选择接口 → 被动捕获 → 两样本校准 → 预览 → 保存”的最小闭环。
//! 捕获使用 `anybattery_windows::capture_with`，只读输入报文；本模块不发送
//! Output/Feature，也不执行用户脚本。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anybattery_core::protocol::{
    Extract, Field, FieldType, MatchByte, Matcher, ProtocolSpec, ProtocolVerification, Request,
    RequestKind, Response, ResponseKind, hex_to_bytes,
};
use anybattery_core::user_protocol::USER_PROFILE_SCHEMA;
use anybattery_core::{Calibration, CalibrationSample, UserProtocolProfile};
use anybattery_windows::{
    CaptureSummary, HidInterface, PassiveCaptureError, ReadOnlyHid, binding_for, capture_with,
};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use eframe::egui::{self, ComboBox, DragValue, TextEdit, Ui};

use crate::theme::{self, Palette};
use crate::widgets;

const CAPTURE_WINDOW: Duration = Duration::from_secs(4);
const CAPTURE_REPORT_LIMIT: usize = 64;
const CAPTURE_PREVIEW_INTERVAL: Duration = Duration::from_millis(50);
const SAMPLE_LIMIT: usize = 6;

struct CaptureRequest {
    id: u64,
    path: String,
    input_len: u16,
    cancel: Arc<AtomicBool>,
}

enum CaptureMessage {
    Report {
        id: u64,
        report: Vec<u8>,
        count: usize,
    },
    Finished {
        id: u64,
        outcome: Result<CaptureSummary, PassiveCaptureError>,
        last: Option<Vec<u8>>,
        count: usize,
    },
}

struct CaptureService {
    requests: Sender<CaptureRequest>,
    results: Receiver<CaptureMessage>,
}

impl CaptureService {
    fn start() -> Self {
        let (requests, request_rx) = unbounded::<CaptureRequest>();
        let (result_tx, results) = unbounded::<CaptureMessage>();
        let _ = std::thread::Builder::new()
            .name("anybattery-capture".to_owned())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let Some(mut reader) = ReadOnlyHid::open(&request.path) else {
                        let _ = result_tx.send(CaptureMessage::Finished {
                            id: request.id,
                            outcome: Err(PassiveCaptureError::OpenFailed),
                            last: None,
                            count: 0,
                        });
                        continue;
                    };
                    let mut count = 0usize;
                    let mut sent_count = 0usize;
                    let mut last_report = None;
                    let mut last_sent = Instant::now() - CAPTURE_PREVIEW_INTERVAL;
                    let outcome = capture_with(
                        &mut reader,
                        usize::from(request.input_len),
                        CAPTURE_WINDOW,
                        &request.cancel,
                        &mut |report| {
                            count += 1;
                            last_report = Some(report.clone());
                            if last_sent.elapsed() >= CAPTURE_PREVIEW_INTERVAL {
                                last_sent = Instant::now();
                                sent_count = count;
                                let _ = result_tx.send(CaptureMessage::Report {
                                    id: request.id,
                                    report,
                                    count,
                                });
                            }
                        },
                    );
                    // Make sure the final report reaches the UI even when the
                    // capture ended between two throttle windows.
                    if count > sent_count {
                        if let Some(report) = last_report.clone() {
                            let _ = result_tx.send(CaptureMessage::Report {
                                id: request.id,
                                report,
                                count,
                            });
                        }
                    }
                    let _ = result_tx.send(CaptureMessage::Finished {
                        id: request.id,
                        outcome,
                        last: last_report,
                        count,
                    });
                }
            });
        Self { requests, results }
    }
}

#[derive(Clone)]
struct Sample {
    report: Vec<u8>,
    shown_percent: u8,
}

impl Sample {
    fn raw(&self, offset: usize) -> Option<u8> {
        self.report.get(offset).copied()
    }
}

struct Preview {
    valid: usize,
    level: Option<u8>,
    report: Option<Vec<u8>>,
}

pub enum WizardAction {
    None,
    Close,
    Save(UserProtocolProfile),
    Toggle { id: String, enabled: bool },
    Delete(String),
}

pub struct ProtocolWizard {
    capture: CaptureService,
    capture_id: u64,
    capture_active: bool,
    capture_cancelling: bool,
    capture_cancel: Option<Arc<AtomicBool>>,
    capture_reports: VecDeque<Vec<u8>>,
    capture_count: usize,
    capture_result: Option<Result<CaptureSummary, PassiveCaptureError>>,
    selected: Option<usize>,
    samples: Vec<Sample>,
    sample_shown: u8,
    offset: usize,
    match_offset: usize,
    match_hex: String,
    name: String,
    message: Option<String>,
    error: Option<String>,
    pending_delete: Option<String>,
}

impl Default for ProtocolWizard {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolWizard {
    pub fn new() -> Self {
        Self {
            capture: CaptureService::start(),
            capture_id: 0,
            capture_active: false,
            capture_cancelling: false,
            capture_cancel: None,
            capture_reports: VecDeque::new(),
            capture_count: 0,
            capture_result: None,
            selected: None,
            samples: Vec::new(),
            sample_shown: 50,
            offset: 4,
            match_offset: 0,
            match_hex: String::new(),
            name: "手动适配".to_owned(),
            message: None,
            error: None,
            pending_delete: None,
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.capture_active || self.capture_cancelling
    }

    /// Poll capture messages even when the wizard page is not visible.
    pub fn poll_capture(&mut self) {
        self.poll();
    }

    /// Clear the transient capture/sample state after a successful save.
    pub fn reset_after_save(&mut self) {
        self.abandon_capture();
        self.samples.clear();
        self.capture_reports.clear();
        self.capture_count = 0;
        self.capture_result = None;
        self.match_hex.clear();
        self.pending_delete = None;
        self.message = Some("已保存为未验证配置；在设置中可以启用、禁用或删除".to_owned());
        self.error = None;
    }

    pub fn cancel_capture(&mut self) {
        if let Some(cancel) = &self.capture_cancel {
            cancel.store(true, Ordering::SeqCst);
            self.capture_cancelling = true;
        }
    }

    /// Abandon a capture without waiting for its final message. The generation
    /// id is bumped so any late report from the old worker is ignored.
    fn abandon_capture(&mut self) {
        if let Some(cancel) = &self.capture_cancel {
            cancel.store(true, Ordering::SeqCst);
        }
        self.capture_id = self.capture_id.wrapping_add(1);
        self.capture_active = false;
        self.capture_cancelling = false;
        self.capture_cancel = None;
    }

    fn poll(&mut self) {
        loop {
            match self.capture.results.try_recv() {
                Ok(CaptureMessage::Report { id, report, count }) => {
                    if id != self.capture_id {
                        continue;
                    }
                    self.capture_count = count;
                    if self.match_hex.trim().is_empty() {
                        if let Some(first) = report.first() {
                            self.match_hex = format!("{first:02X}");
                        }
                    }
                    self.capture_reports.push_back(report);
                    while self.capture_reports.len() > CAPTURE_REPORT_LIMIT {
                        self.capture_reports.pop_front();
                    }
                }
                Ok(CaptureMessage::Finished {
                    id,
                    outcome,
                    last,
                    count,
                }) => {
                    if id != self.capture_id {
                        continue;
                    }
                    self.capture_active = false;
                    self.capture_cancelling = false;
                    self.capture_cancel = None;
                    self.capture_count = count;
                    if let Some(report) = last {
                        if self.match_hex.trim().is_empty() {
                            if let Some(first) = report.first() {
                                self.match_hex = format!("{first:02X}");
                            }
                        }
                        if self.capture_reports.is_empty() {
                            self.capture_reports.push_back(report);
                        }
                    }
                    match &outcome {
                        Ok(summary) => {
                            self.capture_result = Some(Ok(*summary));
                            self.error = None;
                        }
                        Err(PassiveCaptureError::Cancelled) => {
                            self.capture_result = Some(Err(PassiveCaptureError::Cancelled));
                        }
                        Err(err) => {
                            self.capture_result = Some(Err(err.clone()));
                            self.error = Some(err.label_zh().to_owned());
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("捕获线程已停止，请重新打开向导".to_owned());
                    self.capture_active = false;
                    self.capture_cancelling = false;
                    break;
                }
            }
        }
    }

    /// Keep the selected device stable when a scan reorders the interface list.
    pub fn on_interfaces_changed(&mut self, old: &[HidInterface], new: &[HidInterface]) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(old_info) = old.get(index) else {
            self.selected = None;
            return;
        };
        let Ok(binding) = binding_for(old_info) else {
            self.selected = None;
            return;
        };
        let remapped = new.iter().position(|info| {
            binding_for(info)
                .map(|candidate| candidate.same_interface(&binding))
                .unwrap_or(false)
        });
        self.selected = remapped;
        if remapped.is_none() {
            self.abandon_capture();
            self.samples.clear();
            self.capture_reports.clear();
            self.match_hex.clear();
            self.error = Some("选中的接口已消失，请重新选择".to_owned());
        }
    }

    fn selected_info<'a>(&self, interfaces: &'a [HidInterface]) -> Option<&'a HidInterface> {
        self.selected.and_then(|index| interfaces.get(index))
    }

    fn select_interface(&mut self, index: usize, interfaces: &[HidInterface]) {
        if self.selected == Some(index) {
            return;
        }
        if self.is_capturing() {
            self.abandon_capture();
        }
        self.selected = Some(index);
        self.samples.clear();
        self.capture_reports.clear();
        self.capture_count = 0;
        self.capture_result = None;
        self.match_hex.clear();
        self.message = None;
        self.error = None;
        if let Some(info) = interfaces.get(index) {
            self.offset = self
                .offset
                .min(usize::from(info.input_len).saturating_sub(1));
            self.name = if info.product.trim().is_empty() {
                "手动适配".to_owned()
            } else {
                info.product.clone()
            };
        }
    }

    fn start_capture(&mut self, info: &HidInterface, scan_busy: bool) {
        if scan_busy || self.is_capturing() {
            self.error = Some(if scan_busy {
                "扫描进行中，请等扫描完成后再捕获".to_owned()
            } else {
                "已有捕获正在进行".to_owned()
            });
            return;
        }
        let Ok(binding) = binding_for(info) else {
            self.error = Some("该接口缺少完整身份，无法绑定".to_owned());
            return;
        };
        if binding.input_len == 0 {
            self.error = Some("该接口没有可读输入报文".to_owned());
            return;
        }
        self.capture_id = self.capture_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let request = CaptureRequest {
            id: self.capture_id,
            path: info.path.clone(),
            input_len: binding.input_len,
            cancel: cancel.clone(),
        };
        if self.capture.requests.send(request).is_err() {
            self.error = Some("无法启动捕获线程".to_owned());
            return;
        }
        self.capture_cancel = Some(cancel);
        self.capture_active = true;
        self.capture_cancelling = false;
        self.capture_reports.clear();
        self.capture_count = 0;
        self.capture_result = None;
        self.message = None;
        self.error = None;
    }

    fn latest_report(&self) -> Option<&Vec<u8>> {
        self.capture_reports.back()
    }

    fn add_sample(&mut self) {
        let Some(report) = self.latest_report().cloned() else {
            self.error = Some("当前没有可用报文，请先被动捕获".to_owned());
            return;
        };
        if self.samples.len() >= SAMPLE_LIMIT {
            self.error = Some(format!("最多保留 {SAMPLE_LIMIT} 个样本"));
            return;
        }
        self.samples.push(Sample {
            report,
            shown_percent: self.sample_shown.min(100),
        });
        self.message = Some(format!("已加入第 {} 个样本", self.samples.len()));
        self.error = None;
    }

    fn calibration(&self) -> Result<Option<Calibration>, String> {
        if self.samples.len() < 2 {
            return Ok(None);
        }
        let mut a = None;
        let mut b = None;
        'outer: for i in 0..self.samples.len() {
            for j in i + 1..self.samples.len() {
                if self.samples[i].raw(self.offset) != self.samples[j].raw(self.offset) {
                    a = Some(i);
                    b = Some(j);
                    break 'outer;
                }
            }
        }
        let (Some(i), Some(j)) = (a, b) else {
            return Err("两个样本在所选字节上的原始值相同；请改变设备电量后重新捕获".to_owned());
        };
        let sample = |index: usize| CalibrationSample {
            raw: self.samples[index].raw(self.offset).unwrap_or(0),
            shown_percent: self.samples[index].shown_percent,
        };
        Calibration::two_point(sample(i), sample(j))
            .map(Some)
            .map_err(|err| err.to_string())
    }

    fn match_bytes(&self, input_len: usize) -> Result<Vec<MatchByte>, String> {
        let bytes = hex_to_bytes(&self.match_hex).map_err(|err| err.to_string())?;
        if bytes.is_empty() {
            return Err("匹配字节不能为空".to_owned());
        }
        if self.match_offset + bytes.len() > input_len {
            return Err("匹配字节超出报文长度".to_owned());
        }
        Ok(vec![MatchByte(self.match_offset, self.match_hex.clone())])
    }

    fn build_profile(
        &self,
        info: &HidInterface,
        calibration: Calibration,
    ) -> Result<UserProtocolProfile, String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let binding = binding_for(info).map_err(|err| err.to_string())?;
        let input_len = usize::from(binding.input_len);
        if self.offset >= input_len {
            return Err("电量字节超出报文长度".to_owned());
        }
        let match_bytes = self.match_bytes(input_len)?;
        let id = format!(
            "user.{:04X}_{:04X}_{}_{}",
            binding.vendor_id,
            binding.product_id,
            anybattery_core::now_unix(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let name = if self.name.trim().is_empty() {
            if info.product.trim().is_empty() {
                "手动适配".to_owned()
            } else {
                info.product.clone()
            }
        } else {
            self.name.trim().to_owned()
        };
        let spec = ProtocolSpec {
            schema: 1,
            id,
            name,
            matcher: Matcher {
                vid: Some(format!("{:04X}", binding.vendor_id)),
                pid: Some(format!("{:04X}", binding.product_id)),
                usage_page: Some(format!("{:04X}", binding.usage_page)),
                usage: Some(format!("{:04X}", binding.usage)),
                min_output_len: None,
            },
            request: Request {
                kind: RequestKind::None,
                report_id: None,
                length: None,
                payload_hex: String::new(),
                pad: None,
            },
            response: Response {
                kind: ResponseKind::Input,
                timeout_ms: 600,
                retries: 0,
                match_bytes,
            },
            extract: Extract {
                level: Field {
                    offset: self.offset,
                    value_type: FieldType::U8,
                    scale: calibration.scale,
                    bias: calibration.bias,
                    min: Some(0.0),
                    max: Some(100.0),
                },
                charging: None,
                voltage_mv: None,
            },
            verification: ProtocolVerification::Unverified,
            notes: Some("用户两点校准；被动输入".to_owned()),
        };
        let profile = UserProtocolProfile {
            schema: USER_PROFILE_SCHEMA,
            enabled: true,
            binding,
            spec,
        };
        profile.validate().map_err(|err| err.to_string())?;
        Ok(profile)
    }

    fn preview(&self, info: &HidInterface, calibration: Calibration) -> Result<Preview, String> {
        let profile = self.build_profile(info, calibration)?;
        let mut valid = 0usize;
        let mut first = None;
        for report in &self.capture_reports {
            if let Ok(Some(level)) = profile.extract_level(report) {
                valid += 1;
                if first.is_none() {
                    first = Some((level, report.clone()));
                }
            }
        }
        Ok(Preview {
            valid,
            level: first.as_ref().map(|(level, _)| *level),
            report: first.map(|(_, report)| report),
        })
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        interfaces: &[HidInterface],
        profiles: &[UserProtocolProfile],
        diagnostics: &[String],
        scan_busy: bool,
    ) -> WizardAction {
        self.poll();
        let mut action = WizardAction::None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if widgets::pill_button(ui, p, "返回设置", false).clicked() {
                action = WizardAction::Close;
            }
            ui.label(theme::rich(
                "只读：只监听设备已经发出的输入报文，不发送任何查询/写入命令",
                10.5,
                p.tertiary,
                false,
            ));
        });
        if let Some(error) = &self.error {
            ui.label(theme::rich(error, 11.0, p.red, false));
        } else if let Some(message) = &self.message {
            ui.label(theme::rich(message, 11.0, p.secondary, false));
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                widgets::section_title(ui, p, "1. 选择接口");
                widgets::group(ui, p, |ui| {
                    let candidates: Vec<(usize, String)> = interfaces
                        .iter()
                        .enumerate()
                        .filter_map(|(index, info)| {
                            binding_for(info).ok().map(|binding| {
                                (
                                    index,
                                    format!(
                                        "{:04X}:{:04X} {} · MI {:?} · {:04X}:{:04X} · {} B",
                                        info.vendor_id,
                                        info.product_id,
                                        widgets::truncate(&info.product, 22),
                                        binding.interface_number,
                                        info.usage_page,
                                        info.usage,
                                        info.input_len
                                    ),
                                )
                            })
                        })
                        .collect();
                    if candidates.is_empty() {
                        ui.label(theme::rich(
                            "尚未发现可绑定的接口。请先返回设备页完成一次扫描。",
                            11.5,
                            p.secondary,
                            false,
                        ));
                    } else {
                        let selected_text = self
                            .selected
                            .and_then(|index| candidates.iter().find(|(i, _)| *i == index))
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| "选择 2.4G 接口".to_owned());
                        if self.is_capturing() {
                            ui.label(theme::rich(
                                "捕获中，接口选择已锁定；先取消或等待捕获结束。",
                                10.5,
                                p.tertiary,
                                false,
                            ));
                        } else {
                            let mut chosen = self.selected;
                            ComboBox::from_id_salt("user-profile-interface")
                                .selected_text(selected_text)
                                .width(ui.available_width() - 16.0)
                                .show_ui(ui, |ui| {
                                    for (index, label) in &candidates {
                                        ui.selectable_value(&mut chosen, Some(*index), label);
                                    }
                                });
                            if chosen != self.selected {
                                if let Some(index) = chosen {
                                    self.select_interface(index, interfaces);
                                }
                            }
                        }
                        if let Some(info) = self.selected_info(interfaces) {
                            if let Ok(binding) = binding_for(info) {
                                ui.label(theme::rich(
                                    format!(
                                        "容器 {} · 实例 {} · 输入 {} B",
                                        widgets::truncate(&binding.container_id, 18),
                                        widgets::truncate(&binding.instance_id, 24),
                                        binding.input_len
                                    ),
                                    10.5,
                                    p.tertiary,
                                    false,
                                ));
                            }
                        }
                    }
                });
                ui.add_space(10.0);

                let selected = self.selected_info(interfaces).cloned();
                widgets::section_title(ui, p, "2. 被动捕获");
                widgets::group(ui, p, |ui| {
                    let Some(info) = selected.as_ref() else {
                        ui.label(theme::rich(
                            "先选择一个接口。",
                            11.5,
                            p.secondary,
                            false,
                        ));
                        return;
                    };
                    if self.capture_active {
                        ui.label(theme::rich(
                            format!(
                                "捕获中… 已收到 {} 份报文（约 {} 秒）",
                                self.capture_count,
                                CAPTURE_WINDOW.as_secs()
                            ),
                            11.5,
                            p.text,
                            false,
                        ));
                        if widgets::pill_button(ui, p, "取消捕获", false).clicked() {
                            self.cancel_capture();
                        }
                        if self.capture_cancelling {
                            ui.label(theme::rich("正在取消…", 10.5, p.tertiary, false));
                        }
                    } else {
                        let enabled = !scan_busy;
                        if widgets::pill_button_enabled(ui, p, "开始 4 秒被动捕获", false, enabled)
                            .clicked()
                        {
                            self.start_capture(info, scan_busy);
                        }
                        if scan_busy {
                            ui.label(theme::rich(
                                "扫描进行中；等扫描结束后再捕获，避免争用 HID 接口。",
                                10.5,
                                p.tertiary,
                                false,
                            ));
                        }
                        if let Some(result) = &self.capture_result {
                            match result {
                                Ok(summary) => {
                                    ui.label(theme::rich(
                                        format!(
                                            "捕获完成：{} 份报文，已保留最近 {}",
                                            summary.report_count,
                                            self.capture_reports.len()
                                        ),
                                        10.5,
                                        p.secondary,
                                        false,
                                    ));
                                }
                                Err(err) => {
                                    ui.label(theme::rich(
                                        format!("捕获未完成：{}", err.label_zh()),
                                        10.5,
                                        p.red,
                                        false,
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(report) = self.latest_report() {
                        ui.add_space(4.0);
                        ui.label(theme::rich(
                            format!("最新报文（{} B）：", report.len()),
                            10.5,
                            p.tertiary,
                            false,
                        ));
                        ui.label(
                            egui::RichText::new(hex_dump(report, 32))
                                .monospace()
                                .size(10.5)
                                .color(p.secondary),
                        );
                    }
                });
                ui.add_space(10.0);

                widgets::section_title(ui, p, "3. 样本与两点校准");
                widgets::group(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::rich("厂商软件显示电量", 11.5, p.text, false));
                        ui.add(
                            DragValue::new(&mut self.sample_shown)
                                .range(0..=100)
                                .suffix("%"),
                        );
                        if widgets::pill_button(ui, p, "用最新报文加入样本", false).clicked() {
                            self.add_sample();
                        }
                    });
                    if self.samples.is_empty() {
                        ui.label(theme::rich(
                            "至少需要两个样本，且电量字节不同。每次改变设备电量后重新捕获。",
                            10.5,
                            p.tertiary,
                            false,
                        ));
                    } else {
                        let mut remove_sample = None;
                        for (index, sample) in self.samples.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let raw = sample
                                    .raw(self.offset)
                                    .map(|value| format!("raw {value}"))
                                    .unwrap_or_else(|| "raw —".to_owned());
                                ui.label(theme::rich(
                                    format!(
                                        "{}：{}% · {} · {} B",
                                        index + 1,
                                        sample.shown_percent,
                                        raw,
                                        sample.report.len()
                                    ),
                                    10.5,
                                    p.secondary,
                                    false,
                                ));
                                if widgets::pill_button(ui, p, "删除", false).clicked() {
                                    remove_sample = Some(index);
                                }
                            });
                        }
                        if let Some(index) = remove_sample {
                            self.samples.remove(index);
                        }
                    }
                });
                ui.add_space(10.0);

                widgets::section_title(ui, p, "4. 字段与匹配");
                widgets::group(ui, p, |ui| {
                    let max_offset = selected
                        .as_ref()
                        .map(|info| usize::from(info.input_len).saturating_sub(1))
                        .unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(theme::rich("电量字节 offset", 11.5, p.text, false));
                        ui.add(DragValue::new(&mut self.offset).range(0..=max_offset));
                    });
                    ui.horizontal(|ui| {
                        ui.label(theme::rich("匹配头 offset", 11.5, p.text, false));
                        ui.add(DragValue::new(&mut self.match_offset).range(0..=max_offset));
                        ui.label(theme::rich("匹配字节 hex", 11.5, p.text, false));
                        ui.add(
                            TextEdit::singleline(&mut self.match_hex)
                                .desired_width(100.0)
                                .hint_text("例如 72"),
                        );
                    });
                    ui.label(theme::rich(
                        "匹配头必须稳定出现在每份报文中；留空不会推断。",
                        10.5,
                        p.tertiary,
                        false,
                    ));
                });
                ui.add_space(10.0);

                widgets::section_title(ui, p, "5. 预览与保存");
                widgets::group(ui, p, |ui| {
                    let Some(info) = selected.as_ref() else {
                        ui.label(theme::rich("先选择接口。", 11.5, p.secondary, false));
                        return;
                    };
                    match self.calibration() {
                        Ok(None) => {
                            ui.label(theme::rich(
                                "还需要至少两个样本。",
                                11.5,
                                p.secondary,
                                false,
                            ));
                        }
                        Err(err) => {
                            ui.label(theme::rich(err, 11.0, p.red, false));
                        }
                        Ok(Some(calibration)) => {
                            ui.label(theme::rich(
                                format!(
                                    "校准：level = raw × {:.4} + {:.4}",
                                    calibration.scale, calibration.bias
                                ),
                                11.0,
                                p.text,
                                false,
                            ));
                            match self.preview(info, calibration) {
                                Ok(preview) => {
                                    if preview.valid == 0 {
                                        ui.label(theme::rich(
                                            "当前捕获中没有符合匹配头和长度的有效报文；保存已禁用。",
                                            11.0,
                                            p.red,
                                            false,
                                        ));
                                    } else {
                                        ui.label(theme::rich(
                                            format!(
                                                "预览：{} / {} 份报文可解析，首个电量 {}%",
                                                preview.valid,
                                                self.capture_reports.len(),
                                                preview
                                                    .level
                                                    .map(|level| level.to_string())
                                                    .unwrap_or_else(|| "—".to_owned())
                                            ),
                                            11.0,
                                            p.text,
                                            false,
                                        ));
                                        if let Some(report) = &preview.report {
                                            ui.label(
                                                egui::RichText::new(hex_dump(report, 24))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(p.secondary),
                                            );
                                        }
                                        ui.horizontal(|ui| {
                                            ui.label(theme::rich(
                                                "配置名称",
                                                11.5,
                                                p.text,
                                                false,
                                            ));
                                            ui.add(
                                                TextEdit::singleline(&mut self.name)
                                                    .desired_width(180.0),
                                            );
                                            let can_save = !self.capture_active;
                                            if widgets::pill_button_enabled(
                                                ui,
                                                p,
                                                "保存为未验证配置",
                                                false,
                                                can_save,
                                            )
                                            .clicked()
                                            {
                                                match self.build_profile(info, calibration) {
                                                    Ok(profile) => {
                                                        action = WizardAction::Save(profile);
                                                    }
                                                    Err(err) => {
                                                        self.error = Some(err);
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                                Err(err) => {
                                    ui.label(theme::rich(err, 11.0, p.red, false));
                                }
                            }
                        }
                    }
                });
                ui.add_space(10.0);

                widgets::section_title(ui, p, "已保存的用户配置");
                widgets::group(ui, p, |ui| {
                    if profiles.is_empty() {
                        ui.label(theme::rich("暂无用户配置。", 11.0, p.secondary, false));
                    }
                    for profile in profiles {
                        ui.horizontal(|ui| {
                            ui.label(theme::rich(
                                format!(
                                    "{} · {:04X}:{:04X} · {}",
                                    profile.spec.name,
                                    profile.binding.vendor_id,
                                    profile.binding.product_id,
                                    if profile.enabled { "已启用" } else { "已禁用" }
                                ),
                                10.5,
                                p.text,
                                false,
                            ));
                            if widgets::pill_button(
                                ui,
                                p,
                                if profile.enabled { "禁用" } else { "启用" },
                                false,
                            )
                            .clicked()
                            {
                                action = WizardAction::Toggle {
                                    id: profile.spec.id.clone(),
                                    enabled: !profile.enabled,
                                };
                            }
                            if widgets::pill_button(ui, p, "删除", false).clicked() {
                                self.pending_delete = Some(profile.spec.id.clone());
                            }
                        });
                    }
                    if let Some(id) = self.pending_delete.clone() {
                        ui.horizontal(|ui| {
                            ui.label(theme::rich(
                                format!("确认删除 {id}？",),
                                10.5,
                                p.red,
                                false,
                            ));
                            if widgets::pill_button(ui, p, "确认删除", false).clicked() {
                                self.pending_delete = None;
                                action = WizardAction::Delete(id);
                            }
                            if widgets::pill_button(ui, p, "取消", false).clicked() {
                                self.pending_delete = None;
                            }
                        });
                    }
                });
                if !diagnostics.is_empty() {
                    ui.add_space(8.0);
                    widgets::section_title(ui, p, "配置诊断");
                    for line in diagnostics.iter().rev().take(6) {
                        ui.label(
                            egui::RichText::new(line)
                                .size(10.5)
                                .color(p.tertiary)
                                .monospace(),
                        );
                    }
                }
                ui.add_space(18.0);
            });
        action
    }
}

fn hex_dump(bytes: &[u8], max: usize) -> String {
    let mut out = String::new();
    for (index, byte) in bytes.iter().take(max).enumerate() {
        if index > 0 {
            out.push(if index % 8 == 0 { '\n' } else { ' ' });
        }
        out.push_str(&format!("{index:02X}:{byte:02X}"));
    }
    if bytes.len() > max {
        out.push_str(" …");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface() -> HidInterface {
        HidInterface {
            path: r"\\?\hid#vid_1234&pid_5678&mi_02#test".to_owned(),
            vendor_id: 0x1234,
            product_id: 0x5678,
            version: 0x0100,
            product: "示例接收器".to_owned(),
            manufacturer: "示例厂商".to_owned(),
            usage_page: 0xFF05,
            usage: 0x0001,
            input_len: 16,
            output_len: 0,
            feature_len: 0,
            interface_number: Some(2),
            container_id: Some("container-a".to_owned()),
            instance_id: Some("instance-a".to_owned()),
        }
    }

    fn report(raw: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 16];
        bytes[1] = 0x72;
        bytes[4] = raw;
        bytes
    }

    fn wizard_with_two_samples() -> ProtocolWizard {
        let mut wizard = ProtocolWizard::new();
        wizard.offset = 4;
        wizard.match_offset = 1;
        wizard.match_hex = "72".to_owned();
        wizard.samples = vec![
            Sample {
                report: report(20),
                shown_percent: 80,
            },
            Sample {
                report: report(40),
                shown_percent: 40,
            },
        ];
        wizard.capture_reports.push_back(report(30));
        wizard
    }

    #[test]
    fn two_samples_build_a_reversible_calibration() {
        let wizard = wizard_with_two_samples();
        let calibration = wizard
            .calibration()
            .expect("calibration is valid")
            .expect("two samples");
        assert!((calibration.scale + 2.0).abs() < 0.001);
        assert!((calibration.bias - 120.0).abs() < 0.001);
    }

    #[test]
    fn same_raw_samples_are_rejected() {
        let mut wizard = wizard_with_two_samples();
        wizard.samples[1].report = report(20);
        assert!(wizard.calibration().is_err());
    }

    #[test]
    fn profile_preview_uses_capture_reports() {
        let wizard = wizard_with_two_samples();
        let info = interface();
        let calibration = wizard.calibration().unwrap().unwrap();
        let profile = wizard.build_profile(&info, calibration).unwrap();
        assert_eq!(profile.extract_level(&report(30)).unwrap(), Some(60));
        assert!(profile.extract_level(&report(0)).unwrap().is_none());
        let preview = wizard.preview(&info, calibration).unwrap();
        assert_eq!(preview.valid, 1);
        assert_eq!(preview.level, Some(60));
    }

    #[test]
    fn selection_remaps_by_binding_after_rescan() {
        let mut wizard = wizard_with_two_samples();
        wizard.selected = Some(0);
        let old = vec![interface()];
        let mut other = interface();
        other.product_id = 0x9999;
        let new = vec![other, interface()];
        wizard.on_interfaces_changed(&old, &new);
        assert_eq!(wizard.selected, Some(1));
        assert_eq!(wizard.samples.len(), 2);

        wizard.on_interfaces_changed(&new, &[new[0].clone()]);
        assert_eq!(wizard.selected, None);
        assert!(wizard.samples.is_empty());
    }

    #[test]
    fn empty_match_header_is_rejected() {
        let wizard = wizard_with_two_samples();
        let info = interface();
        let calibration = wizard.calibration().unwrap().unwrap();
        let mut broken = wizard;
        broken.match_hex.clear();
        assert!(broken.build_profile(&info, calibration).is_err());
    }
}
