
use anybattery_core::{Confidence, DeviceKind};
use std::time::{Duration, Instant};

use super::hid;
use super::vendor::VendorReading;

/// HID transport used by the session-based vendor protocols.
///
/// Production wraps an open file handle; tests feed scripted reports through
/// the same session code so retry, drain and confirmation ordering is what is
/// exercised, not just parser vectors.
trait VendorTransport {
    fn write_output(&mut self, report: &[u8]) -> bool;
    fn read_input(&mut self, length: usize, timeout_ms: u32) -> Option<Vec<u8>>;
    fn set_feature(&mut self, report: &[u8]) -> bool;

    fn drain(&mut self, length: usize, timeout_ms: u32, attempts: usize) {
        for _ in 0..attempts {
            if self.read_input(length, timeout_ms).is_none() {
                return;
            }
        }
    }
}

struct HandleTransport(windows::Win32::Foundation::HANDLE);

impl VendorTransport for HandleTransport {
    fn write_output(&mut self, report: &[u8]) -> bool {
        hid::write_report(self.0, report)
    }

    fn read_input(&mut self, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        hid::read_report(self.0, length, timeout_ms)
    }

    fn set_feature(&mut self, report: &[u8]) -> bool {
        hid::feature_set_on(self.0, report)
    }
}

/// How much of `window` is left before `deadline`, or None when it passed.
fn bounded_window(deadline: Instant, window: Duration) -> Option<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    (!remaining.is_zero()).then_some(window.min(remaining))
}

/// Clamp a blocking read so it cannot outlive the caller's deadline window.
fn bounded_timeout(window: Duration, read_timeout_ms: u32) -> u32 {
    let window_ms = window.as_millis().min(u32::MAX as u128) as u32;
    read_timeout_ms.min(window_ms.max(1)).max(1)
}

pub fn read_vendor_extra(info: &hid::HidInterface) -> Vec<VendorReading> {
    if let Some(reading) = read_controller(info) {
        return vec![reading];
    }
    if is_1d57_status_interface(info) {
        if let Some(reading) = read_1d57_mouse(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x1532 {
        if let Some(reading) = read_razer_pa(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x0B05 {
        if let Some(reading) = read_asus(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x3434 {
        if let Some(reading) = read_keychron(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x388D {
        if let Some(reading) = read_lofree(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x1E71 {
        if let Some(reading) = read_nzxt(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x1038 && info.usage_page == 0xFFC0 {
        if let Some(reading) = read_steelseries(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x03F0 && info.usage_page == 0xFF90 && info.usage == 0x0303 {
        if let Some(reading) = read_hyperx(info) {
            return vec![reading];
        }
    }
    if info.vendor_id == 0x1B1C && info.is_vendor_collection() {
        if let Some(reading) = read_corsair(info) {
            return vec![reading];
        }
    }
    Vec::new()
}

fn reading(
    level: u8,
    charging: Option<bool>,
    provider: &'static str,
    verification: Confidence,
    name: Option<String>,
    kind: Option<DeviceKind>,
    key: String,
    detail: String,
) -> VendorReading {
    VendorReading {
        level,
        charging,
        provider,
        verification,
        detail,
        name,
        kind,
        key_suffix: Some(key),
    }
}

fn read_controller(info: &hid::HidInterface) -> Option<VendorReading> {
    match info.vendor_id {
        0x28DE => read_steam(info),
        0x054C => read_playstation(info),
        0x057E => read_nintendo(info),
        0x2DC8 => read_eightbitdo(info),
        _ => None,
    }
}

fn parse_steam_battery(report: &[u8]) -> Option<(u8, Option<bool>)> {
    if report.len() < 15 || report[0] != 0x43 {
        return None;
    }
    let charging = match report[1] {
        1 => false,
        2 | 4 => true,
        _ => return None,
    };
    let level = report[2];
    (level <= 100).then_some((level, Some(charging)))
}

fn read_steam(info: &hid::HidInterface) -> Option<VendorReading> {
    let (name, puck) = match info.product_id {
        0x1304 => ("Steam Controller Puck (Proteus)", true),
        0x1305 => ("Steam Controller Puck (Nereid)", true),
        0x1302 => ("Steam Controller (USB)", false),
        0x1303 => ("Steam Controller (Bluetooth)", false),
        _ => return None,
    };
    if puck {
        let iface = info.interface_number?;
        if !(2..=5).contains(&iface) {
            return None;
        }
    }
    if puck && info.usage_page < 0xFF00 && !(info.usage_page == 1 && info.usage == 5) {
        return None;
    }
    let parsed = hid::listen(&info.path, 64, 5000, parse_steam_battery)?;
    let key = format!("steam:{:04X}:{}", info.product_id, info.interface_number.unwrap_or(0));
    Some(reading(
        parsed.0,
        parsed.1,
        "Steam 输入报告",
        Confidence::Verified,
        Some(name.to_owned()),
        Some(DeviceKind::Controller),
        key,
        "report 0x43".to_owned(),
    ))
}

fn parse_dualsense_battery(byte: u8) -> Option<(u8, Option<bool>)> {
    let status = (byte >> 4) & 0x0F;
    let level = byte & 0x0F;
    if level == 0x0C {
        return None;
    }
    match status {
        0 if level <= 10 => Some((level * 10, Some(false))),
        1 if level <= 10 => Some((level * 10, Some(true))),
        2 => Some((100, Some(true))),
        _ => None,
    }
}

fn parse_dual_shock4_battery(byte: u8) -> Option<(u8, Option<bool>)> {
    let level = byte & 0x0F;
    let plugged = byte & 0x10 != 0;
    match level {
        0..=10 => Some((level * 10, Some(plugged))),
        11 => Some((100, Some(plugged))),
        _ => None,
    }
}

/// Battery byte of a PlayStation input report.
///
/// DS4 Bluetooth uses report IDs 0x11 through 0x19 with the same status byte
/// layout (SDL `SDL_hidapi_ps4.c`); only report ID 0x01 is the USB shape.
fn playstation_battery_report(report: &[u8], dualsense: bool) -> Option<(u8, Option<bool>)> {
    if report.len() < 20 {
        return None;
    }
    let offset = if dualsense {
        match report[0] {
            0x01 => 53usize,
            0x31 => 54usize,
            _ => return None,
        }
    } else {
        match report[0] {
            0x01 => 30usize,
            0x11..=0x19 => 32usize,
            _ => return None,
        }
    };
    let byte = *report.get(offset)?;
    if dualsense {
        parse_dualsense_battery(byte)
    } else {
        parse_dual_shock4_battery(byte)
    }
}

fn read_playstation(info: &hid::HidInterface) -> Option<VendorReading> {
    let (name, dualsense) = match info.product_id {
        0x05C4 | 0x09CC | 0x05C5 | 0x0BA0 => ("Sony DualShock 4", false),
        0x0CE6 => ("Sony DualSense", true),
        0x0DF2 => ("Sony DualSense Edge", true),
        _ => return None,
    };
    if info.usage_page != 1 || !matches!(info.usage, 0x04 | 0x05) {
        return None;
    }
    let parsed = hid::listen(&info.path, 78, 1800, |report| {
        playstation_battery_report(report, dualsense)
    })?;
    let key = format!("ps:{:04X}", info.product_id);
    Some(reading(
        parsed.0,
        parsed.1,
        "PlayStation 输入报告",
        Confidence::Verified,
        Some(name.to_owned()),
        Some(DeviceKind::Controller),
        key,
        "listen only / no feature trigger".to_owned(),
    ))
}

fn read_nintendo(info: &hid::HidInterface) -> Option<VendorReading> {
    let name = match info.product_id {
        0x2006 => "Nintendo Joy-Con (L)",
        0x2007 => "Nintendo Joy-Con (R)",
        0x2009 => "Nintendo Switch Pro Controller",
        _ => return None,
    };
    if !hid::is_bluetooth_path(&info.path) {
        return None;
    }
    let parsed = hid::listen(&info.path, 64, 1600, |report| {
        if report.len() < 3 || !matches!(report[0], 0x21 | 0x30 | 0x31) {
            return None;
        }
        let byte = report[2];
        let level = (byte & 0xE0) >> 4;
        if level > 8 {
            return None;
        }
        let charging = byte & 0x10 != 0;
        Some((level * 100 / 8, Some(charging)))
    })?;
    Some(reading(
        parsed.0,
        parsed.1,
        "Nintendo 输入报告",
        Confidence::Verified,
        Some(name.to_owned()),
        Some(DeviceKind::Controller),
        format!("nintendo:{:04X}", info.product_id),
        "report 0x21 / 0x30 / 0x31".to_owned(),
    ))
}

fn eightbitdo_name(product_id: u16) -> Option<&'static str> {
    match product_id {
        0x6000 | 0x6100 => Some("8BitDo SF30 Pro"),
        0x6001 | 0x6101 => Some("8BitDo SN30 Pro"),
        0x6003 | 0x6006 => Some("8BitDo Pro 2"),
        0x6009 => Some("8BitDo Pro 3"),
        0x6012 => Some("8BitDo Ultimate 2 Wireless"),
        _ => None,
    }
}

fn eightbitdo_confidence(product_id: u16) -> Confidence {
    if product_id == 0x6009 {
        Confidence::Verified
    } else {
        Confidence::Community
    }
}

fn parse_eightbitdo_battery(report: &[u8], report_id: u8) -> Option<(u8, Option<bool>)> {
    if report.len() <= 14 || report[0] != report_id {
        return None;
    }
    let byte = report[14];
    let level = byte & 0x7F;
    if !(1..=100).contains(&level) {
        return None;
    }
    Some((level, Some(byte & 0x80 != 0)))
}

fn read_eightbitdo(info: &hid::HidInterface) -> Option<VendorReading> {
    let name = eightbitdo_name(info.product_id)?;
    let bluetooth = hid::is_bluetooth_path(&info.path);
    let report_id = if info.product_id == 0x6009 {
        0x04
    } else if bluetooth {
        0x01
    } else {
        0x04
    };
    let parsed = hid::listen(&info.path, 64, 900, |report| {
        parse_eightbitdo_battery(report, report_id)
    })?;
    Some(reading(
        parsed.0,
        parsed.1,
        "8BitDo 输入报告",
        eightbitdo_confidence(info.product_id),
        Some(name.to_owned()),
        Some(DeviceKind::Controller),
        format!("8bitdo:{:04X}", info.product_id),
        "listen only".to_owned(),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RazerPaLayout {
    report_id: u8,
    marker_offset: usize,
    command_offset: usize,
    ack_offset: usize,
    length_offset: usize,
    sequence_byte: u8,
    allow_notification: bool,
    attempts: u8,
}

const RAZER_PA_BLACKSHARK: RazerPaLayout = RazerPaLayout {
    report_id: 0x02,
    marker_offset: 5,
    command_offset: 12,
    ack_offset: 13,
    length_offset: 14,
    sequence_byte: 0x00,
    allow_notification: false,
    attempts: 3,
};

const RAZER_PA_BARRACUDA: RazerPaLayout = RazerPaLayout {
    report_id: 0x01,
    marker_offset: 3,
    command_offset: 13,
    ack_offset: 14,
    length_offset: 15,
    sequence_byte: 0x08,
    allow_notification: true,
    attempts: 4,
};

const RAZER_PA_SETTLE: Duration = Duration::from_millis(35);
const RAZER_PA_REPLY_WINDOW: Duration = Duration::from_millis(150);
const RAZER_PA_REPLY_TIMEOUT_MS: u32 = 50;

/// Validate a PA reply's `P`/`I` marker, command echo, ACK and declared length.
///
/// The reply may or may not carry the report ID as its first byte. Both offsets
/// are checked against the layout: when the report ID is present the marker sits
/// at `marker_offset`, otherwise one byte earlier.
fn razer_pa_value(reply: &[u8], layout: RazerPaLayout, command: u8) -> Option<u8> {
    let shift = if reply.first() == Some(&layout.report_id)
        && reply.len() > layout.marker_offset + 1
        && reply[layout.marker_offset] == b'P'
        && reply[layout.marker_offset + 1] == b'I'
    {
        0isize
    } else if layout.marker_offset > 0
        && reply.len() > layout.marker_offset
        && reply[layout.marker_offset - 1] == b'P'
        && reply[layout.marker_offset] == b'I'
    {
        -1isize
    } else {
        return None;
    };

    let command_offset = (layout.command_offset as isize + shift) as usize;
    let ack_offset = (layout.ack_offset as isize + shift) as usize;
    let length_offset = (layout.length_offset as isize + shift) as usize;
    if reply.len() <= ack_offset || reply.len() <= length_offset {
        return None;
    }
    if reply[command_offset] != command {
        return None;
    }
    let ack = reply[ack_offset];
    if ack != 0x01 && !(layout.allow_notification && ack == 0x02) {
        return None;
    }
    let length = reply[length_offset] as usize;
    if !(1..=32).contains(&length) {
        return None;
    }
    let data_offset = length_offset + 1;
    let end = data_offset.checked_add(length)?;
    if reply.len() < end {
        return None;
    }
    Some(reply[data_offset])
}

fn parse_razer_pa_level(reply: &[u8], layout: RazerPaLayout, command: u8) -> Option<u8> {
    let level = razer_pa_value(reply, layout, command)?;
    (level <= 100).then_some(level)
}

fn parse_razer_pa_charging(reply: &[u8], layout: RazerPaLayout) -> Option<bool> {
    match razer_pa_value(reply, layout, 0x2A)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn razer_pa_remote(layout: RazerPaLayout, on: bool) -> Vec<u8> {
    let mut frame = vec![0u8; 64];
    frame[0] = layout.report_id;
    frame[1] = 0x80;
    frame[2] = 0x07;
    frame[layout.marker_offset] = b'P';
    frame[layout.marker_offset + 1] = b'A';
    frame[layout.marker_offset + 2] = 0x0E;
    frame[layout.marker_offset + 3] = layout.sequence_byte;
    frame[layout.marker_offset + 4] = 0x02;
    frame[layout.marker_offset + 5] = 0xE1;
    frame[layout.marker_offset + 6] = on as u8;
    frame
}

fn razer_pa_query(layout: RazerPaLayout, command: u8) -> Vec<u8> {
    let mut frame = vec![0u8; 64];
    frame[0] = layout.report_id;
    frame[1] = 0x80;
    frame[2] = 8;
    frame[layout.marker_offset] = b'P';
    frame[layout.marker_offset + 1] = b'A';
    frame[layout.marker_offset + 2] = 0x08;
    frame[layout.marker_offset + 3] = layout.sequence_byte;
    frame[layout.marker_offset + 4] = 0x03;
    frame[layout.marker_offset + 5] = command;
    frame
}

/// One PA command with bounded retries. The radio dozes after ~0.3 s idle, so
/// every attempt re-sends remote-on before the query (upstream behaviour).
fn razer_pa_ask<T>(
    io: &mut impl VendorTransport,
    layout: RazerPaLayout,
    command: u8,
    settle: Duration,
    reply_window: Duration,
    reply_timeout_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    for _ in 0..layout.attempts.max(1) {
        if !io.write_output(&razer_pa_remote(layout, true)) {
            return None;
        }
        if !settle.is_zero() {
            std::thread::sleep(settle);
        }
        if !io.write_output(&razer_pa_query(layout, command)) {
            return None;
        }
        let deadline = Instant::now() + reply_window;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let timeout = bounded_timeout(remaining, reply_timeout_ms);
            let Some(reply) = io.read_input(65, timeout) else {
                continue;
            };
            if let Some(value) = parse(&reply) {
                return Some(value);
            }
        }
    }
    None
}

/// Battery then charging on one handle. Remote-off restores the session state
/// the protocol left behind; it is not a configuration write.
fn razer_pa_read_at(
    io: &mut impl VendorTransport,
    layout: RazerPaLayout,
    settle: Duration,
    reply_window: Duration,
    reply_timeout_ms: u32,
) -> Option<(u8, Option<bool>)> {
    io.drain(65, 5, 32);
    if !io.write_output(&razer_pa_remote(layout, true)) {
        return None;
    }
    if !settle.is_zero() {
        std::thread::sleep(settle);
    }
    let result = (|| {
        let level = razer_pa_ask(io, layout, 0x21, settle, reply_window, reply_timeout_ms, |reply| {
            parse_razer_pa_level(reply, layout, 0x21)
        })?;
        let charging = razer_pa_ask(io, layout, 0x2A, settle, reply_window, reply_timeout_ms, |reply| {
            parse_razer_pa_charging(reply, layout)
        });
        Some((level, charging))
    })();
    let _ = io.write_output(&razer_pa_remote(layout, false));
    result
}

fn razer_pa_read(
    io: &mut impl VendorTransport,
    layout: RazerPaLayout,
) -> Option<(u8, Option<bool>)> {
    razer_pa_read_at(
        io,
        layout,
        RAZER_PA_SETTLE,
        RAZER_PA_REPLY_WINDOW,
        RAZER_PA_REPLY_TIMEOUT_MS,
    )
}

fn razer_pa_collection_ok(product_id: u16, usage_page: u16) -> bool {
    match product_id {
        0x053A => usage_page == 0xFF00,
        0x0555 | 0x0556 => matches!(usage_page, 0xFF00 | 0xFF14),
        _ => false,
    }
}

fn read_razer_pa(info: &hid::HidInterface) -> Option<VendorReading> {
    let (name, layout) = match info.product_id {
        0x053A => ("Razer Barracuda Pro", RAZER_PA_BARRACUDA),
        0x0555 | 0x0556 => ("Razer BlackShark V2 Pro (2023)", RAZER_PA_BLACKSHARK),
        _ => return None,
    };
    if !razer_pa_collection_ok(info.product_id, info.usage_page) {
        return None;
    }
    if info.output_len < 64 && info.feature_len < 64 {
        return None;
    }
    let (level, charging) = hid::with_handle(&info.path, true, |handle| {
        razer_pa_read(&mut HandleTransport(handle), layout)
    })?;
    Some(reading(
        level,
        charging,
        "Razer PA 耳机",
        Confidence::Community,
        Some(name.to_owned()),
        Some(DeviceKind::Headset),
        format!("razer-pa:{:04X}", info.product_id),
        "PA / 0x21 + 0x2A".to_owned(),
    ))
}

fn read_asus(info: &hid::HidInterface) -> Option<VendorReading> {
    if !info.is_vendor_collection() || info.output_len < 65 && info.feature_len < 65 {
        return None;
    }
    let mut request = vec![0u8; 65];
    request[0] = 0x00;
    request[1] = 0x12;
    request[2] = 0x07;
    let reply = hid::write_and_read(&info.path, &request, 65, 300)?;
    let offset = if reply.len() >= 3 && reply[0] == 0x00 && reply[1] == 0x12 && reply[2] == 0x07 {
        1
    } else if reply.len() >= 2 && reply[0] == 0x12 && reply[1] == 0x07 {
        0
    } else {
        return None;
    };
    let level_byte = *reply.get(offset + 4)?;
    let charging = *reply.get(offset + 9).unwrap_or(&0) != 0;
    if level_byte == 0 && !charging {
        return None;
    }
    let steps = matches!(
        info.product_id,
        0x18E5 | 0x18E3 | 0x1960 | 0x195E | 0x1A59 | 0x1A57 | 0x1908 | 0x1906 | 0x1949 | 0x1947
    );
    let level = if steps {
        (level_byte.min(4) * 25).min(100)
    } else if level_byte <= 100 {
        level_byte
    } else {
        return None;
    };
    let name = if info.product.is_empty() {
        "ASUS ROG 无线鼠标".to_owned()
    } else {
        info.product.clone()
    };
    Some(reading(
        level,
        Some(charging),
        "ASUS ROG 0x12 07",
        Confidence::Community,
        Some(name),
        Some(DeviceKind::Mouse),
        format!("asus:{:04X}", info.product_id),
        "cmd 12 07".to_owned(),
    ))
}

fn parse_keychron_battery(reply: &[u8]) -> Option<u8> {
    if reply.len() <= 20 || reply[0] != 0xB4 || reply[1] != 0x06 {
        return None;
    }
    let level = reply[20];
    (level <= 100).then_some(level)
}

const KEYCHRON_INTERFACE: u16 = 4;
const KEYCHRON_ATTEMPTS: u8 = 3;
const KEYCHRON_RETRY_DELAY: Duration = Duration::from_millis(100);
const KEYCHRON_READ_TIMEOUT_MS: u32 = 500;

fn keychron_collection_ok(info: &hid::HidInterface) -> bool {
    info.interface_number == Some(KEYCHRON_INTERFACE)
}

/// Feature Set followed by an input read on the same handle.
///
/// The B4 status report is delivered on the interrupt endpoint of the handle
/// that sent B3 06, so reopening the path between the two loses it. Stale
/// reports are dropped before the first attempt; each attempt waits up to
/// 500 ms and at most three attempts are made.
fn keychron_query(
    io: &mut impl VendorTransport,
    request: &[u8],
    attempts: u8,
    retry_delay: Duration,
    read_timeout_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<u8>,
) -> Option<u8> {
    io.drain(64, 30, 8);
    for attempt in 0..attempts.max(1) {
        if attempt > 0 && !retry_delay.is_zero() {
            std::thread::sleep(retry_delay);
        }
        if !io.set_feature(request) {
            continue;
        }
        let Some(reply) = io.read_input(64, read_timeout_ms) else {
            continue;
        };
        if let Some(level) = parse(&reply) {
            return Some(level);
        }
    }
    None
}

fn read_keychron(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0xD028 && info.product_id != 0xD048 {
        return None;
    }
    if !keychron_collection_ok(info) {
        return None;
    }
    let mut request = vec![0u8; 64];
    request[0] = 0xB3;
    request[1] = 0x06;
    let level = hid::with_handle(&info.path, true, |handle| {
        keychron_query(
            &mut HandleTransport(handle),
            &request,
            KEYCHRON_ATTEMPTS,
            KEYCHRON_RETRY_DELAY,
            KEYCHRON_READ_TIMEOUT_MS,
            parse_keychron_battery,
        )
    })?;
    Some(reading(
        level,
        None,
        "Keychron 0xB3/0xB4",
        Confidence::Community,
        Some(if info.product.is_empty() { "Keychron Ultra-Link".to_owned() } else { info.product.clone() }),
        Some(DeviceKind::Mouse),
        format!("keychron:{:04X}", info.product_id),
        "feature B3 06".to_owned(),
    ))
}

fn read_lofree(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0x0025 {
        return None;
    }
    if info.usage_page != 0xFF1C || info.usage != 0x0092 {
        return None;
    }
    let level = hid::with_handle(&info.path, true, |handle| {
        let mut start = vec![0u8; 64];
        start[0] = 0x04;
        start[3] = 0x01;
        if !hid::write_report(handle, &start) {
            return None;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(700);
        let mut acked = false;
        while std::time::Instant::now() < deadline {
            let Some(reply) = hid::read_report(handle, 64, 100) else {
                continue;
            };
            if reply.len() > 3 && reply[0] == 0x04 && reply[3] == 0x01 {
                acked = true;
                break;
            }
        }
        if !acked {
            return None;
        }
        let mut query = vec![0u8; 64];
        query[0] = 0x04;
        query[3] = 0x1A;
        if !hid::write_report(handle, &query) {
            return None;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(900);
        let mut level = None;
        while std::time::Instant::now() < deadline {
            let Some(reply) = hid::read_report(handle, 64, 100) else {
                continue;
            };
            if reply.len() > 8 && reply[0] == 0x04 && (reply[3] == 0x1A || reply[3] == 0x00) {
                let value = reply[8];
                if value <= 100 {
                    level = Some(value);
                    break;
                }
            }
        }
        let mut end = vec![0u8; 64];
        end[0] = 0x04;
        end[3] = 0x02;
        let _ = hid::write_report(handle, &end);
        level
    })?;
    Some(reading(
        level,
        None,
        "Lofree Hyzen",
        Confidence::Community,
        Some("Lofree Hyzen".to_owned()),
        Some(DeviceKind::Keyboard),
        "lofree:0025".to_owned(),
        "report 0x04 / cmd 1A".to_owned(),
    ))
}

fn read_nzxt(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0x2101 && info.product_id != 0x2129 {
        return None;
    }
    if info.usage_page != 0xFFCA || info.usage != 0x0001 {
        return None;
    }
    let mut request = vec![0u8; 64];
    request[0] = 0x4E;
    request[1] = 0x02;
    request[2] = 0x81;
    request[3] = 0x00;
    request[4] = 0xB0;
    let parsed = hid::with_handle(&info.path, true, |handle| {
        if !hid::write_report(handle, &request) {
            return None;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            let Some(reply) = hid::read_report(handle, 64, 100) else {
                continue;
            };
            if reply.len() >= 23 && reply[0] == 0x4E && reply[2] == 0x97 {
                let level = reply[17] as u16 | ((reply[18] as u16) << 8);
                if level <= 100 {
                    return Some((level as u8, reply[22] & 0x01 != 0));
                }
            }
        }
        None
    })?;
    let (level, charging) = parsed;
    Some(reading(
        level as u8,
        Some(charging),
        "NZXT 4E B0",
        Confidence::Community,
        Some("NZXT Lift Elite".to_owned()),
        Some(DeviceKind::Mouse),
        "nzxt:2101".to_owned(),
        "telemetry property B0".to_owned(),
    ))
}

const STEELSERIES_NOVA7_DISCRETE: [u16; 6] = [0x2202, 0x2206, 0x220A, 0x223A, 0x227A, 0x22A4];
const STEELSERIES_NOVA7_PERCENT: [u16; 8] = [
    0x22A1, 0x227E, 0x2258, 0x229E, 0x22AD, 0x22A9, 0x22A5, 0x22A7,
];
const STEELSERIES_NOVA5: [u16; 2] = [0x2232, 0x2253];

fn is_steelseries_nova(product_id: u16) -> bool {
    STEELSERIES_NOVA7_DISCRETE.contains(&product_id)
        || STEELSERIES_NOVA7_PERCENT.contains(&product_id)
        || STEELSERIES_NOVA5.contains(&product_id)
}

fn parse_steelseries_b0(product_id: u16, reply: &[u8]) -> Option<(u8, Option<bool>)> {
    if reply.len() < 4 || reply[0] != 0xB0 {
        return None;
    }
    if STEELSERIES_NOVA5.contains(&product_id) {
        if reply.len() < 5 || reply[1] == 0x02 {
            return None;
        }
        let level = reply[3];
        return (level <= 100).then_some((level, Some(reply[4] == 0x01)));
    }
    if !STEELSERIES_NOVA7_DISCRETE.contains(&product_id)
        && !STEELSERIES_NOVA7_PERCENT.contains(&product_id)
    {
        return None;
    }
    if reply[1] != 0x03 || reply[3] == 0x00 {
        return None;
    }
    let level = if STEELSERIES_NOVA7_DISCRETE.contains(&product_id) {
        if reply[2] > 4 {
            return None;
        }
        reply[2] * 25
    } else {
        if reply[2] > 100 {
            return None;
        }
        reply[2]
    };
    Some((level, Some(matches!(reply[3], 0x01 | 0x02))))
}

fn read_steelseries(info: &hid::HidInterface) -> Option<VendorReading> {
    if is_steelseries_nova(info.product_id) {
        let mut request = vec![0u8; 64];
        request[1] = 0xB0;
        let reply = hid::write_and_read(&info.path, &request, 64, 600)?;
        let (level, charging) = parse_steelseries_b0(info.product_id, &reply)?;
        let detail = if STEELSERIES_NOVA5.contains(&info.product_id) {
            "Nova 5 reply"
        } else {
            "Nova 7 reply"
        };
        return Some(reading(
            level,
            charging,
            "SteelSeries B0",
            Confidence::Community,
            Some(if info.product.is_empty() {
                "SteelSeries 无线耳机".to_owned()
            } else {
                info.product.clone()
            }),
            Some(DeviceKind::Headset),
            format!("steelseries:{:04X}", info.product_id),
            detail.to_owned(),
        ));
    }
    let mut mouse = vec![0u8; 64];
    mouse[1] = 0xAA;
    mouse[2] = 0x01;
    let reply = hid::write_and_read(&info.path, &mouse, 64, 600)?;
    if reply.len() >= 2 && reply[0] == 0xAA {
        let level = reply[1];
        if level <= 100 {
            return Some(reading(
                level,
                None,
                "SteelSeries AA",
                Confidence::Unverified,
                Some(if info.product.is_empty() { "SteelSeries 无线鼠标".to_owned() } else { info.product.clone() }),
                Some(DeviceKind::Mouse),
                format!("steelseries:{:04X}", info.product_id),
                "AA reply".to_owned(),
            ));
        }
    }
    None
}

fn read_hyperx(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0x0696 && info.product_id != 0x018B {
        return None;
    }
    let mut request = vec![0u8; 52];
    request[0] = 0x06;
    request[1] = 0xFF;
    request[2] = 0xBB;
    request[3] = 0x02;
    let reply = hid::write_and_read(&info.path, &request, 52, 1000)?;
    if reply.len() <= 7 || reply[0] != 0x06 || reply[1] != 0xFF || reply[2] != 0xBB || reply[3] != 0x02 {
        return None;
    }
    let level = reply[7];
    if level > 100 {
        return None;
    }
    let mut charging_request = request.clone();
    charging_request[3] = 0x03;
    let charging = hid::write_and_read(&info.path, &charging_request, 52, 1000)
        .filter(|r| r.len() > 4 && r[0] == 0x06 && r[1] == 0xFF && r[2] == 0xBB && r[3] == 0x03)
        .map(|r| r[4] == 1);
    Some(reading(
        level,
        charging,
        "HyperX Cloud II",
        Confidence::Community,
        Some("HyperX Cloud II Wireless".to_owned()),
        Some(DeviceKind::Headset),
        format!("hyperx:{:04X}", info.product_id),
        "06 FF BB 02".to_owned(),
    ))
}

const CORSAIR_LEVEL_MAX: u16 = 1000;
const CORSAIR_TOTAL_BUDGET: Duration = Duration::from_secs(7);
const CORSAIR_BATTERY_WINDOW: Duration = Duration::from_secs(6);
const CORSAIR_CONFIRM_WINDOW: Duration = Duration::from_secs(3);
const CORSAIR_SESSION_WINDOW: Duration = Duration::from_millis(800);
const CORSAIR_READ_TIMEOUT_MS: u32 = 500;

#[derive(Clone, Copy)]
struct CorsairTimings {
    total: Duration,
    battery: Duration,
    confirm: Duration,
    session: Duration,
    read_timeout_ms: u32,
}

const CORSAIR_TIMINGS: CorsairTimings = CorsairTimings {
    total: CORSAIR_TOTAL_BUDGET,
    battery: CORSAIR_BATTERY_WINDOW,
    confirm: CORSAIR_CONFIRM_WINDOW,
    session: CORSAIR_SESSION_WINDOW,
    read_timeout_ms: CORSAIR_READ_TIMEOUT_MS,
};

fn corsair_bare_frame(product_id: u16) -> bool {
    matches!(product_id, 0x0A40 | 0x0A3E | 0x0A64)
}

fn corsair_runs_session(product_id: u16) -> bool {
    product_id == 0x0A40
}

fn corsair_reads_charge(product_id: u16) -> bool {
    matches!(product_id, 0x0A40 | 0x0A3E | 0x0A64)
}

fn corsair_routes(product_id: u16) -> &'static [u8] {
    match product_id {
        0x1B7F | 0x0A40 => &[0x09],
        0x0A3E | 0x0A64 => &[0x09, 0x08],
        _ => &[],
    }
}

fn corsair_request(product_id: u16, route: u8, property: u8) -> Vec<u8> {
    if corsair_bare_frame(product_id) {
        // Windows refuses the report-id-prefixed form on the Virtuoso
        // receivers; the whole 64-byte frame goes out bare.
        let mut frame = vec![0u8; 64];
        frame[0] = 0x02;
        frame[1] = route;
        frame[2] = 0x02;
        frame[3] = property;
        frame
    } else {
        let mut frame = vec![0u8; 65];
        frame[1] = route;
        frame[2] = 0x02;
        frame[3] = property;
        frame
    }
}

/// Drop the report ID when Windows returns the full 65-byte buffer.
fn corsair_reply_data(reply: &[u8]) -> &[u8] {
    if reply.len() >= 65 {
        &reply[1..]
    } else {
        reply
    }
}

fn parse_corsair_dark_core_raw(reply: &[u8]) -> Option<u16> {
    let data = corsair_reply_data(reply);
    if data.len() < 5 || data[0] != 0x01 || data[1] != 0x02 || data[2] != 0x00 {
        return None;
    }
    Some(data[3] as u16 | ((data[4] as u16) << 8))
}

fn parse_corsair_virtuoso_raw(reply: &[u8]) -> Option<u16> {
    let data = corsair_reply_data(reply);
    if data.len() < 6 || data[0] != 0x01 || data[1] != 0x01 || data[2] != 0x02 || data[3] != 0x00 {
        return None;
    }
    Some(data[4] as u16 | ((data[5] as u16) << 8))
}

fn corsair_level(raw: u16) -> Option<u8> {
    if raw == 0 || raw > CORSAIR_LEVEL_MAX {
        return None;
    }
    Some((raw / 10).min(100) as u8)
}

fn parse_corsair_dark_core_level(reply: &[u8]) -> Option<u8> {
    corsair_level(parse_corsair_dark_core_raw(reply)?)
}

fn parse_corsair_virtuoso_level(reply: &[u8]) -> Option<u8> {
    corsair_level(parse_corsair_virtuoso_raw(reply)?)
}

fn corsair_parse_level(product_id: u16, reply: &[u8]) -> Option<u8> {
    if product_id == 0x1B7F {
        parse_corsair_dark_core_level(reply)
    } else {
        parse_corsair_virtuoso_level(reply)
    }
}

/// Virtuoso property 0x10: 1 = charging, 2 = on battery.
fn parse_corsair_virtuoso_charging(reply: &[u8]) -> Option<bool> {
    match parse_corsair_virtuoso_raw(reply)? {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// One get-property ask inside its own bounded window.
///
/// `Err(())` means the write was refused, so the caller must not keep reading
/// a channel that still belongs to another app.
fn corsair_ask<T>(
    io: &mut impl VendorTransport,
    request: &[u8],
    window: Duration,
    read_timeout_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Result<Option<T>, ()> {
    if !io.write_output(request) {
        return Err(());
    }
    let deadline = Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let timeout = bounded_timeout(remaining, read_timeout_ms);
        let Some(reply) = io.read_input(65, timeout) else {
            continue;
        };
        if let Some(value) = parse(&reply) {
            return Ok(Some(value));
        }
    }
}

/// The measured 0A40 wake-up: firmware query, receiver heartbeat, headset
/// heartbeat, each followed by a short read window. Silent replies are fine;
/// only a refused write aborts the session.
fn corsair_session(
    io: &mut impl VendorTransport,
    product_id: u16,
    deadline: Instant,
    timings: &CorsairTimings,
) -> bool {
    for (property, route) in [(0x13, 0x08), (0x12, 0x08), (0x12, 0x09)] {
        let Some(window) = bounded_window(deadline, timings.session) else {
            return true;
        };
        if !io.write_output(&corsair_request(product_id, route, property)) {
            return false;
        }
        let end = Instant::now() + window;
        loop {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let _ = io.read_input(65, bounded_timeout(remaining, timings.read_timeout_ms));
        }
    }
    true
}

fn corsair_query(
    io: &mut impl VendorTransport,
    product_id: u16,
    timings: &CorsairTimings,
    deadline: Instant,
) -> Option<(u8, Option<bool>)> {
    io.drain(65, 30, 4);
    if corsair_runs_session(product_id) && !corsair_session(io, product_id, deadline, timings) {
        return None;
    }

    let mut first = None;
    let mut answered_route = 0x09;
    for route in corsair_routes(product_id) {
        let Some(window) = bounded_window(deadline, timings.battery) else {
            break;
        };
        answered_route = *route;
        let answer = match corsair_ask(
            io,
            &corsair_request(product_id, answered_route, 0x0F),
            window,
            timings.read_timeout_ms,
            |reply| corsair_parse_level(product_id, reply),
        ) {
            Ok(value) => value,
            Err(()) => return None,
        };
        if answer.is_some() {
            first = answer;
            break;
        }
    }
    let first = first?;

    // A second ask must agree. A silent second ask keeps the single reading
    // (the same fallback the measured implementation uses); a disagreement is
    // refused because another app can put a foreign frame on this channel.
    let second = match bounded_window(deadline, timings.confirm) {
        Some(window) => match corsair_ask(
            io,
            &corsair_request(product_id, answered_route, 0x0F),
            window,
            timings.read_timeout_ms,
            |reply| corsair_parse_level(product_id, reply),
        ) {
            Ok(value) => value,
            Err(()) => return None,
        },
        None => None,
    };
    if let Some(second) = second {
        if second != first {
            return None;
        }
    }

    let charging = if corsair_reads_charge(product_id) {
        match bounded_window(deadline, timings.confirm) {
            Some(window) => match corsair_ask(
                io,
                &corsair_request(product_id, answered_route, 0x10),
                window,
                timings.read_timeout_ms,
                parse_corsair_virtuoso_charging,
            ) {
                Ok(value) => value,
                Err(()) => None,
            },
            None => None,
        }
    } else {
        None
    };
    Some((first, charging))
}

fn read_corsair(info: &hid::HidInterface) -> Option<VendorReading> {
    // 0xFF42:0x0001 is the channel that answers; 0xFF42:0x0002 is the dongle's
    // notice channel and must not receive property requests.
    if info.usage_page != 0xFF42 || info.usage != 0x0001 {
        return None;
    }
    let (name, kind, detail) = match info.product_id {
        0x1B7F => (
            "Corsair Dark Core RGB Pro SE",
            DeviceKind::Mouse,
            "battery property 0x0F",
        ),
        0x0A40 => (
            "Corsair Virtuoso RGB Wireless SE",
            DeviceKind::Headset,
            "battery 0x0F + charge 0x10",
        ),
        0x0A3E => (
            "Corsair Virtuoso SE",
            DeviceKind::Headset,
            "battery 0x0F + charge 0x10",
        ),
        0x0A64 => (
            "Corsair Virtuoso XT",
            DeviceKind::Headset,
            "battery 0x0F + charge 0x10",
        ),
        _ => return None,
    };
    let deadline = Instant::now() + CORSAIR_TIMINGS.total;
    let (level, charging) = hid::with_handle(&info.path, true, |handle| {
        corsair_query(
            &mut HandleTransport(handle),
            info.product_id,
            &CORSAIR_TIMINGS,
            deadline,
        )
    })?;
    Some(reading(
        level,
        charging,
        "Corsair bragi",
        Confidence::Community,
        Some(name.to_owned()),
        Some(kind),
        format!("corsair:{:04X}", info.product_id),
        detail.to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// 1D57:FA60 2.4G receiver: passive Beken / X11 / X6 status reports.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MouseBatteryReport {
    Direct {
        device_id: u8,
        state: u8,
        raw: u8,
        level: u8,
    },
    Scaled { device_id: u8, state: u8, raw: u8 },
}

impl MouseBatteryReport {
    fn level(self) -> u8 {
        match self {
            MouseBatteryReport::Direct { level, .. } => level,
            MouseBatteryReport::Scaled { raw, .. } => raw * 10,
        }
    }

    fn charging(self) -> Option<bool> {
        match self {
            // 0x24 state 01 is the M618Air level frame. Other direct/scaled
            // models keep charging unknown.
            MouseBatteryReport::Direct {
                device_id: 0x24,
                state: 0x01,
                ..
            } => Some(false),
            _ => None,
        }
    }

    fn detail(self) -> String {
        match self {
            MouseBatteryReport::Direct {
                device_id,
                state,
                raw,
                level,
            } => format!(
                "report 03 {:02X} 40 {:02X} · raw {} · {}%",
                device_id, state, raw, level
            ),
            MouseBatteryReport::Scaled {
                device_id,
                state,
                raw,
            } => format!(
                "report 03 {:02X} 40 {:02X} · raw {} · {}%",
                device_id,
                state,
                raw,
                raw * 10
            ),
        }
    }
}

/// M618Air 0x24 state 01: the official handler at 0x4290b0 takes byte[4] and
/// passes it to 0x421b90, which maps an accepted level of 1..8 to the coarse
/// UI percentages 10/20/40/60/80/100 and saturates everything >= 8 at 100.
/// The handler rejects 0 before the helper; this parser does the same.
fn parse_m618air_level(raw: u8) -> Option<u8> {
    if raw == 0 || raw > 100 {
        return None;
    }
    Some(match raw {
        1 => 10,
        2 => 20,
        3 => 40,
        4 | 5 => 60,
        6 | 7 => 80,
        _ => 100,
    })
}

fn parse_1d57_battery(report: &[u8]) -> Option<MouseBatteryReport> {
    if report.len() < 5 || report[0] != 0x03 || report[2] != 0x40 {
        return None;
    }
    let device_id = report[1];
    let state = report[3];
    let raw = report[4];
    match device_id {
        0x55 => (state == 0x01 && raw <= 100).then_some(MouseBatteryReport::Direct {
            device_id,
            state,
            raw,
            level: raw,
        }),
        0x10 => (raw <= 10).then_some(MouseBatteryReport::Scaled { device_id, state, raw }),
        0x85 => (0 < raw && raw <= 10)
            .then_some(MouseBatteryReport::Scaled { device_id, state, raw }),
        0x24 => match state {
            0x01 => parse_m618air_level(raw).map(|level| MouseBatteryReport::Direct {
                device_id,
                state,
                raw,
                level,
            }),
            // 0x4290b0 ignores state entirely. The state 02/03 charging
            // branch belongs to a different handler (0x429583 inside
            // 0x429460), so it is not evidence for M618Air. Do not claim a
            // charging meaning or a percentage here.
            _ => None,
        },
        _ => (raw <= 100).then_some(MouseBatteryReport::Direct {
            device_id,
            state,
            raw,
            level: raw,
        }),
    }
}

fn is_1d57_status_interface(info: &hid::HidInterface) -> bool {
    info.vendor_id == 0x1D57
        && info.product_id == 0xFA60
        && info.interface_number == Some(2)
        && info.usage_page == 0x000A
        && info.usage == 0
        && info.input_len == 5
}

fn read_1d57_mouse(info: &hid::HidInterface) -> Option<VendorReading> {
    let report = hid::listen(&info.path, 64, 3000, parse_1d57_battery)?;
    Some(VendorReading {
        level: report.level(),
        charging: report.charging(),
        provider: "1D57 被动状态",
        verification: Confidence::Community,
        detail: report.detail(),
        name: Some("1D57 2.4G 无线鼠标".to_owned()),
        kind: Some(DeviceKind::Mouse),
        key_suffix: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CorsairTimings, MouseBatteryReport, RAZER_PA_BARRACUDA, RAZER_PA_BLACKSHARK,
        corsair_query, corsair_request, eightbitdo_confidence,
        eightbitdo_name, keychron_collection_ok, keychron_query, parse_1d57_battery,
        parse_corsair_dark_core_level, parse_corsair_virtuoso_charging,
        parse_corsair_virtuoso_level, parse_dual_shock4_battery, parse_dualsense_battery,
        parse_eightbitdo_battery, parse_keychron_battery, parse_razer_pa_charging,
        parse_razer_pa_level, parse_steam_battery, parse_steelseries_b0,
        playstation_battery_report, razer_pa_query, razer_pa_read_at, razer_pa_remote,
    };
    use anybattery_core::Confidence;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    /// Records the production session's writes and replays scripted reports.
    /// Reports queued in `stale` are only visible before the first write, which
    /// is exactly the state a drain is meant to clear.
    struct ScriptedTransport {
        writes: Vec<Vec<u8>>,
        features: Vec<Vec<u8>>,
        replies: VecDeque<Vec<u8>>,
        stale: VecDeque<Vec<u8>>,
        read_calls: Vec<(usize, u32)>,
        respond_after: usize,
        started: bool,
    }

    impl ScriptedTransport {
        fn new(replies: Vec<Vec<u8>>) -> Self {
            Self {
                writes: Vec::new(),
                features: Vec::new(),
                replies: replies.into(),
                stale: VecDeque::new(),
                read_calls: Vec::new(),
                respond_after: 0,
                started: false,
            }
        }
    }

    impl super::VendorTransport for ScriptedTransport {
        fn write_output(&mut self, report: &[u8]) -> bool {
            self.started = true;
            self.writes.push(report.to_vec());
            true
        }

        fn read_input(&mut self, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
            self.read_calls.push((length, timeout_ms));
            if !self.started {
                return self.stale.pop_front();
            }
            if self.writes.len() < self.respond_after {
                return None;
            }
            self.replies.pop_front()
        }

        fn set_feature(&mut self, report: &[u8]) -> bool {
            self.started = true;
            self.features.push(report.to_vec());
            true
        }
    }

    fn fast_timings() -> CorsairTimings {
        CorsairTimings {
            total: Duration::from_millis(200),
            battery: Duration::from_millis(20),
            confirm: Duration::from_millis(20),
            session: Duration::from_millis(2),
            read_timeout_ms: 1,
        }
    }

    fn corsair_level_reply(product_id: u16, raw: u16) -> Vec<u8> {
        let mut reply = vec![0u8; 65];
        if product_id == 0x1B7F {
            reply[1..6].copy_from_slice(&[0x01, 0x02, 0x00, raw as u8, (raw >> 8) as u8]);
        } else {
            reply[1..7].copy_from_slice(&[
                0x01,
                0x01,
                0x02,
                0x00,
                raw as u8,
                (raw >> 8) as u8,
            ]);
        }
        reply
    }

    fn corsair_charge_reply(state: u8) -> Vec<u8> {
        let mut reply = vec![0u8; 65];
        reply[1..7].copy_from_slice(&[0x01, 0x01, 0x02, 0x00, state, 0x00]);
        reply
    }

    fn keychron_reply(level: u8) -> Vec<u8> {
        let mut reply = vec![0u8; 64];
        reply[0] = 0xB4;
        reply[1] = 0x06;
        reply[20] = level;
        reply
    }

    #[test]
    fn x11_status_reports_percentage() {
        assert_eq!(
            parse_1d57_battery(&[0x03, 0x55, 0x40, 0x01, 73]),
            Some(MouseBatteryReport::Direct {
                device_id: 0x55,
                state: 0x01,
                raw: 73,
                level: 73,
            })
        );
        assert_eq!(
            parse_1d57_battery(&[0x03, 0x55, 0x40, 0x01, 0]),
            Some(MouseBatteryReport::Direct {
                device_id: 0x55,
                state: 0x01,
                raw: 0,
                level: 0,
            })
        );
    }

    #[test]
    fn x6_heartbeat_scales_raw_by_ten() {
        let report = parse_1d57_battery(&[0x03, 0x10, 0x40, 0x01, 7]);
        assert_eq!(
            report,
            Some(MouseBatteryReport::Scaled {
                device_id: 0x10,
                state: 0x01,
                raw: 7,
            })
        );
        assert_eq!(report.map(MouseBatteryReport::level), Some(70));
        assert_eq!(
            parse_1d57_battery(&[0x03, 0x10, 0x40, 0x00, 10])
                .map(MouseBatteryReport::level),
            Some(100)
        );
    }

    #[test]
    fn x6_ack_is_never_battery() {
        assert_eq!(parse_1d57_battery(&[0x03, 0x10, 0x50, 0x01, 7]), None);
    }

    #[test]
    fn m618air_state_01_maps_level_six_to_eighty_percent() {
        // Live frame from the user's M618Air. The official DELUX driver
        // reads 80% for the same frame, matching 0x421b90.
        let report = parse_1d57_battery(&[0x03, 0x24, 0x40, 0x01, 0x06]);
        assert_eq!(
            report,
            Some(MouseBatteryReport::Direct {
                device_id: 0x24,
                state: 0x01,
                raw: 6,
                level: 80,
            })
        );
        assert_eq!(report.map(MouseBatteryReport::level), Some(80));
        assert_eq!(report.and_then(MouseBatteryReport::charging), Some(false));
        assert_eq!(
            report.map(MouseBatteryReport::detail),
            Some("report 03 24 40 01 · raw 6 · 80%".to_owned())
        );
    }

    #[test]
    fn m618air_state_01_level_table_matches_official_helper() {
        for (raw, level) in [
            (1, 10),
            (2, 20),
            (3, 40),
            (4, 60),
            (5, 60),
            (6, 80),
            (7, 80),
            (8, 100),
            (9, 100),
            (100, 100),
        ] {
            let report = parse_1d57_battery(&[0x03, 0x24, 0x40, 0x01, raw]);
            assert_eq!(
                report.map(MouseBatteryReport::level),
                Some(level),
                "raw {raw}"
            );
            assert_eq!(
                report.and_then(MouseBatteryReport::charging),
                Some(false),
                "raw {raw}"
            );
        }
    }

    #[test]
    fn m618air_state_01_rejects_zero_and_above_one_hundred() {
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x01, 0]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x01, 101]), None);
    }

    #[test]
    fn m618air_state_02_03_are_not_percent_readings() {
        for state in [0x02, 0x03] {
            assert_eq!(
                parse_1d57_battery(&[0x03, 0x24, 0x40, state, 0x2a]),
                None,
                "state {state:02X}"
            );
        }
    }

    #[test]
    fn m618air_unknown_states_are_rejected() {
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x80, 0x06]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x00, 0x06]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x04, 0x06]), None);
    }

    #[test]
    fn x85_heartbeat_scales_only_one_to_ten() {
        let report = parse_1d57_battery(&[0x03, 0x85, 0x40, 0x01, 7]);
        assert_eq!(
            report,
            Some(MouseBatteryReport::Scaled {
                device_id: 0x85,
                state: 0x01,
                raw: 7,
            })
        );
        assert_eq!(report.map(MouseBatteryReport::level), Some(70));
        assert_eq!(parse_1d57_battery(&[0x03, 0x85, 0x40, 0x01, 0]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x85, 0x40, 0x01, 11]), None);
    }

    #[test]
    fn other_beken_device_uses_direct_value() {
        assert_eq!(
            parse_1d57_battery(&[0x03, 0x4D, 0x40, 0x01, 80]),
            Some(MouseBatteryReport::Direct {
                device_id: 0x4D,
                state: 0x01,
                raw: 80,
                level: 80,
            })
        );
    }

    #[test]
    fn wrong_header_and_short_reports_are_rejected() {
        assert_eq!(parse_1d57_battery(&[0x02, 0x24, 0x40, 0x01, 50]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x41, 0x01, 50]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x55, 0x40, 0x01]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x55, 0x40, 0x00, 50]), None);
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        assert_eq!(parse_1d57_battery(&[0x03, 0x55, 0x40, 0x01, 101]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x10, 0x40, 0x01, 11]), None);
        assert_eq!(parse_1d57_battery(&[0x03, 0x24, 0x40, 0x01, 101]), None);
    }

    #[test]
    fn steelseries_gen2_percentage_is_not_scaled() {
        assert_eq!(
            parse_steelseries_b0(0x22A1, &[0xB0, 0x03, 0x03, 0x03]),
            Some((3, Some(false)))
        );
        assert_eq!(
            parse_steelseries_b0(0x227E, &[0xB0, 0x03, 0x04, 0x01]),
            Some((4, Some(true)))
        );
        assert_eq!(
            parse_steelseries_b0(0x22A7, &[0xB0, 0x03, 0x64, 0x02]),
            Some((100, Some(true)))
        );
    }

    #[test]
    fn steelseries_original_discrete_levels_scale_by_25() {
        assert_eq!(
            parse_steelseries_b0(0x2202, &[0xB0, 0x03, 0x01, 0x03]),
            Some((25, Some(false)))
        );
        assert_eq!(
            parse_steelseries_b0(0x223A, &[0xB0, 0x03, 0x04, 0x00]),
            None
        );
        assert_eq!(
            parse_steelseries_b0(0x22A4, &[0xB0, 0x03, 0x05, 0x03]),
            None
        );
    }

    #[test]
    fn steelseries_nova5_uses_level_byte_three() {
        assert_eq!(
            parse_steelseries_b0(0x2232, &[0xB0, 0x03, 0x00, 0x55, 0x00]),
            Some((85, Some(false)))
        );
        assert_eq!(
            parse_steelseries_b0(0x2253, &[0xB0, 0x03, 0x00, 0x55, 0x01]),
            Some((85, Some(true)))
        );
        assert_eq!(
            parse_steelseries_b0(0x2232, &[0xB0, 0x02, 0x00, 0x55, 0x00]),
            None
        );
    }

    #[test]
    fn steelseries_rejects_wrong_shape_and_unknown_pids() {
        assert_eq!(
            parse_steelseries_b0(0x22A1, &[0xAA, 0x03, 0x03, 0x03]),
            None
        );
        assert_eq!(
            parse_steelseries_b0(0x22A1, &[0xB0, 0x03, 0x65, 0x03]),
            None
        );
        assert_eq!(
            parse_steelseries_b0(0x2202, &[0xB0, 0x02, 0x01, 0x03]),
            None
        );
        assert_eq!(
            parse_steelseries_b0(0x1234, &[0xB0, 0x03, 0x03, 0x03]),
            None
        );
    }

    #[test]
    fn dualsense_charged_and_unknown_levels() {
        assert_eq!(parse_dualsense_battery(0x25), Some((100, Some(true))));
        assert_eq!(parse_dualsense_battery(0x0C), None);
        assert_eq!(parse_dualsense_battery(0x1C), None);
        assert_eq!(parse_dualsense_battery(0x35), None);
        assert_eq!(parse_dualsense_battery(0x17), Some((70, Some(true))));
        assert_eq!(parse_dualsense_battery(0x07), Some((70, Some(false))));
    }

    #[test]
    fn dualshock4_full_and_error_levels() {
        assert_eq!(parse_dual_shock4_battery(0x15), Some((50, Some(true))));
        assert_eq!(parse_dual_shock4_battery(0x05), Some((50, Some(false))));
        assert_eq!(parse_dual_shock4_battery(0x1B), Some((100, Some(true))));
        assert_eq!(parse_dual_shock4_battery(0x0B), Some((100, Some(false))));
        assert_eq!(parse_dual_shock4_battery(0x1E), None);
        assert_eq!(parse_dual_shock4_battery(0x1F), None);
    }

    #[test]
    fn steam_battery_report_window_parser() {
        let mut discharging = vec![0u8; 15];
        discharging[0] = 0x43;
        discharging[1] = 0x01;
        discharging[2] = 95;
        assert_eq!(parse_steam_battery(&discharging), Some((95, Some(false))));

        let mut charged = vec![0u8; 15];
        charged[0] = 0x43;
        charged[1] = 0x04;
        charged[2] = 100;
        assert_eq!(parse_steam_battery(&charged), Some((100, Some(true))));

        charged[1] = 0x03;
        assert_eq!(parse_steam_battery(&charged), None);
        charged[1] = 0x02;
        charged[2] = 101;
        assert_eq!(parse_steam_battery(&charged), None);
        assert_eq!(parse_steam_battery(&[0x43, 0x01, 0x5F]), None);
    }

    #[test]
    fn eightbitdo_adds_6012_and_keeps_only_pro3_verified() {
        assert_eq!(eightbitdo_name(0x6012), Some("8BitDo Ultimate 2 Wireless"));
        assert_eq!(eightbitdo_confidence(0x6009), Confidence::Verified);
        assert_eq!(eightbitdo_confidence(0x6012), Confidence::Community);
        assert_eq!(eightbitdo_confidence(0x6000), Confidence::Community);
    }

    #[test]
    fn eightbitdo_battery_parser_checks_report_id_and_range() {
        let mut report = [0u8; 15];
        report[0] = 0x01;
        report[14] = 0x80 | 55;
        assert_eq!(
            parse_eightbitdo_battery(&report, 0x01),
            Some((55, Some(true)))
        );
        assert_eq!(parse_eightbitdo_battery(&report, 0x04), None);
        report[14] = 0;
        assert_eq!(parse_eightbitdo_battery(&report, 0x01), None);
    }

    #[test]
    fn keychron_battery_parser_checks_header_and_range() {
        let mut reply = vec![0u8; 64];
        reply[0] = 0xB4;
        reply[1] = 0x06;
        reply[20] = 55;
        assert_eq!(parse_keychron_battery(&reply), Some(55));
        reply[20] = 101;
        assert_eq!(parse_keychron_battery(&reply), None);
        reply[20] = 55;
        reply[0] = 0xB3;
        assert_eq!(parse_keychron_battery(&reply), None);
    }
    fn razer_pa_frame(layout: super::RazerPaLayout, command: u8, value: u8) -> Vec<u8> {
        let mut reply = vec![0u8; 64];
        reply[0] = layout.report_id;
        reply[layout.marker_offset] = b'P';
        reply[layout.marker_offset + 1] = b'I';
        reply[layout.command_offset] = command;
        reply[layout.ack_offset] = 0x01;
        reply[layout.length_offset] = 1;
        reply[layout.length_offset + 1] = value;
        reply
    }

    #[test]
    fn razer_pa_blackshark_parses_level_and_charging() {
        let battery = razer_pa_frame(RAZER_PA_BLACKSHARK, 0x21, 55);
        assert_eq!(
            parse_razer_pa_level(&battery, RAZER_PA_BLACKSHARK, 0x21),
            Some(55)
        );
        let charging = razer_pa_frame(RAZER_PA_BLACKSHARK, 0x2A, 1);
        assert_eq!(
            parse_razer_pa_charging(&charging, RAZER_PA_BLACKSHARK),
            Some(true)
        );
        let discharging = razer_pa_frame(RAZER_PA_BLACKSHARK, 0x2A, 0);
        assert_eq!(
            parse_razer_pa_charging(&discharging, RAZER_PA_BLACKSHARK),
            Some(false)
        );
    }

    #[test]
    fn razer_pa_barracuda_uses_shifted_layout_and_notification_ack() {
        let battery = razer_pa_frame(RAZER_PA_BARRACUDA, 0x21, 97);
        assert_eq!(
            parse_razer_pa_level(&battery, RAZER_PA_BARRACUDA, 0x21),
            Some(97)
        );
        let mut notification = battery.clone();
        notification[RAZER_PA_BARRACUDA.ack_offset] = 0x02;
        assert_eq!(
            parse_razer_pa_level(&notification, RAZER_PA_BARRACUDA, 0x21),
            Some(97)
        );
        let mut unnumbered = battery[1..].to_vec();
        unnumbered[RAZER_PA_BARRACUDA.marker_offset - 1] = b'P';
        unnumbered[RAZER_PA_BARRACUDA.marker_offset] = b'I';
        unnumbered[RAZER_PA_BARRACUDA.command_offset - 1] = 0x21;
        unnumbered[RAZER_PA_BARRACUDA.ack_offset - 1] = 0x01;
        unnumbered[RAZER_PA_BARRACUDA.length_offset - 1] = 1;
        unnumbered[RAZER_PA_BARRACUDA.length_offset] = 97;
        assert_eq!(
            parse_razer_pa_level(&unnumbered, RAZER_PA_BARRACUDA, 0x21),
            Some(97)
        );
    }

    #[test]
    fn razer_pa_rejects_bad_marker_echo_ack_length_and_level() {
        let good = razer_pa_frame(RAZER_PA_BLACKSHARK, 0x21, 55);
        for (index, value) in [
            (RAZER_PA_BLACKSHARK.marker_offset, b'Q'),
            (RAZER_PA_BLACKSHARK.marker_offset + 1, b'J'),
            (RAZER_PA_BLACKSHARK.command_offset, 0x22),
            (RAZER_PA_BLACKSHARK.ack_offset, 0x02),
            (RAZER_PA_BLACKSHARK.length_offset, 0),
        ] {
            let mut reply = good.clone();
            reply[index] = value;
            assert_eq!(
                parse_razer_pa_level(&reply, RAZER_PA_BLACKSHARK, 0x21),
                None
            );
        }
        let mut too_long = good.clone();
        too_long[RAZER_PA_BLACKSHARK.length_offset] = 33;
        assert_eq!(
            parse_razer_pa_level(&too_long, RAZER_PA_BLACKSHARK, 0x21),
            None
        );
        assert_eq!(
            parse_razer_pa_level(&good[..RAZER_PA_BLACKSHARK.length_offset + 1], RAZER_PA_BLACKSHARK, 0x21),
            None
        );
        let mut out_of_range = good;
        out_of_range[RAZER_PA_BLACKSHARK.length_offset + 1] = 101;
        assert_eq!(
            parse_razer_pa_level(&out_of_range, RAZER_PA_BLACKSHARK, 0x21),
            None
        );
    }

    #[test]
    fn razer_pa_requests_match_the_family_layouts() {
        let blackshark_remote = razer_pa_remote(RAZER_PA_BLACKSHARK, true);
        assert_eq!(blackshark_remote.len(), 64);
        assert_eq!(
            &blackshark_remote[..12],
            &[0x02, 0x80, 0x07, 0x00, 0x00, 0x50, 0x41, 0x0E, 0x00, 0x02, 0xE1, 0x01]
        );
        let blackshark_query = razer_pa_query(RAZER_PA_BLACKSHARK, 0x2A);
        assert_eq!(
            &blackshark_query[..13],
            &[0x02, 0x80, 0x08, 0x00, 0x00, 0x50, 0x41, 0x08, 0x00, 0x03, 0x2A, 0x00, 0x00]
        );

        let barracuda_remote = razer_pa_remote(RAZER_PA_BARRACUDA, true);
        assert_eq!(
            &barracuda_remote[..10],
            &[0x01, 0x80, 0x07, 0x50, 0x41, 0x0E, 0x08, 0x02, 0xE1, 0x01]
        );
        let barracuda_query = razer_pa_query(RAZER_PA_BARRACUDA, 0x21);
        assert_eq!(
            &barracuda_query[..9],
            &[0x01, 0x80, 0x08, 0x50, 0x41, 0x08, 0x08, 0x03, 0x21]
        );
    }

    #[test]
    fn razer_pa_charging_rejects_unknown_state() {
        let unknown = razer_pa_frame(RAZER_PA_BLACKSHARK, 0x2A, 2);
        assert_eq!(
            parse_razer_pa_charging(&unknown, RAZER_PA_BLACKSHARK),
            None
        );
        let mut wrong_ack = unknown;
        wrong_ack[RAZER_PA_BLACKSHARK.ack_offset] = 0x00;
        assert_eq!(
            parse_razer_pa_charging(&wrong_ack, RAZER_PA_BLACKSHARK),
            None
        );
    }

    #[test]
    fn corsair_dark_core_parser_validates_route_ack_and_range() {
        let mut reply = vec![0u8; 64];
        reply[..5].copy_from_slice(&[0x01, 0x02, 0x00, 0x26, 0x02]);
        assert_eq!(parse_corsair_dark_core_level(&reply), Some(55));

        let mut framed = vec![0u8; 65];
        framed[0] = 0x00; // report ID as Windows hands it back
        framed[1..6].copy_from_slice(&reply[..5]);
        assert_eq!(parse_corsair_dark_core_level(&framed), Some(55));

        for index in 0..3 {
            let mut bad = reply.clone();
            bad[index] = 0xFF;
            assert_eq!(parse_corsair_dark_core_level(&bad), None);
        }
        assert_eq!(parse_corsair_dark_core_level(&reply[..4]), None);
        reply[3] = 0;
        reply[4] = 0;
        assert_eq!(parse_corsair_dark_core_level(&reply), None);
        reply[3] = 0xE9;
        reply[4] = 0x03;
        assert_eq!(parse_corsair_dark_core_level(&reply), None);
        reply[3] = 0xE8;
        reply[4] = 0x03;
        assert_eq!(parse_corsair_dark_core_level(&reply), Some(100));
    }

    #[test]
    fn corsair_virtuoso_parser_validates_ack_and_charge_property() {
        let mut reply = vec![0u8; 64];
        reply[..6].copy_from_slice(&[0x01, 0x01, 0x02, 0x00, 0xCA, 0x03]);
        assert_eq!(parse_corsair_virtuoso_level(&reply), Some(97));

        let mut framed = vec![0u8; 65];
        framed[0] = 0x01;
        framed[1..7].copy_from_slice(&reply[..6]);
        assert_eq!(parse_corsair_virtuoso_level(&framed), Some(97));

        for index in 0..4 {
            let mut bad = reply.clone();
            bad[index] = 0xFF;
            assert_eq!(parse_corsair_virtuoso_level(&bad), None);
        }
        assert_eq!(parse_corsair_virtuoso_level(&reply[..5]), None);
        reply[4] = 0;
        reply[5] = 0;
        assert_eq!(parse_corsair_virtuoso_level(&reply), None);
        reply[4] = 0xE9;
        reply[5] = 0x03;
        assert_eq!(parse_corsair_virtuoso_level(&reply), None);

        reply[4] = 1;
        reply[5] = 0;
        assert_eq!(parse_corsair_virtuoso_charging(&reply), Some(true));
        reply[4] = 2;
        assert_eq!(parse_corsair_virtuoso_charging(&reply), Some(false));
        reply[4] = 3;
        assert_eq!(parse_corsair_virtuoso_charging(&reply), None);
    }

    #[test]
    fn corsair_requests_keep_family_framing() {
        let dark_core = corsair_request(0x1B7F, 0x09, 0x0F);
        assert_eq!(dark_core.len(), 65);
        assert_eq!(&dark_core[..4], &[0x00, 0x09, 0x02, 0x0F]);
        let virtuoso = corsair_request(0x0A40, 0x09, 0x0F);
        assert_eq!(virtuoso.len(), 64);
        assert_eq!(&virtuoso[..4], &[0x02, 0x09, 0x02, 0x0F]);
        assert_eq!(
            &corsair_request(0x0A40, 0x09, 0x10)[..4],
            &[0x02, 0x09, 0x02, 0x10]
        );
        assert_eq!(
            &corsair_request(0x0A3E, 0x08, 0x0F)[..4],
            &[0x02, 0x08, 0x02, 0x0F]
        );
    }


    #[test]
    fn playstation_bluetooth_report_ids_0x12_to_0x19_share_the_status_offset() {
        for report_id in 0x11..=0x19 {
            let mut report = vec![0u8; 78];
            report[0] = report_id;
            report[32] = 0x05;
            assert_eq!(
                playstation_battery_report(&report, false),
                Some((50, Some(false))),
                "DS4 Bluetooth report {report_id:#04X}"
            );
            report[32] = 0x1B;
            assert_eq!(
                playstation_battery_report(&report, false),
                Some((100, Some(true))),
                "DS4 Bluetooth report {report_id:#04X} plugged"
            );
        }

        let mut usb = vec![0u8; 78];
        usb[0] = 0x01;
        usb[30] = 0x09;
        assert_eq!(playstation_battery_report(&usb, false), Some((90, Some(false))));

        let mut unknown = vec![0u8; 78];
        unknown[0] = 0x1A;
        unknown[32] = 0x05;
        assert_eq!(playstation_battery_report(&unknown, false), None);

        let mut dualsense_bluetooth = vec![0u8; 78];
        dualsense_bluetooth[0] = 0x31;
        dualsense_bluetooth[54] = 0x09;
        assert_eq!(
            playstation_battery_report(&dualsense_bluetooth, true),
            Some((90, Some(false)))
        );
    }

    #[test]
    fn keychron_requires_interface_four() {
        let mut info = super::hid::HidInterface {
            product_id: 0xD028,
            ..Default::default()
        };
        info.interface_number = Some(3);
        assert!(!keychron_collection_ok(&info));
        info.interface_number = Some(4);
        assert!(keychron_collection_ok(&info));
        info.interface_number = None;
        assert!(!keychron_collection_ok(&info));
    }

    #[test]
    fn keychron_query_drains_and_sets_feature_before_reading_the_same_handle() {
        let request = {
            let mut request = vec![0u8; 64];
            request[0] = 0xB3;
            request[1] = 0x06;
            request
        };
        let mut io = ScriptedTransport::new(vec![keychron_reply(55)]);
        io.stale.push_back(keychron_reply(99));

        let level = keychron_query(
            &mut io,
            &request,
            3,
            Duration::ZERO,
            500,
            parse_keychron_battery,
        );

        assert_eq!(level, Some(55));
        assert_eq!(io.features, vec![request]);
        assert_eq!(
            io.read_calls,
            vec![(64, 30), (64, 30), (64, 500)],
            "one drain, one empty drain probe, then the 500 ms reply read"
        );
    }

    #[test]
    fn keychron_query_stops_after_three_unusable_replies() {
        let request = vec![0xB3, 0x06];
        let mut io = ScriptedTransport::new(vec![
            keychron_reply(101),
            keychron_reply(101),
            keychron_reply(101),
        ]);
        let level = keychron_query(
            &mut io,
            &request,
            3,
            Duration::ZERO,
            500,
            parse_keychron_battery,
        );
        assert_eq!(level, None);
        assert_eq!(io.features.len(), 3);
        assert_eq!(io.read_calls.len(), 4, "drain probe plus three attempts");
    }

    #[test]
    fn razer_pa_retries_and_always_restores_remote_off() {
        let mut io = ScriptedTransport::new(Vec::new());
        let result = razer_pa_read_at(
            &mut io,
            RAZER_PA_BLACKSHARK,
            Duration::ZERO,
            Duration::from_millis(1),
            1,
        );
        assert_eq!(result, None);
        // wake remote-on, then three (remote-on, query) attempts, then off
        assert_eq!(io.writes.len(), 1 + RAZER_PA_BLACKSHARK.attempts as usize * 2 + 1);
        let last = io.writes.last().unwrap();
        assert_eq!(last[RAZER_PA_BLACKSHARK.marker_offset + 6], 0);
        assert_eq!(&last[..3], &[0x02, 0x80, 0x07]);
    }

    #[test]
    fn razer_pa_reads_battery_then_charging_before_remote_off() {
        let mut io = ScriptedTransport::new(vec![
            razer_pa_frame(RAZER_PA_BLACKSHARK, 0x21, 55),
            razer_pa_frame(RAZER_PA_BLACKSHARK, 0x2A, 1),
        ]);
        let result = razer_pa_read_at(
            &mut io,
            RAZER_PA_BLACKSHARK,
            Duration::ZERO,
            Duration::from_millis(5),
            1,
        );
        assert_eq!(result, Some((55, Some(true))));
        assert_eq!(io.writes.len(), 1 + 2 * 2 + 1);
        let last = io.writes.last().unwrap();
        assert_eq!(last[RAZER_PA_BLACKSHARK.marker_offset + 6], 0);
        assert_eq!(io.read_calls[0], (65, 5), "each read accepts a report id");
    }

    #[test]
    fn corsair_notice_collection_is_not_asked() {
        let info = super::hid::HidInterface {
            vendor_id: 0x1B1C,
            product_id: 0x1B7F,
            usage_page: 0xFF42,
            usage: 0x0002,
            path: String::new(),
            ..Default::default()
        };
        assert!(super::read_vendor_extra(&info).is_empty());
    }

    #[test]
    fn razer_pa_accepts_only_the_captured_vendor_pages() {
        assert!(super::razer_pa_collection_ok(0x053A, 0xFF00));
        assert!(!super::razer_pa_collection_ok(0x053A, 0xFF14));
        assert!(super::razer_pa_collection_ok(0x0555, 0xFF14));
        assert!(super::razer_pa_collection_ok(0x0556, 0xFF00));
        assert!(!super::razer_pa_collection_ok(0x0556, 0x0001));
    }

    #[test]
    fn razer_pa_capture_fixtures_use_literal_offsets() {
        let mut blackshark = vec![0u8; 64];
        blackshark[0] = 0x02;
        blackshark[1] = 0x80;
        blackshark[5..7].copy_from_slice(b"PI");
        blackshark[12] = 0x21;
        blackshark[13] = 0x01;
        blackshark[14] = 1;
        blackshark[15] = 55;
        assert_eq!(
            parse_razer_pa_level(&blackshark, RAZER_PA_BLACKSHARK, 0x21),
            Some(55)
        );

        let mut barracuda = vec![0u8; 64];
        barracuda[0] = 0x01;
        barracuda[1] = 0x80;
        barracuda[3..5].copy_from_slice(b"PI");
        barracuda[13] = 0x2A;
        barracuda[14] = 0x01;
        barracuda[15] = 1;
        barracuda[16] = 1;
        assert_eq!(
            parse_razer_pa_charging(&barracuda, RAZER_PA_BARRACUDA),
            Some(true)
        );
    }

    #[test]
    fn corsair_requires_two_equal_answers() {
        let mut timings = fast_timings();
        timings.battery = Duration::from_millis(50);
        timings.confirm = Duration::from_millis(50);
        let deadline = Instant::now() + Duration::from_millis(500);

        let mut agree = ScriptedTransport::new(vec![
            corsair_level_reply(0x1B7F, 550),
            corsair_level_reply(0x1B7F, 550),
        ]);
        assert_eq!(
            corsair_query(&mut agree, 0x1B7F, &timings, deadline),
            Some((55, None))
        );
        assert_eq!(agree.writes.len(), 2);

        let mut disagree = ScriptedTransport::new(vec![
            corsair_level_reply(0x1B7F, 550),
            corsair_level_reply(0x1B7F, 700),
        ]);
        assert_eq!(
            corsair_query(&mut disagree, 0x1B7F, &timings, deadline),
            None
        );

        let mut silent_confirm =
            ScriptedTransport::new(vec![corsair_level_reply(0x1B7F, 550)]);
        assert_eq!(
            corsair_query(&mut silent_confirm, 0x1B7F, &timings, deadline),
            Some((55, None)),
            "a silent second ask keeps the single measured reading"
        );
    }

    #[test]
    fn corsair_drains_stale_reports_before_asking() {
        let mut io = ScriptedTransport::new(vec![
            corsair_level_reply(0x1B7F, 550),
            corsair_level_reply(0x1B7F, 550),
        ]);
        io.stale.push_back(corsair_level_reply(0x1B7F, 990));
        let deadline = Instant::now() + Duration::from_millis(500);
        assert_eq!(
            corsair_query(&mut io, 0x1B7F, &fast_timings(), deadline),
            Some((55, None))
        );
        assert_eq!(io.writes[0][..4], [0x00, 0x09, 0x02, 0x0F]);
    }

    #[test]
    fn corsair_0a40_runs_the_measured_session_and_confirms_before_charge() {
        let mut io = ScriptedTransport::new(vec![
            corsair_level_reply(0x0A40, 970),
            corsair_level_reply(0x0A40, 970),
            corsair_charge_reply(1),
        ]);
        // The session's three wake writes happen before the battery request;
        // replies only become visible once the battery ask has been written.
        io.respond_after = 4;
        let deadline = Instant::now() + Duration::from_millis(500);
        assert_eq!(
            corsair_query(&mut io, 0x0A40, &fast_timings(), deadline),
            Some((97, Some(true)))
        );
        let heads: Vec<&[u8]> = io.writes.iter().map(|write| &write[..4]).collect();
        assert_eq!(
            heads,
            vec![
                &[0x02, 0x08, 0x02, 0x13][..],
                &[0x02, 0x08, 0x02, 0x12][..],
                &[0x02, 0x09, 0x02, 0x12][..],
                &[0x02, 0x09, 0x02, 0x0F][..],
                &[0x02, 0x09, 0x02, 0x0F][..],
                &[0x02, 0x09, 0x02, 0x10][..],
            ]
        );
        assert_eq!(io.writes[0].len(), 64, "Virtuoso takes the bare frame");
    }

    #[test]
    fn corsair_total_deadline_bounds_session_and_asks() {
        let timings = CorsairTimings {
            total: Duration::from_millis(5),
            battery: Duration::from_secs(6),
            confirm: Duration::from_secs(3),
            session: Duration::from_secs(1),
            read_timeout_ms: 1,
        };
        let started = Instant::now();
        let deadline = started + timings.total;
        let mut io = ScriptedTransport::new(Vec::new());
        assert_eq!(corsair_query(&mut io, 0x0A40, &timings, deadline), None);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the total deadline must not be multiplied per collection"
        );
        assert!(
            io.writes.len() <= 2,
            "a spent deadline stops after the current session write"
        );
    }
}
