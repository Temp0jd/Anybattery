use anybattery_core::{Confidence, DeviceKind};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super::hid::{self, HidInterface};

#[derive(Clone, Debug)]
pub struct VendorReading {
    pub level: u8,
    pub charging: Option<bool>,
    pub provider: &'static str,
    pub verification: Confidence,
    pub detail: String,
    pub name: Option<String>,
    pub kind: Option<DeviceKind>,
    pub key_suffix: Option<String>,
}

impl Default for VendorReading {
    fn default() -> Self {
        Self {
            level: 0,
            charging: None,
            provider: "",
            verification: Confidence::Unverified,
            detail: String::new(),
            name: None,
            kind: None,
            key_suffix: None,
        }
    }
}

pub fn read_vendor(info: &HidInterface, oem_paths: &HashSet<String>) -> Vec<VendorReading> {
    if info.vendor_id == 0x046D {
        let readings = read_logitech(info);
        if !readings.is_empty() {
            return readings;
        }
    }
    if oem_paths.contains(&info.path) {
        if let Some(reading) = read_oem_mouse(info) {
            return vec![reading];
        }
    }
    if let Some(reading) = read_vendor_single(info) {
        return vec![reading];
    }
    super::extra::read_vendor_extra(info)
}

fn read_vendor_single(info: &HidInterface) -> Option<VendorReading> {
    if !info.is_vendor_collection() && info.feature_len < 90 {
        return None;
    }
    if is_aula_keyboard(info) {
        return read_aula(info);
    }
    if info.vendor_id == 0x1532 && info.product_id == 0x0528 {
        return read_razer_blackshark_2020(info);
    }
    if info.vendor_id == 0x1532 && razer_uses_special_protocol(info.product_id) {
        return None;
    }
    if info.vendor_id == 0x1532 {
        if let Some(reading) = read_razer(info) {
            return Some(reading);
        }
    }
    if is_compX_vid(info.vendor_id) {
        if let Some(reading) = read_compX_17(info) {
            return Some(reading);
        }
        if info.usage_page == 0xFF05 {
            if let Some(reading) = read_compX_64(info) {
                return Some(reading);
            }
        }
    }
    if info.vendor_id == 0x3057 && info.usage_page == 0xFF05 {
        if let Some(reading) = read_vaxee(info) {
            return Some(reading);
        }
    }
    if info.vendor_id == 0x3367 && info.usage_page == 0xFF01 {
        if let Some(reading) = read_endgame(info) {
            return Some(reading);
        }
    }
    if info.vendor_id == 0x1915 && info.usage_page == 0xFFA0 {
        if let Some(reading) = read_nordic(info) {
            return Some(reading);
        }
    }
    None
}

fn is_compX_vid(vid: u16) -> bool {
    matches!(vid, 0x3554 | 0x373B | 0x3770 | 0x3710)
}

// Razer PIDs that must never take the 90-byte generic path: 0x0528 uses a
// 64-byte 0xFF feature report, 0x0557 is the wired BlackShark V2 X USB, and
// 0x053A/0x0555/0x0556 speak the PA protocol in extra.rs.
fn razer_uses_special_protocol(pid: u16) -> bool {
    matches!(pid, 0x0528 | 0x053A | 0x0555 | 0x0556 | 0x0557)
}

// ---------------------------------------------------------------------------
// Razer: 90-byte feature report, command class 0x07.
// ---------------------------------------------------------------------------

fn razer_name(pid: u16) -> Option<(&'static str, u8, Confidence)> {
    let verified = Confidence::Verified;
    let community = Confidence::Community;
    Some(match pid {
        // 0x0555/0x0556 are routed to the PA protocol in extra.rs, which marks
        // them Community; keep the generic table consistent if it is ever read.
        0x0555 => ("Razer BlackShark V2 Pro (2023)", 0x3F, community),
        0x0556 => ("Razer BlackShark V2 Pro (2023)", 0x3F, community),
        0x053A => ("Razer Barracuda Pro (2.4 GHz)", 0x3F, community),
        0x0290 => ("Razer DeathStalker V2 Pro", 0x9F, community),
        0x0292 => ("Razer DeathStalker V2 Pro", 0x1F, community),
        0x0296 => ("Razer DeathStalker V2 Pro TKL", 0x9F, verified),
        0x0298 => ("Razer DeathStalker V2 Pro TKL", 0x1F, community),
        0x0271 => ("Razer BlackWidow V3 Mini HyperSpeed", 0x9F, community),
        0x0258 => ("Razer BlackWidow V3 Mini HyperSpeed", 0x1F, community),
        0x02BA => ("Razer BlackWidow V4 Mini HyperSpeed", 0x9F, community),
        0x02B9 => ("Razer BlackWidow V4 Mini HyperSpeed", 0x1F, community),
        0x02D5 => ("Razer BlackWidow V4 Tenkeyless HyperSpeed", 0x9F, community),
        0x02D7 => ("Razer BlackWidow V4 Tenkeyless HyperSpeed", 0x1F, community),
        0x025A => ("Razer BlackWidow V3 Pro", 0x3F, community),
        0x025C => ("Razer BlackWidow V3 Pro", 0x9F, community),
        0x00A4 => ("Razer Mouse Dock Pro", 0x1F, community),
        0x001F => ("Razer Naga Epic", 0xFF, community),
        0x0024 => ("Razer Mamba (2012)", 0xFF, community),
        0x0025 => ("Razer Mamba (2012)", 0xFF, community),
        0x0032 => ("Razer Ouroboros", 0xFF, community),
        0x003E => ("Razer Naga Epic Chroma", 0xFF, community),
        0x003F => ("Razer Naga Epic Chroma", 0xFF, community),
        0x0044 => ("Razer Mamba", 0xFF, community),
        0x0045 => ("Razer Mamba", 0xFF, community),
        0x0059 => ("Razer Lancehead", 0x3F, community),
        0x005A => ("Razer Lancehead", 0x3F, community),
        0x0062 => ("Razer Atheris", 0x1F, community),
        0x006F => ("Razer Lancehead Wireless", 0x1F, community),
        0x0070 => ("Razer Lancehead Wireless", 0x1F, community),
        0x0072 => ("Razer Mamba Wireless", 0x3F, community),
        0x0073 => ("Razer Mamba Wireless", 0x3F, community),
        0x0077 => ("Razer Pro Click", 0x1F, community),
        0x0080 => ("Razer Pro Click", 0x1F, community),
        0x007A => ("Razer Viper Ultimate", 0xFF, community),
        0x007B => ("Razer Viper Ultimate", 0xFF, community),
        0x007C => ("Razer DeathAdder V2 Pro", 0x3F, community),
        0x007D => ("Razer DeathAdder V2 Pro", 0x3F, community),
        0x0083 => ("Razer Basilisk X HyperSpeed", 0xFF, community),
        0x0086 => ("Razer Basilisk Ultimate", 0x1F, community),
        0x0088 => ("Razer Basilisk Ultimate", 0x1F, community),
        0x008F => ("Razer Naga Pro", 0x1F, community),
        0x0090 => ("Razer Naga Pro", 0x1F, community),
        0x0094 => ("Razer Orochi V2", 0x1F, community),
        0x009A => ("Razer Pro Click Mini", 0x1F, community),
        0x009C => ("Razer DeathAdder V2 X HyperSpeed", 0x1F, community),
        0x009E => ("Razer Viper Mini SE", 0x1F, community),
        0x009F => ("Razer Viper Mini SE", 0x1F, community),
        0x00A5 => ("Razer Viper V2 Pro", 0x1F, community),
        0x00A6 => ("Razer Viper V2 Pro", 0x1F, community),
        0x00A7 => ("Razer Naga V2 Pro", 0x1F, community),
        0x00A8 => ("Razer Naga V2 Pro", 0x1F, community),
        0x00AA => ("Razer Basilisk V3 Pro", 0x1F, community),
        0x00AB => ("Razer Basilisk V3 Pro", 0x1F, community),
        0x00AF => ("Razer Cobra Pro", 0x1F, community),
        0x00B0 => ("Razer Cobra Pro", 0x1F, community),
        0x00B3 => ("Razer HyperPolling Dongle", 0x1F, community),
        0x00B4 => ("Razer Naga V2 HyperSpeed", 0x1F, community),
        0x00B6 => ("Razer DeathAdder V3 Pro", 0x1F, community),
        0x00B7 => ("Razer DeathAdder V3 Pro", 0x1F, community),
        0x00B8 => ("Razer Viper V3 HyperSpeed", 0x1F, community),
        0x00B9 => ("Razer Basilisk V3 X HyperSpeed", 0x1F, community),
        0x00BE => ("Razer DeathAdder V4 Pro", 0x1F, community),
        0x00BF => ("Razer DeathAdder V4 Pro", 0x1F, verified),
        0x00C0 => ("Razer Viper V3 Pro", 0x1F, community),
        0x00C1 => ("Razer Viper V3 Pro", 0x1F, community),
        0x00C2 => ("Razer DeathAdder V3 Pro", 0x1F, community),
        0x00C3 => ("Razer DeathAdder V3 Pro", 0x1F, community),
        0x00C4 => ("Razer DeathAdder V3 HyperSpeed", 0x1F, community),
        0x00C5 => ("Razer DeathAdder V3 HyperSpeed", 0x1F, community),
        0x00C7 => ("Razer Pro Click V2 Vertical Edition", 0x1F, community),
        0x00C8 => ("Razer Pro Click V2 Vertical Edition", 0x1F, community),
        0x00CC => ("Razer Basilisk V3 Pro 35K", 0x1F, community),
        0x00CD => ("Razer Basilisk V3 Pro 35K", 0x1F, community),
        0x00D0 => ("Razer Pro Click V2", 0x1F, community),
        0x00D1 => ("Razer Pro Click V2", 0x1F, community),
        0x00D3 => ("Razer Basilisk Mobile", 0x1F, community),
        0x00D4 => ("Razer Basilisk Mobile", 0x1F, community),
        0x00D6 => ("Razer Basilisk V3 Pro 35K Phantom Green", 0x1F, community),
        0x00D7 => ("Razer Basilisk V3 Pro 35K Phantom Green", 0x1F, community),
        _ => return None,
    })
}

fn build_razer_request(transaction_id: u8, class: u8, command: u8) -> [u8; 91] {
    let mut buffer = [0u8; 91];
    buffer[0] = 0;
    let msg = &mut buffer[1..];
    msg[1] = transaction_id;
    msg[5] = 0x02;
    msg[6] = class;
    msg[7] = command;
    let mut crc = 0u8;
    for byte in &msg[2..88] {
        crc ^= *byte;
    }
    msg[88] = crc;
    buffer
}

fn normalize_razer_reply(reply: &[u8]) -> Option<Vec<u8>> {
    if reply.len() >= 91 {
        Some(reply[1..91].to_vec())
    } else if reply.len() >= 90 {
        Some(reply[..90].to_vec())
    } else {
        None
    }
}

fn razer_query(info: &HidInterface, tid: u8, class: u8, command: u8) -> Option<(u8, u8)> {
    let request = build_razer_request(tid, class, command);
    if !hid::feature_set(&info.path, &request) {
        return None;
    }
    std::thread::sleep(Duration::from_millis(60));
    let deadline = std::time::Instant::now() + Duration::from_millis(1000);
    while std::time::Instant::now() < deadline {
        let reply = hid::feature_get(&info.path, 91)?;
        let data = normalize_razer_reply(&reply)?;
        let status = data[0];
        if data[6] == class && data[7] == command {
            return Some((status, data[9]));
        }
        if status != 0x01 && status != 0x02 {
            return Some((status, 0));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

fn read_razer(info: &HidInterface) -> Option<VendorReading> {
    let (name, preferred_tid, verification) = razer_name(info.product_id)
        .or_else(|| {
            let lower = info.product.to_lowercase();
            let wireless = ["hyperspeed", "wireless", "receiver", "dongle", "blackshark", "barracuda"]
                .iter()
                .any(|word| lower.contains(word));
            wireless.then_some((info.product.as_str(), 0x1F, Confidence::Unverified))
        })
        ?;
    let mut level = None;
    let mut used_tid = preferred_tid;
    for tid in [preferred_tid, 0x1F, 0x9F, 0x3F, 0xFF, 0x08] {
        if let Some((status, raw)) = razer_query(info, tid, 0x07, 0x80) {
            if status == 0x02 {
                let value = ((raw as u32 * 100 + 127) / 255) as u8;
                if value <= 100 {
                    level = Some(value);
                    used_tid = tid;
                    break;
                }
            }
        }
    }
    let level = level?;
    let charging = razer_query(info, used_tid, 0x07, 0x84)
        .filter(|(status, _)| *status == 0x02)
        .map(|(_, raw)| raw != 0);
    Some(VendorReading {
        level,
        charging,
        provider: "Razer 90-byte",
        verification,
        detail: format!("{} · tid={:#04X}", name, used_tid),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Razer BlackShark V2 Pro (2020), receiver 1532:0528: 64-byte Feature 0xFF.
// ---------------------------------------------------------------------------

const RAZER_0528_REPORT_LEN: usize = 64;
const RAZER_0528_PREFIX: [u8; 9] = [0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05];
const RAZER_0528_NO_REPLY: [u8; 9] = [0xFF, 0x01, 0x00, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Razer0528Reply {
    state: u8,
    millivolts: u16,
    level: u8,
}

fn razer_0528_candidate(info: &HidInterface) -> bool {
    info.vendor_id == 0x1532
        && info.product_id == 0x0528
        && info.usage_page == 0xFF00
        && info.usage == 0x01
        && info.feature_len as usize == RAZER_0528_REPORT_LEN
}

fn razer_0528_request() -> [u8; RAZER_0528_REPORT_LEN] {
    let mut request = [0u8; RAZER_0528_REPORT_LEN];
    request[..9].copy_from_slice(&[0xFF, 0x0A, 0x00, 0xFD, 0x04, 0x12, 0xF1, 0x02, 0x05]);
    request
}

fn parse_razer_0528(report: &[u8]) -> Option<Razer0528Reply> {
    if report.len() != RAZER_0528_REPORT_LEN {
        return None;
    }
    if report.starts_with(&RAZER_0528_NO_REPLY) {
        return None;
    }
    if !report.starts_with(&RAZER_0528_PREFIX) {
        return None;
    }
    let state = report[11];
    let millivolts = u16::from_be_bytes([report[12], report[13]]);
    let level = report[14];
    if !(2500..=5000).contains(&millivolts) || level > 100 {
        return None;
    }
    Some(Razer0528Reply {
        state,
        millivolts,
        level,
    })
}

// Only these states were observed on 1532:0528. Unknown states keep the fresh
// level but leave charging unknown instead of guessing from individual bits.
fn razer_0528_charging(state: u8) -> Option<bool> {
    match state {
        0x09 => Some(true),
        0x01 | 0x02 | 0x03 | 0x06 => Some(false),
        _ => None,
    }
}

fn read_razer_blackshark_2020(info: &HidInterface) -> Option<VendorReading> {
    if !razer_0528_candidate(info) {
        return None;
    }
    let request = razer_0528_request();
    let reply = hid::feature_set_get(
        &info.path,
        &request,
        RAZER_0528_REPORT_LEN,
        100,
        5,
        parse_razer_0528,
    )?;
    let charging = razer_0528_charging(reply.state);
    let mut detail = format!(
        "BlackShark V2 Pro (2020) · state {:#04X} · {} mV · raw {}",
        reply.state, reply.millivolts, reply.level
    );
    if charging == Some(true) {
        // Upstream hides the level while actively charging because the
        // device-reported value is unreliable (10% can jump to 80%). Keep
        // the fresh level visible until the core model can represent a
        // charging-only reading, but do not present it as exact.
        detail.push_str(" · 充电中电量仅作粗略参考");
    }
    Some(VendorReading {
        level: reply.level,
        charging,
        provider: "Razer 64-byte 0xFF",
        verification: Confidence::Community,
        detail,
        name: Some("Razer BlackShark V2 Pro (2020)".to_owned()),
        kind: Some(DeviceKind::Headset),
        key_suffix: None,
    })
}

// ---------------------------------------------------------------------------
// AULA 3554:FA09 keyboard receiver: 20-byte output report 0x13, status 0x0A.
// ---------------------------------------------------------------------------

fn is_aula_keyboard(info: &HidInterface) -> bool {
    info.vendor_id == 0x3554
        && info.product_id == 0xFA09
        && info.usage_page == 0xFF02
        && info.input_len >= 20
        && info.output_len >= 20
}

fn aula_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

fn aula_query() -> [u8; 20] {
    let mut frame = [0u8; 20];
    frame[0] = 0x13;
    frame[1] = 0x05;
    frame[2] = 0x01;
    frame[19] = aula_checksum(&frame[..19]);
    frame
}

fn parse_aula_battery(report: &[u8]) -> Option<u8> {
    if report.len() != 20 || report[0] != 0x13 || report[1] != 0x0A {
        return None;
    }
    if aula_checksum(&report[..19]) != report[19] {
        return None;
    }
    if report[7] != 1 {
        return None;
    }
    let level = report[6];
    (level <= 100).then_some(level)
}

fn read_aula(info: &HidInterface) -> Option<VendorReading> {
    let request = aula_query();
    let level = hid::write_and_listen(
        &info.path,
        &request,
        info.input_len as usize,
        2000,
        parse_aula_battery,
    )?;
    Some(VendorReading {
        level,
        charging: None,
        provider: "0x13 键盘状态",
        verification: Confidence::Community,
        detail: "report 0x13 / 0x0A · 在线".to_owned(),
        name: Some("0x13 2.4G 键盘".to_owned()),
        kind: Some(DeviceKind::Keyboard),
        key_suffix: None,
    })
}

// ---------------------------------------------------------------------------
// CompX family: 17-byte output frames (Pulsar/ATK/VXE/G-Wolves/Hitscan).
// ---------------------------------------------------------------------------

fn compX_name(vid: u16, pid: u16) -> Option<(&'static str, Confidence)> {
    let verified = Confidence::Verified;
    let community = Confidence::Community;
    Some(match (vid, pid) {
        (0x3554, 0xF508) => ("Pulsar X2 V2 Mini (2.4G)", verified),
        (0x3554, 0xF507) => ("Pulsar X2 V2 Mini (USB)", verified),
        (0x3554, 0xF58F) => ("ATK VXE R1 SE+ (USB)", verified),
        (0x3554, 0xF58A) => ("VXE R1 Pro Max (2.4G)", verified),
        (0x3554, 0xF58C) => ("VXE R1 Pro Max (USB)", verified),
        (0x373B, 0x1085) => ("ATK VXE R1 SE+ (2.4G)", verified),
        (0x3770, 0x0200) => ("Hitscan Hyperlight (2.4G)", community),
        (0x3770, 0x0100) => ("Hitscan Hyperlight (USB)", community),
        (0x3710, 0x5406) => ("Pulsar X2 CrazyLight Mini (2.4G)", verified),
        (0x3710, 0x3414) => ("Pulsar X2 CrazyLight Mini (USB)", verified),
        _ => return None,
    })
}

fn compX_17_frame() -> [u8; 17] {
    let mut frame = [0u8; 17];
    frame[0] = 0x08;
    frame[1] = 0x04;
    let sum: u16 = frame[..16].iter().map(|b| *b as u16).sum();
    frame[16] = ((0x55u16.wrapping_sub(sum)) & 0xFF) as u8;
    frame
}

fn compX_17_parse(reply: &[u8]) -> Option<(u8, bool, u16)> {
    if reply.len() < 17 {
        return None;
    }
    let frame = &reply[..17];
    if frame[0] != 0x08 || frame[1] != 0x04 {
        return None;
    }
    let sum: u16 = frame[..16].iter().map(|b| *b as u16).sum();
    if frame[16] != ((0x55u16.wrapping_sub(sum)) & 0xFF) as u8 {
        return None;
    }
    let level = frame[6];
    if level > 100 {
        return None;
    }
    let charging = frame[7] != 0;
    let millivolts = u16::from_be_bytes([frame[8], frame[9]]);
    Some((level, charging, millivolts))
}

fn read_compX_17(info: &HidInterface) -> Option<VendorReading> {
    let (name, verification) = compX_name(info.vendor_id, info.product_id)?;
    if info.output_len < 17 {
        return None;
    }
    let request = compX_17_frame();
    for _ in 0..3 {
        let Some(reply) = hid::write_and_read(&info.path, &request, 17, 250) else {
            continue;
        };
        if let Some((level, charging, millivolts)) = compX_17_parse(&reply) {
            return Some(VendorReading {
                level,
                charging: Some(charging),
                provider: "CompX 17-byte",
                verification,
                detail: format!("{} · {} mV", name, millivolts),
                ..Default::default()
            });
        }
    }
    None
}

fn compX_64_frame() -> [u8; 64] {
    let mut frame = [0u8; 64];
    frame[0] = 0x08;
    frame[1..8].copy_from_slice(&[0x7D, 0x72, 0x02, 0x00, 0x01, 0x07, 0x01]);
    frame
}

fn read_compX_64(info: &HidInterface) -> Option<VendorReading> {
    let (name, _) = compX_name(info.vendor_id, info.product_id)?;
    if info.output_len < 64 {
        return None;
    }
    let request = compX_64_frame();
    for _ in 0..3 {
        let Some(reply) = hid::write_and_read(&info.path, &request, 64, 300) else {
            continue;
        };
        if reply.len() >= 8 && reply[1] == 0x72 && reply[5] == 0x07 {
            let level = reply[7];
            if level <= 100 {
                return Some(VendorReading {
                    level,
                    charging: None,
                    provider: "CompX 64-byte",
                    verification: Confidence::Community,
                    detail: name.to_owned(),
                    ..Default::default()
                });
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Small feature-report families.
// ---------------------------------------------------------------------------

fn feature_roundtrip(info: &HidInterface, request: &[u8], length: usize) -> Option<Vec<u8>> {
    if !hid::feature_set(&info.path, request) {
        return None;
    }
    std::thread::sleep(Duration::from_millis(80));
    hid::feature_get(&info.path, length)
}

const VAXEE_REPORT_LEN: usize = 64;

fn vaxee_candidate(info: &HidInterface) -> bool {
    info.vendor_id == 0x3057
        && info.usage_page == 0xFF05
        && info.feature_len as usize == VAXEE_REPORT_LEN
}

fn vaxee_request(page: u8) -> [u8; VAXEE_REPORT_LEN] {
    let mut request = [0u8; VAXEE_REPORT_LEN];
    request[..5].copy_from_slice(&[0x0E, 0xA5, page, 0x01, 0x01]);
    request
}

// Byte 5 is a 0..20 step, not a percentage. Reject anything outside 0..=20
// instead of clamping it into a plausible-looking value.
fn parse_vaxee_battery(reply: &[u8]) -> Option<u8> {
    if reply.len() != VAXEE_REPORT_LEN || !reply.starts_with(&[0x0E, 0xA5, 0x0B]) {
        return None;
    }
    let raw = reply[5];
    if raw > 20 {
        return None;
    }
    Some(raw * 5)
}

// The 0x10 page reports charging explicitly: 1 is charging, 0 is not charging.
fn parse_vaxee_charging(reply: &[u8]) -> Option<bool> {
    if reply.len() != VAXEE_REPORT_LEN || !reply.starts_with(&[0x0E, 0xA5, 0x10]) {
        return None;
    }
    match reply[5] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn read_vaxee(info: &HidInterface) -> Option<VendorReading> {
    if !vaxee_candidate(info) {
        return None;
    }
    let level = hid::feature_set_get(
        &info.path,
        &vaxee_request(0x0B),
        VAXEE_REPORT_LEN,
        120,
        2,
        parse_vaxee_battery,
    )?;
    let charging = hid::feature_set_get(
        &info.path,
        &vaxee_request(0x10),
        VAXEE_REPORT_LEN,
        120,
        2,
        parse_vaxee_charging,
    );
    Some(VendorReading {
        level,
        charging,
        provider: "VAXEE/ZYGEN",
        verification: Confidence::Community,
        detail: format!("feature 0x0E · page 0x0B raw {} ×5", level / 5),
        ..Default::default()
    })
}

fn read_endgame(info: &HidInterface) -> Option<VendorReading> {
    let mut request = [0u8; 64];
    request[0] = 0xA1;
    request[1] = 0xB4;
    let reply = feature_roundtrip(info, &request, 64)?;
    let level = *reply.get(16)?;
    if level > 100 {
        return None;
    }
    let voltage = if reply.len() >= 19 {
        Some(u16::from_le_bytes([reply[17], reply[18]]))
    } else {
        None
    };
    Some(VendorReading {
        level,
        charging: None,
        provider: "Endgame Gear",
        verification: Confidence::Community,
        detail: voltage.map(|mv| format!("{} mV", mv)).unwrap_or_default(),
        ..Default::default()
    })
}

fn read_nordic(info: &HidInterface) -> Option<VendorReading> {
    let mut request = [0u8; 32];
    request[..5].copy_from_slice(&[0x05, 0x15, 0x00, 0x00, 0x01]);
    let reply = feature_roundtrip(info, &request, 32)?;
    let level = *reply.get(9)?;
    if level > 100 {
        return None;
    }
    Some(VendorReading {
        level,
        charging: None,
        provider: "Nordic nRF",
        verification: Confidence::Community,
        detail: "feature 0x05".to_owned(),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// OEM mice: PixArt/JM 64-byte feature, MCHOSE inverted 0x06, Delux M800 Pro.
// All routes start as Community until a real device is reproduced locally.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct PixArtReading {
    level: u8,
    charging: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct MchoseReading {
    level: u8,
    charging: bool,
    model: u16,
    flags: u8,
    firmware: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct DeluxReading {
    level: u8,
    charging: bool,
}

fn pixart_name(vid: u16, pid: u16) -> Option<&'static str> {
    Some(match (vid, pid) {
        (0x36A7, 0xA880) => "WLmouse Beast X Max (8K)",
        (0x36A7, 0xA887) => "WLmouse Beast X",
        (0x36A7, 0xA868) => "WLmouse Beast X Mini Pro",
        (0x373E, 0x001E) => "LAMZU Maya X (8K)",
        (0x373E, 0x001C) => "LAMZU Maya X (USB)",
        (0x373E, 0x0046) => "Attack Shark R5 Ultra (USB)",
        (0x373E, 0x0047) => "Attack Shark R5 Ultra (2.4G)",
        (0x2023, 0xF013) => "MCHOSE A5 Pro Max (1K)",
        (0x2023, 0xF015) => "MCHOSE A5 Pro Max (4K)",
        (0x2023, 0xF019) => "MCHOSE A5 Pro Max (USB)",
        _ => return None,
    })
}

fn pixart_product_ok(info: &HidInterface) -> bool {
    let product = info.product.to_ascii_lowercase();
    match (info.vendor_id, info.product_id) {
        (0x373E, 0x001E | 0x001C) => product.contains("lamzu"),
        (0x373E, 0x0046 | 0x0047) => product.contains("attack shark"),
        (0x36A7, 0xA880 | 0xA887 | 0xA868) => true,
        (0x2023, 0xF013 | 0xF015 | 0xF019) => product.contains("mchose"),
        _ => false,
    }
}

fn pixart_candidate(info: &HidInterface) -> bool {
    info.usage_page == 0xFFFF
        && info.feature_len == 65
        && (info.usage == 0 || (info.vendor_id == 0x2023 && info.usage == 1))
        && pixart_interface_ok(info)
}

// LAMZU Maya X's 0xFFFF:0x0000 collection is Interface 2. WLmouse, Attack
// Shark and MCHOSE A5 Pro Max are selected by their usage collection instead
// of a fixed interface number.
fn pixart_interface_ok(info: &HidInterface) -> bool {
    match (info.vendor_id, info.product_id) {
        (0x373E, 0x001E | 0x001C) => info.interface_number == Some(2),
        _ => true,
    }
}

fn pixart_request() -> [u8; 65] {
    let mut request = [0u8; 65];
    request[..7].copy_from_slice(&[0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x83]);
    request
}

// Windows `HidD_GetFeature` returns report ID 0 in raw[0]. The marker must be
// anchored at offset 1; scanning the buffer for the header would accept noise.
fn parse_pixart_reply(raw: &[u8]) -> Option<PixArtReading> {
    if raw.len() != 65 {
        return None;
    }
    if raw[0] != 0x00 {
        return None;
    }
    if !matches!(raw[1], 0xA1 | 0xA2) {
        return None;
    }
    if raw[2] != 0x00 || raw[3] != 0x02 || raw[4] != 0x02 || raw[5] != 0x00 {
        return None;
    }
    if raw[6] != 0x83 {
        return None;
    }
    let charging = match raw[7] {
        0 => false,
        1 => true,
        _ => return None,
    };
    let level = raw[8];
    if level > 100 {
        return None;
    }
    Some(PixArtReading { level, charging })
}

fn read_pixart(info: &HidInterface) -> Option<VendorReading> {
    if !pixart_candidate(info) || !pixart_product_ok(info) {
        return None;
    }
    let name = pixart_name(info.vendor_id, info.product_id)?;
    let request = pixart_request();
    let parsed = hid::feature_poll(
        &info.path,
        &request,
        65,
        15,
        Duration::from_millis(50),
        parse_pixart_reply,
    )?;
    Some(VendorReading {
        level: parsed.level,
        charging: Some(parsed.charging),
        provider: "PixArt/JM 64B",
        verification: Confidence::Community,
        detail: format!(
            "feature 0x83 · raw[7]={} raw[8]={}",
            parsed.charging as u8, parsed.level
        ),
        name: Some(name.to_owned()),
        kind: Some(DeviceKind::Mouse),
        key_suffix: None,
    })
}

fn mchose_name(vid: u16, pid: u16) -> Option<&'static str> {
    Some(match (vid, pid) {
        (0x5253, 0x1020) => "MCHOSE M7 Ultra (2.4G)",
        (0x5253, 0x0031) => "MCHOSE M7 Ultra (USB)",
        (0x3837, 0x100C) => "MCHOSE A5 V2 Ultra (2.4G)",
        (0x3837, 0x1101) => "MCHOSE A5 V2 Ultra (USB)",
        (0x3837, 0x100B) => "MCHOSE A7 V2 Ultra (2.4G)",
        // 0x3837:0x600A is the V9 Turbo+ headset (Input 0x55), never this route.
        _ => return None,
    })
}

fn mchose_candidate(info: &HidInterface) -> bool {
    mchose_name(info.vendor_id, info.product_id).is_some()
        && info.usage_page == 0xFF01
        && info.usage == 1
}

fn mchose_request(report_id: u8, length: usize) -> Vec<u8> {
    let mut request = vec![0xFFu8; length];
    request[0] = report_id;
    request[1] = 0xF9;
    request
}

fn parse_mchose_reply(raw: &[u8], report_id: u8, device_vid: u16) -> Option<MchoseReading> {
    let expected_len = match report_id {
        0x11 => 21,
        0x12 => 65,
        _ => return None,
    };
    if raw.len() != expected_len {
        return None;
    }
    if raw[0] != report_id || raw[1] != 0xF9 {
        return None;
    }
    let payload: Vec<u8> = raw[2..].iter().map(|byte| byte ^ 0xFF).collect();
    if payload.len() < 11 {
        return None;
    }
    let vid = u16::from_le_bytes([payload[0], payload[1]]);
    if vid != device_vid || !matches!(vid, 0x5253 | 0x3837) {
        return None;
    }
    let model = u16::from_le_bytes([payload[2], payload[3]]);
    let firmware = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[8];
    let level = payload[9];
    let charging = match payload[10] {
        0 => false,
        1 => true,
        _ => return None,
    };
    if level > 100 {
        return None;
    }
    Some(MchoseReading {
        level,
        charging,
        model,
        flags,
        firmware,
    })
}

fn read_mchose_inverse(info: &HidInterface) -> Option<VendorReading> {
    if !mchose_candidate(info) || info.feature_len < 21 {
        return None;
    }
    let name = mchose_name(info.vendor_id, info.product_id)?;
    let caps_len = info.feature_len as usize;
    hid::with_handle(&info.path, true, |handle| {
        // Both report IDs share one handle: re-opening between the short and
        // long channels would discard the second channel's reply state.
        for (report_id, protocol_len) in [(0x11u8, 21usize), (0x12u8, 65usize)] {
            if caps_len < protocol_len {
                continue;
            }
            let request = mchose_request(report_id, protocol_len);
            for _ in 0..4 {
                match hid::feature_set_get_caps_on(
                    handle,
                    &request,
                    caps_len,
                    protocol_len,
                    Duration::from_millis(120),
                ) {
                    Ok(reply) => {
                        if let Some(parsed) =
                            parse_mchose_reply(&reply, report_id, info.vendor_id)
                        {
                            return Some(VendorReading {
                                level: parsed.level,
                                charging: Some(parsed.charging),
                                provider: "MCHOSE 反码 Feature",
                                verification: Confidence::Community,
                                detail: format!(
                                    "report {:#04X} · vid {:04X} model {:04X} flags {:02X} fw {:08X}",
                                    report_id, info.vendor_id, parsed.model, parsed.flags, parsed.firmware
                                ),
                                name: Some(name.to_owned()),
                                kind: Some(DeviceKind::Mouse),
                                key_suffix: None,
                            });
                        }
                    }
                    // A failed Set must end this channel, not be retried four
                    // times. The long channel is still allowed one attempt.
                    Err(hid::FeatureIoError::SetFailed)
                    | Err(hid::FeatureIoError::InvalidLength) => break,
                    Err(hid::FeatureIoError::GetFailed) => {}
                }
            }
        }
        None
    })
}

const DELUX_REPORT_LEN: usize = 33;

fn delux_candidate(info: &HidInterface) -> bool {
    info.vendor_id == 0x248A
        && matches!(info.product_id, 0x5B2F | 0x5B2E)
        && info.interface_number == Some(1)
        && info.feature_len as usize == DELUX_REPORT_LEN
}

fn delux_request(attempt: u32) -> [u8; DELUX_REPORT_LEN] {
    let mut request = [0u8; DELUX_REPORT_LEN];
    request[0] = 0x0C;
    request[1] = 0x01;
    request[2] = 0x20;
    request[4] = attempt.wrapping_add(1) as u8;
    request
}

// The source driver's `BATTERY_CHARGING = 0x65` is an internal return value,
// not an on-wire byte; only raw[19] == 1 means charging. raw[18] == 0 means the
// query is not ready yet, not a real 0% reading.
fn parse_delux_reply(raw: &[u8]) -> Option<DeluxReading> {
    if raw.len() != DELUX_REPORT_LEN {
        return None;
    }
    if raw[0] != 0x0C {
        return None;
    }
    let level = raw[18];
    if level == 0 || level > 100 {
        return None;
    }
    let charging = match raw[19] {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DeluxReading { level, charging })
}

fn read_delux_m800(info: &HidInterface) -> Option<VendorReading> {
    if !delux_candidate(info) {
        return None;
    }
    let parsed = hid::feature_exchange(
        &info.path,
        DELUX_REPORT_LEN,
        8,
        Duration::from_millis(30),
        |attempt| delux_request(attempt).to_vec(),
        parse_delux_reply,
    )?;
    Some(VendorReading {
        level: parsed.level,
        charging: Some(parsed.charging),
        provider: "Delux M800 Pro 33B",
        verification: Confidence::Community,
        detail: format!(
            "report 0x0C · raw[18]={} raw[19]={}",
            parsed.level, parsed.charging as u8
        ),
        name: Some("Delux M800 Pro".to_owned()),
        kind: Some(DeviceKind::Mouse),
        key_suffix: None,
    })
}

fn read_oem_mouse(info: &HidInterface) -> Option<VendorReading> {
    read_pixart(info)
        .or_else(|| read_mchose_inverse(info))
        .or_else(|| read_delux_m800(info))
}

/// Pick one interface per physical OEM device before any Feature Set is sent.
///
/// PixArt receivers can expose several 0xFFFF collections: A5 Pro Max reports
/// `ffff:0001` on interface 1 and `ffff:0000` on interface 2. Usage 0 is
/// preferred; usage 1 is only a fallback when the same container has no usage 0
/// candidate. MCHOSE and Delux are likewise reduced to one path per container.
pub fn select_oem_paths(interfaces: &[HidInterface]) -> HashSet<String> {
    let mut groups: HashMap<String, Vec<&HidInterface>> = HashMap::new();
    for info in interfaces {
        groups.entry(oem_group_key(info)).or_default().push(info);
    }
    let mut selected = HashSet::new();
    for group in groups.values() {
        if let Some(path) = select_pixart_path(group) {
            selected.insert(path);
        }
        if let Some(path) = select_mchose_path(group) {
            selected.insert(path);
        }
        if let Some(path) = select_delux_path(group) {
            selected.insert(path);
        }
    }
    selected
}

fn oem_group_key(info: &HidInterface) -> String {
    let identity = info
        .container_id
        .as_deref()
        .or(info.instance_id.as_deref())
        .unwrap_or(info.path.as_str());
    format!(
        "{}:{:04X}:{:04X}",
        identity, info.vendor_id, info.product_id
    )
}

fn interface_rank(info: &HidInterface) -> (u16, &str) {
    (
        info.interface_number.unwrap_or(u16::MAX),
        info.path.as_str(),
    )
}

fn pixart_selectable(info: &HidInterface) -> bool {
    pixart_candidate(info)
        && pixart_product_ok(info)
        && pixart_name(info.vendor_id, info.product_id).is_some()
}

fn select_pixart_path(group: &[&HidInterface]) -> Option<String> {
    let mut candidates: Vec<&HidInterface> = group
        .iter()
        .copied()
        .filter(|info| pixart_selectable(info))
        .collect();
    // Usage 0 always sorts ahead of the 2023 usage-1 fallback; equal usages
    // are ordered by interface/path so selection does not depend on
    // enumeration order.
    candidates.sort_by_key(|info| (u8::from(info.usage != 0), interface_rank(info)));
    candidates.first().map(|info| info.path.clone())
}

fn select_mchose_path(group: &[&HidInterface]) -> Option<String> {
    let mut candidates: Vec<&HidInterface> = group
        .iter()
        .copied()
        .filter(|info| mchose_candidate(info) && info.feature_len >= 21)
        .collect();
    candidates.sort_by_key(|info| interface_rank(info));
    candidates.first().map(|info| info.path.clone())
}

fn select_delux_path(group: &[&HidInterface]) -> Option<String> {
    let mut candidates: Vec<&HidInterface> = group
        .iter()
        .copied()
        .filter(|info| delux_candidate(info))
        .collect();
    candidates.sort_by_key(|info| interface_rank(info));
    candidates.first().map(|info| info.path.clone())
}

const LOGITECH_VID: u16 = 0x046D;
const F_ROOT: u16 = 0x0000;
const F_NAME: u16 = 0x0005;
const F_UNIFIED: u16 = 0x1004;
const F_STATUS: u16 = 0x1000;
const F_VOLTAGE: u16 = 0x1001;
const F_ADC: u16 = 0x1F20;
const BATTERY_FEATURES: [u16; 4] = [F_UNIFIED, F_STATUS, F_VOLTAGE, F_ADC];
const LOGITECH_RECEIVERS: [u16; 23] = [
    0xC548, 0xC52B, 0xC532, 0xC52F, 0xC518, 0xC51A, 0xC51B, 0xC521, 0xC525, 0xC526,
    0xC52E, 0xC531, 0xC534, 0xC535, 0xC537, 0xC539, 0xC53A, 0xC53D, 0xC53F, 0xC541,
    0xC545, 0xC547, 0xC54D,
];
// Solaar/OpenLogi's 13-point curve. Linux uses a different 100-point curve for
// 0x1F20 (3905 mV is about 87% there vs 67% here); keep one documented curve
// until a hardware comparison decides between them.
const VOLTAGE_CURVE: [(u16, u8); 13] = [
    (4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50),
    (3778, 40), (3751, 30), (3717, 20), (3671, 10), (3646, 5), (3579, 2), (3500, 0),
];

fn is_logitech_receiver(info: &HidInterface) -> bool {
    LOGITECH_RECEIVERS.contains(&info.product_id)
        || {
            let name = info.product.to_lowercase();
            name.contains("receiver")
                || name.contains("unifying")
                || name.contains("lightspeed")
                || name.contains("bolt")
        }
}

fn hpp_request(
    handle: windows::Win32::Foundation::HANDLE,
    long: bool,
    idx: u8,
    feature: u8,
    function: u8,
    params: &[u8],
) -> Option<Vec<u8>> {
    let len = if long { 20 } else { 7 };
    let mut request = vec![0u8; len];
    request[0] = if long { 0x11 } else { 0x10 };
    request[1] = idx;
    request[2] = feature;
    request[3] = (function << 4) | 0x0A;
    for (i, byte) in params.iter().enumerate() {
        if 4 + i < len {
            request[4 + i] = *byte;
        }
    }
    if !hid::write_report(handle, &request) {
        return None;
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(350);
    while std::time::Instant::now() < deadline {
        let Some(reply) = hid::read_report(handle, 64, 80) else {
            continue;
        };
        if reply.len() < 4 || reply[1] != idx {
            continue;
        }
        let function_byte = (function << 4) | 0x0A;
        if reply[2] == feature && reply[3] == function_byte {
            let mut payload = reply[4..].to_vec();
            payload.resize(16, 0);
            return Some(payload);
        }
        if (reply[2] == 0x8F || reply[2] == 0xFF) && reply.len() >= 5 {
            return None;
        }
    }
    None
}

fn hpp_feature_index(
    handle: windows::Win32::Foundation::HANDLE,
    long: bool,
    idx: u8,
    feature: u16,
) -> u8 {
    let params = [(feature >> 8) as u8, (feature & 0xFF) as u8];
    hpp_request(handle, long, idx, 0x00, 0, &params)
        .and_then(|p| p.first().copied())
        .unwrap_or(0)
}

// 0x1000/0x1004 status: 0 discharging, 1 charging, 2 almost full, 3 full.
// 4 is slow charging in Solaar/OpenLogi but a charging error in Linux's 0x1004
// path; 5..7 are battery/thermal errors. Leave all of 4..7 unknown.
fn parse_logitech_charge_status(status: u8) -> Option<bool> {
    match status {
        0 => Some(false),
        1..=3 => Some(true),
        _ => None,
    }
}

// 0x1000: 0 is a legal empty charge while discharging, and status 3 (full)
// means 100% even when the device reports 0 there.
fn parse_logitech_battery_status(payload: &[u8]) -> Option<(u8, Option<bool>)> {
    let status = *payload.get(2)?;
    if status > 7 {
        return None;
    }
    let level = if status == 3 { 100 } else { *payload.first()? };
    if level > 100 {
        return None;
    }
    Some((level, parse_logitech_charge_status(status)))
}

// 0x1004 without a state-of-charge gauge reports 0 in byte 0. The capability
// fallback is not implemented in this batch, so only a real percentage is used.
fn parse_logitech_unified(payload: &[u8]) -> Option<(u8, Option<bool>)> {
    let level = *payload.first()?;
    if level == 0 || level > 100 {
        return None;
    }
    let status = *payload.get(2)?;
    if status > 7 {
        return None;
    }
    Some((level, parse_logitech_charge_status(status)))
}

// 0x1001: bit 7 only means external power. Charging is decided by the low
// three status bits, so flags 0x82 (external power, not charging) must not be
// reported as charging. 3..7 are undefined states.
fn parse_logitech_voltage(payload: &[u8]) -> Option<(u8, Option<bool>)> {
    let mv = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    if mv < 2500 {
        return None;
    }
    let flags = *payload.get(2)?;
    if flags & 0x80 == 0 {
        return Some((voltage_to_percent(mv), Some(false)));
    }
    let charging = match flags & 0x07 {
        0 | 1 => Some(true),
        2 => Some(false),
        _ => None,
    };
    Some((voltage_to_percent(mv), charging))
}

// 0x1F20 only defines 0x01 (discharging), 0x03 (charging) and 0x07 (full).
// Linux rejects 0x0F and every other value as unknown instead of treating the
// low bits as a connected/charging pair.
fn parse_logitech_adc(payload: &[u8]) -> Option<(u8, Option<bool>)> {
    let mv = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    if mv < 2500 {
        return None;
    }
    let charging = match *payload.get(2)? {
        0x01 => Some(false),
        0x03 | 0x07 => Some(true),
        _ => return None,
    };
    Some((voltage_to_percent(mv), charging))
}

fn voltage_to_percent(mv: u16) -> u8 {
    if mv >= VOLTAGE_CURVE[0].0 {
        return 100;
    }
    for pair in VOLTAGE_CURVE.windows(2) {
        let (high_mv, high_pct) = pair[0];
        let (low_mv, low_pct) = pair[1];
        if mv >= low_mv {
            let span = (high_mv - low_mv).max(1) as f32;
            let t = (mv - low_mv) as f32 / span;
            let pct = low_pct as f32 + t * (high_pct as f32 - low_pct as f32);
            return pct.round().clamp(0.0, 100.0) as u8;
        }
    }
    0
}

fn logitech_identity(
    handle: windows::Win32::Foundation::HANDLE,
    long: bool,
    idx: u8,
) -> (Option<String>, Option<DeviceKind>) {
    let fi = hpp_feature_index(handle, long, idx, F_NAME);
    if fi == 0 {
        return (None, None);
    }
    let length = hpp_request(handle, long, idx, fi, 0, &[])
        .and_then(|p| p.first().copied())
        .unwrap_or(0) as usize;
    let mut raw = Vec::new();
    let mut offset = 0usize;
    while raw.len() < length.min(64) {
        let Some(chunk) = hpp_request(handle, long, idx, fi, 1, &[offset as u8]) else {
            break;
        };
        if chunk.iter().all(|b| *b == 0) {
            break;
        }
        raw.extend_from_slice(&chunk[..chunk.len().min(16)]);
        offset += 16;
    }
    let name = String::from_utf8_lossy(&raw[..raw.len().min(length)]).trim().to_string();
    let kind = hpp_request(handle, long, idx, fi, 2, &[])
        .and_then(|p| p.first().copied())
        .map(|value| match value {
            0 | 2 => DeviceKind::Keyboard,
            3 | 4 | 5 => DeviceKind::Mouse,
            8 => DeviceKind::Headset,
            _ => DeviceKind::Unknown,
        });
    ((!name.is_empty()).then_some(name), kind)
}

fn read_logitech_slot(info: &HidInterface, idx: u8) -> Option<VendorReading> {
    let long = info.output_len >= 20 || info.feature_len >= 20;
    hid::with_handle(&info.path, true, |handle| {
        if hpp_request(handle, long, idx, 0x00, 1, &[]).is_none() {
            return None;
        }
        let (name, kind) = logitech_identity(handle, long, idx);
        for feature in BATTERY_FEATURES {
            let fi = hpp_feature_index(handle, long, idx, feature);
            if fi == 0 {
                continue;
            }
            let function = if feature == F_UNIFIED { 1 } else { 0 };
            let Some(payload) = hpp_request(handle, long, idx, fi, function, &[]) else {
                continue;
            };
            let parsed = match feature {
                F_UNIFIED => parse_logitech_unified(&payload),
                F_STATUS => parse_logitech_battery_status(&payload),
                F_VOLTAGE => parse_logitech_voltage(&payload),
                F_ADC => parse_logitech_adc(&payload),
                _ => None,
            };
            let Some((level, charging)) = parsed else {
                continue;
            };
            return Some(VendorReading {
                level,
                charging,
                provider: "Logitech HID++",
                verification: Confidence::Verified,
                detail: format!("feature {:04X} · slot {}", feature, idx),
                name,
                kind,
                key_suffix: Some(format!("logitech:{}", idx)),
            });
        }
        None
    })
}

pub fn read_logitech(info: &HidInterface) -> Vec<VendorReading> {
    if info.usage_page < 0xFF00 {
        return Vec::new();
    }
    let receiver = is_logitech_receiver(info);
    let slots: Vec<u8> = if receiver {
        (1..=6).collect()
    } else {
        vec![0xFF]
    };
    let mut out = Vec::new();
    for slot in slots {
        if let Some(reading) = read_logitech_slot(info, slot) {
            out.push(reading);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        aula_checksum, aula_query, compX_name, delux_candidate, delux_request, is_compX_vid,
        mchose_candidate, mchose_request, parse_aula_battery, parse_delux_reply,
        parse_logitech_adc, parse_logitech_battery_status, parse_logitech_unified,
        parse_logitech_voltage, parse_mchose_reply, parse_pixart_reply, parse_razer_0528,
        parse_vaxee_battery, parse_vaxee_charging, pixart_candidate, pixart_interface_ok,
        pixart_name, pixart_product_ok, pixart_request, razer_0528_candidate, razer_0528_charging,
        razer_0528_request, razer_name, razer_uses_special_protocol, select_oem_paths,
        vaxee_candidate,
        vaxee_request, voltage_to_percent, DeluxReading, MchoseReading, PixArtReading,
        Razer0528Reply, LOGITECH_RECEIVERS,
    };
    use crate::hid::HidInterface;
    use anybattery_core::Confidence;

    fn aula_status(level: u8, online: u8) -> [u8; 20] {
        let mut report = [0u8; 20];
        report[0] = 0x13;
        report[1] = 0x0A;
        report[6] = level;
        report[7] = online;
        report[19] = aula_checksum(&report[..19]);
        report
    }

    #[test]
    fn aula_query_matches_documented_frame() {
        let mut expected = [0u8; 20];
        expected[0] = 0x13;
        expected[1] = 0x05;
        expected[2] = 0x01;
        expected[19] = 0x19;
        assert_eq!(aula_query(), expected);
    }

    #[test]
    fn aula_status_packet_reports_percentage() {
        assert_eq!(parse_aula_battery(&aula_status(76, 1)), Some(76));
        assert_eq!(parse_aula_battery(&aula_status(0, 1)), Some(0));
        assert_eq!(parse_aula_battery(&aula_status(100, 1)), Some(100));
    }

    #[test]
    fn aula_bad_checksum_is_rejected() {
        let mut report = aula_status(76, 1);
        report[19] ^= 0x01;
        assert_eq!(parse_aula_battery(&report), None);
    }

    #[test]
    fn aula_echo_and_wrong_header_are_rejected() {
        assert_eq!(parse_aula_battery(&aula_query()), None);
        let mut report = aula_status(76, 1);
        report[0] = 0x14;
        report[19] = aula_checksum(&report[..19]);
        assert_eq!(parse_aula_battery(&report), None);
    }

    #[test]
    fn aula_offline_and_out_of_range_are_rejected() {
        assert_eq!(parse_aula_battery(&aula_status(76, 0)), None);
        assert_eq!(parse_aula_battery(&aula_status(101, 1)), None);
        assert_eq!(parse_aula_battery(&aula_status(76, 1)[..19]), None);
    }

    #[test]
    fn aula_observed_0x05_reply_is_rejected() {
        let observed = [
            0x13, 0x05, 0x01, 0x00, 0x0A, 0x03, 0x00, 0x00, 0x00, 0x00, 0x6E, 0x00, 0x00, 0x20,
            0x10, 0x00, 0x00, 0x00, 0x00, 0xC4,
        ];
        assert_eq!(parse_aula_battery(&observed), None);
    }

    fn razer_confidence(pid: u16) -> Option<Confidence> {
        razer_name(pid).map(|(_, _, confidence)| confidence)
    }

    #[test]
    fn razer_0528_and_0557_skip_the_generic_90_byte_path() {
        assert!(razer_uses_special_protocol(0x0528));
        assert!(razer_uses_special_protocol(0x0557));
        assert!(razer_uses_special_protocol(0x053A));
        assert!(razer_uses_special_protocol(0x0555));
        assert!(razer_uses_special_protocol(0x0556));
        assert!(!razer_uses_special_protocol(0x0558));
        assert_eq!(razer_name(0x0528), None);
        assert_eq!(razer_name(0x0557), None);
    }

    #[test]
    fn razer_confidence_drops_unverified_models() {
        assert_eq!(razer_confidence(0x0556), Some(Confidence::Community));
        assert_eq!(razer_confidence(0x0298), Some(Confidence::Community));
        assert_eq!(razer_confidence(0x02D5), Some(Confidence::Community));
        assert_eq!(razer_confidence(0x02D7), Some(Confidence::Community));
        assert_eq!(razer_confidence(0x00BE), Some(Confidence::Community));
        assert_eq!(razer_confidence(0x0296), Some(Confidence::Verified));
        assert_eq!(razer_confidence(0x00BF), Some(Confidence::Verified));
        assert_eq!(razer_confidence(0x0555), Some(Confidence::Community));
    }

    fn hid_interface(
        vendor_id: u16,
        product_id: u16,
        usage_page: u16,
        usage: u16,
        feature_len: u16,
    ) -> HidInterface {
        HidInterface {
            vendor_id,
            product_id,
            usage_page,
            usage,
            feature_len,
            ..Default::default()
        }
    }

    fn razer_0528_frame(first: [u8; 16]) -> [u8; 64] {
        let mut frame = [0u8; 64];
        frame[..16].copy_from_slice(&first);
        frame
    }

    fn vaxee_reply(prefix: [u8; 3], value: u8) -> [u8; 64] {
        let mut reply = [0u8; 64];
        reply[..3].copy_from_slice(&prefix);
        reply[5] = value;
        reply
    }

    #[test]
    fn razer_0528_candidate_requires_exact_collection_and_length() {
        assert!(razer_0528_candidate(&hid_interface(
            0x1532, 0x0528, 0xFF00, 0x01, 64
        )));
        assert!(!razer_0528_candidate(&hid_interface(
            0x1532, 0x0528, 0xFF00, 0x01, 65
        )));
        assert!(!razer_0528_candidate(&hid_interface(
            0x1532, 0x0528, 0xFF00, 0x02, 64
        )));
        assert!(!razer_0528_candidate(&hid_interface(
            0x1532, 0x0528, 0xFF01, 0x01, 64
        )));
        assert!(!razer_0528_candidate(&hid_interface(
            0x1532, 0x0529, 0xFF00, 0x01, 64
        )));
        assert!(!razer_0528_candidate(&hid_interface(
            0x046D, 0x0528, 0xFF00, 0x01, 64
        )));
    }

    #[test]
    fn razer_0528_request_matches_capture() {
        let request = razer_0528_request();
        assert_eq!(
            &request[..9],
            &[0xFF, 0x0A, 0x00, 0xFD, 0x04, 0x12, 0xF1, 0x02, 0x05]
        );
        assert!(request[9..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn vaxee_requests_target_battery_and_charging_pages() {
        assert_eq!(&vaxee_request(0x0B)[..5], &[0x0E, 0xA5, 0x0B, 0x01, 0x01]);
        assert_eq!(&vaxee_request(0x10)[..5], &[0x0E, 0xA5, 0x10, 0x01, 0x01]);
        assert!(vaxee_request(0x0B)[5..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn razer_0528_reference_frames_parse() {
        let on_battery = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x03, 0x05, 0x01, 0x0E, 0xA0,
            0x32, 0x00,
        ]);
        assert_eq!(
            parse_razer_0528(&on_battery),
            Some(Razer0528Reply {
                state: 0x01,
                millivolts: 3744,
                level: 50,
            })
        );

        let charging = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x05, 0x03, 0x09, 0x10, 0x80,
            0x50, 0x00,
        ]);
        assert_eq!(
            parse_razer_0528(&charging),
            Some(Razer0528Reply {
                state: 0x09,
                millivolts: 4224,
                level: 80,
            })
        );
        assert_eq!(razer_0528_charging(0x09), Some(true));

        let complete = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x06, 0x05, 0x06, 0x10, 0x88,
            0x64, 0x00,
        ]);
        assert_eq!(
            parse_razer_0528(&complete),
            Some(Razer0528Reply {
                state: 0x06,
                millivolts: 4232,
                level: 100,
            })
        );
        assert_eq!(razer_0528_charging(0x06), Some(false));

        let unplugged = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x03, 0x05, 0x01, 0x10, 0x38,
            0x64, 0x00,
        ]);
        assert_eq!(
            parse_razer_0528(&unplugged),
            Some(Razer0528Reply {
                state: 0x01,
                millivolts: 4152,
                level: 100,
            })
        );
    }

    #[test]
    fn razer_0528_stale_no_reply_is_rejected() {
        let stale = razer_0528_frame([
            0xFF, 0x01, 0x00, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x05, 0x03, 0x09, 0x10, 0x88,
            0x50, 0x00,
        ]);
        assert_eq!(parse_razer_0528(&stale), None);
    }

    #[test]
    fn razer_0528_rejects_bad_header_length_and_offline() {
        let valid = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x03, 0x05, 0x01, 0x0E, 0xA0,
            0x32, 0x00,
        ]);

        let mut bad_header = valid;
        bad_header[8] = 0x06;
        assert_eq!(parse_razer_0528(&bad_header), None);
        let mut bad_report_id = valid;
        bad_report_id[0] = 0xFE;
        assert_eq!(parse_razer_0528(&bad_report_id), None);
        assert_eq!(parse_razer_0528(&valid[..63]), None);
        assert_eq!(parse_razer_0528(&[valid.as_slice(), &[0]].concat()), None);

        let mut offline = valid;
        offline[12] = 0;
        offline[13] = 0;
        assert_eq!(parse_razer_0528(&offline), None);

        let mut low_voltage = valid;
        low_voltage[12..14].copy_from_slice(&2499u16.to_be_bytes());
        assert_eq!(parse_razer_0528(&low_voltage), None);
        let mut high_voltage = valid;
        high_voltage[12..14].copy_from_slice(&5001u16.to_be_bytes());
        assert_eq!(parse_razer_0528(&high_voltage), None);

        let mut high_level = valid;
        high_level[14] = 101;
        assert_eq!(parse_razer_0528(&high_level), None);
    }

    #[test]
    fn razer_0528_charging_uses_only_observed_states() {
        assert_eq!(razer_0528_charging(0x09), Some(true));
        for state in [0x01, 0x02, 0x03, 0x06] {
            assert_eq!(razer_0528_charging(state), Some(false), "{state:#04X}");
        }
        for state in [0x00, 0x04, 0x05, 0x07, 0x08, 0x0A, 0xFF] {
            assert_eq!(razer_0528_charging(state), None, "{state:#04X}");
        }
    }

    #[test]
    fn razer_0528_unknown_state_keeps_level_without_charging_claim() {
        let frame = razer_0528_frame([
            0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05, 0x03, 0x05, 0x04, 0x0E, 0xA0,
            0x28, 0x00,
        ]);
        let reply = parse_razer_0528(&frame).unwrap();
        assert_eq!(reply.level, 40);
        assert_eq!(razer_0528_charging(reply.state), None);
    }

    #[test]
    fn vaxee_candidate_requires_exact_collection_and_length() {
        assert!(vaxee_candidate(&hid_interface(
            0x3057, 0x0001, 0xFF05, 0x01, 64
        )));
        assert!(!vaxee_candidate(&hid_interface(
            0x3057, 0x0001, 0xFF05, 0x01, 63
        )));
        assert!(!vaxee_candidate(&hid_interface(
            0x3057, 0x0001, 0xFF06, 0x01, 64
        )));
        assert!(!vaxee_candidate(&hid_interface(
            0x3058, 0x0001, 0xFF05, 0x01, 64
        )));
    }

    #[test]
    fn vaxee_battery_uses_five_percent_steps() {
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 20)), Some(100));
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 16)), Some(80));
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 1)), Some(5));
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 0)), Some(0));
    }

    #[test]
    fn vaxee_battery_rejects_bad_header_length_and_range() {
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 21)), None);
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0B], 100)), None);
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA5, 0x0C], 20)), None);
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0E, 0xA4, 0x0B], 20)), None);
        assert_eq!(parse_vaxee_battery(&vaxee_reply([0x0D, 0xA5, 0x0B], 20)), None);
        assert_eq!(parse_vaxee_battery(&[0u8; 64]), None);
        let full = vaxee_reply([0x0E, 0xA5, 0x0B], 20);
        assert_eq!(parse_vaxee_battery(&full[..63]), None);
    }

    #[test]
    fn vaxee_charging_page_reports_only_zero_or_one() {
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA5, 0x10], 1)), Some(true));
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA5, 0x10], 0)), Some(false));
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA5, 0x10], 2)), None);
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA5, 0x0B], 1)), None);
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA4, 0x10], 1)), None);
        assert_eq!(parse_vaxee_charging(&vaxee_reply([0x0E, 0xA5, 0x10], 1)[..63]), None);
    }

    #[test]
    fn logitech_lightspeed_receiver_ids_are_present() {
        assert_eq!(LOGITECH_RECEIVERS.len(), 23);
        for pid in [0xC545, 0xC547, 0xC54D] {
            assert!(LOGITECH_RECEIVERS.contains(&pid), "{pid:#06X} missing");
        }
    }

    fn logitech_voltage_payload(mv: u16, flags: u8) -> [u8; 3] {
        let [high, low] = mv.to_be_bytes();
        [high, low, flags]
    }

    #[test]
    fn logitech_voltage_external_power_is_not_charging_by_itself() {
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x80)),
            Some((80, Some(true)))
        );
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x82)),
            Some((80, Some(false)))
        );
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x83)),
            Some((80, None))
        );
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x00)),
            Some((80, Some(false)))
        );
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x0F)),
            Some((80, Some(false)))
        );
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(3990, 0x8F)),
            Some((80, None))
        );
        assert_eq!(parse_logitech_voltage(&logitech_voltage_payload(2499, 0x80)), None);
        assert_eq!(
            parse_logitech_voltage(&logitech_voltage_payload(2500, 0x80)),
            Some((0, Some(true)))
        );
    }

    #[test]
    fn logitech_status_accepts_zero_and_full() {
        assert_eq!(
            parse_logitech_battery_status(&[0, 0, 0]),
            Some((0, Some(false)))
        );
        assert_eq!(
            parse_logitech_battery_status(&[0, 0, 3]),
            Some((100, Some(true)))
        );
        assert_eq!(
            parse_logitech_battery_status(&[42, 0, 1]),
            Some((42, Some(true)))
        );
        assert_eq!(
            parse_logitech_battery_status(&[42, 0, 4]),
            Some((42, None))
        );
        assert_eq!(
            parse_logitech_battery_status(&[42, 0, 7]),
            Some((42, None))
        );
        assert_eq!(parse_logitech_battery_status(&[101, 0, 0]), None);
        assert_eq!(parse_logitech_battery_status(&[42, 0, 8]), None);
        assert_eq!(parse_logitech_battery_status(&[42, 0]), None);
    }

    #[test]
    fn logitech_unified_keeps_conflicting_status_unknown() {
        assert_eq!(
            parse_logitech_unified(&[42, 8, 4]),
            Some((42, None))
        );
        assert_eq!(
            parse_logitech_unified(&[42, 8, 5]),
            Some((42, None))
        );
        assert_eq!(
            parse_logitech_unified(&[42, 4, 3]),
            Some((42, Some(true)))
        );
        assert_eq!(parse_logitech_unified(&[0, 8, 3]), None);
        assert_eq!(parse_logitech_unified(&[101, 8, 0]), None);
        assert_eq!(parse_logitech_unified(&[42, 8, 8]), None);
    }

    #[test]
    fn logitech_adc_accepts_only_documented_states() {
        assert_eq!(
            parse_logitech_adc(&logitech_voltage_payload(3905, 0x01)),
            Some((67, Some(false)))
        );
        assert_eq!(
            parse_logitech_adc(&logitech_voltage_payload(3905, 0x03)),
            Some((67, Some(true)))
        );
        assert_eq!(
            parse_logitech_adc(&logitech_voltage_payload(3905, 0x07)),
            Some((67, Some(true)))
        );
        assert_eq!(parse_logitech_adc(&logitech_voltage_payload(3905, 0x0F)), None);
        assert_eq!(parse_logitech_adc(&logitech_voltage_payload(3905, 0x02)), None);
        assert_eq!(parse_logitech_adc(&logitech_voltage_payload(3905, 0x00)), None);
        assert_eq!(parse_logitech_adc(&logitech_voltage_payload(2499, 0x01)), None);
    }

    #[test]
    fn logitech_voltage_curve_keeps_the_documented_conflict() {
        assert_eq!(voltage_to_percent(3905), 67);
        assert_eq!(voltage_to_percent(4186), 100);
        assert_eq!(voltage_to_percent(3499), 0);
    }

    fn pixart_reply(marker: u8, charging: u8, level: u8) -> Vec<u8> {
        let mut raw = vec![0u8; 65];
        raw[..9].copy_from_slice(&[
            0x00, marker, 0x00, 0x02, 0x02, 0x00, 0x83, charging, level,
        ]);
        raw
    }

    fn with_product(mut info: HidInterface, product: &str) -> HidInterface {
        info.product = product.to_owned();
        info
    }

    #[test]
    fn pixart_valid_frames_use_raw_seven_and_eight() {
        assert_eq!(
            parse_pixart_reply(&pixart_reply(0xA1, 1, 75)),
            Some(PixArtReading {
                level: 75,
                charging: true,
            })
        );
        assert_eq!(
            parse_pixart_reply(&pixart_reply(0xA2, 0, 0)),
            Some(PixArtReading {
                level: 0,
                charging: false,
            })
        );
    }

    #[test]
    fn pixart_header_must_be_anchored_at_offset_one() {
        for marker in [0x00, 0x82, 0xA0, 0xA3, 0xFF] {
            assert_eq!(parse_pixart_reply(&pixart_reply(marker, 0, 50)), None);
        }
        let mut shifted = vec![0u8; 65];
        shifted[..10].copy_from_slice(&[
            0x00, 0x00, 0xA1, 0x00, 0x02, 0x02, 0x00, 0x83, 0x00, 0x50,
        ]);
        assert_eq!(parse_pixart_reply(&shifted), None);

        let mut bad_echo = pixart_reply(0xA1, 0, 50);
        bad_echo[6] = 0x82;
        assert_eq!(parse_pixart_reply(&bad_echo), None);
    }

    #[test]
    fn pixart_rejects_wrong_length_report_id_and_range() {
        assert_eq!(parse_pixart_reply(&pixart_reply(0xA1, 0, 50)[..64]), None);
        let mut long = pixart_reply(0xA1, 0, 50);
        long.push(0);
        assert_eq!(parse_pixart_reply(&long), None);
        assert_eq!(parse_pixart_reply(&[]), None);

        let mut report_id = pixart_reply(0xA1, 0, 50);
        report_id[0] = 0x01;
        assert_eq!(parse_pixart_reply(&report_id), None);

        assert_eq!(parse_pixart_reply(&pixart_reply(0xA1, 0, 101)), None);
        for charging in [2, 0x65, 0xFF] {
            assert_eq!(
                parse_pixart_reply(&pixart_reply(0xA1, charging, 50)),
                None,
                "{charging:#04X}"
            );
        }
    }

    #[test]
    fn pixart_all_zero_is_offline_but_headed_zero_is_valid() {
        assert_eq!(parse_pixart_reply(&vec![0u8; 65]), None);
        assert_eq!(
            parse_pixart_reply(&pixart_reply(0xA1, 0, 0)),
            Some(PixArtReading {
                level: 0,
                charging: false,
            })
        );
    }

    #[test]
    fn pixart_candidate_requires_exact_usage_and_length() {
        assert!(pixart_candidate(&hid_interface(
            0x36A7, 0xA880, 0xFFFF, 0, 65
        )));
        assert!(pixart_candidate(&hid_interface(
            0x2023, 0xF013, 0xFFFF, 1, 65
        )));
        assert!(!pixart_candidate(&hid_interface(
            0x2023, 0xF013, 0xFFFF, 2, 65
        )));
        assert!(!pixart_candidate(&hid_interface(
            0x36A7, 0xA880, 0xFF00, 0, 65
        )));
        assert!(!pixart_candidate(&hid_interface(
            0x36A7, 0xA880, 0xFFFF, 0, 64
        )));
        assert!(!pixart_candidate(&hid_interface(
            0x373E, 0x001E, 0xFFFF, 1, 65
        )));

        let mut lamzu = hid_interface(0x373E, 0x001E, 0xFFFF, 0, 65);
        assert!(!pixart_interface_ok(&lamzu));
        lamzu.interface_number = Some(1);
        assert!(!pixart_interface_ok(&lamzu));
        lamzu.interface_number = Some(2);
        assert!(pixart_interface_ok(&lamzu));
        assert!(pixart_candidate(&lamzu));
        let mut r5 = hid_interface(0x373E, 0x0047, 0xFFFF, 0, 65);
        r5.interface_number = Some(0);
        assert!(pixart_interface_ok(&r5));

        let request = pixart_request();
        assert_eq!(
            &request[..7],
            &[0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x83]
        );
        assert!(request[7..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn pixart_whitelist_rejects_unknown_pids_and_products() {
        assert_eq!(pixart_name(0x373E, 0x0001), None);
        assert_eq!(pixart_name(0x2023, 0xF014), None);
        assert_eq!(pixart_name(0x36A7, 0xA881), None);
        assert_eq!(pixart_name(0x248A, 0x5B2F), None);

        assert!(pixart_product_ok(&with_product(
            hid_interface(0x373E, 0x001E, 0xFFFF, 0, 65),
            "LAMZU Maya X"
        )));
        assert!(!pixart_product_ok(&with_product(
            hid_interface(0x373E, 0x001E, 0xFFFF, 0, 65),
            "Maya X"
        )));
        assert!(pixart_product_ok(&with_product(
            hid_interface(0x373E, 0x0046, 0xFFFF, 0, 65),
            "Attack Shark R5 Ultra"
        )));
        assert!(!pixart_product_ok(&with_product(
            hid_interface(0x373E, 0x0046, 0xFFFF, 0, 65),
            "R5 Ultra"
        )));
        // 36A7 is accepted by known PID even when the product string is absent.
        assert!(pixart_product_ok(&hid_interface(
            0x36A7, 0xA880, 0xFFFF, 0, 65
        )));
        assert!(pixart_product_ok(&with_product(
            hid_interface(0x2023, 0xF013, 0xFFFF, 0, 65),
            "MCHOSE A5 Pro Max"
        )));
        assert!(!pixart_product_ok(&with_product(
            hid_interface(0x2023, 0xF013, 0xFFFF, 0, 65),
            "A5 Pro Max"
        )));
    }

    const MCHOSE_OK_100_DISCHARGING: [u8; 21] = [
        0x11, 0xF9, 0xAC, 0xAD, 0xCE, 0xFF, 0xFD, 0xFA, 0xF8, 0xFF, 0xF6, 0x9B, 0xFF, 0x9B,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    const MCHOSE_OK_100_CHARGING: [u8; 21] = [
        0x11, 0xF9, 0xAC, 0xAD, 0xCE, 0xFF, 0xFD, 0xFA, 0xF8, 0xFF, 0xF6, 0x9B, 0xFE, 0x9B,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];

    fn mchose_long(level: u8, charging: u8) -> Vec<u8> {
        let mut raw = vec![0xFFu8; 65];
        raw[0] = 0x12;
        raw[1] = 0xF9;
        let payload = [
            0x53, 0x52, 0x31, 0x00, 0x02, 0x05, 0x07, 0x00, 0x09, level, charging,
        ];
        for (index, byte) in payload.iter().enumerate() {
            raw[2 + index] = *byte ^ 0xFF;
        }
        raw
    }

    #[test]
    fn mchose_short_frames_decode_identity_and_charge() {
        assert_eq!(
            parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING, 0x11, 0x5253),
            Some(MchoseReading {
                level: 100,
                charging: false,
                model: 0x0031,
                flags: 0x09,
                firmware: 0x0007_0502,
            })
        );
        assert_eq!(
            parse_mchose_reply(&MCHOSE_OK_100_CHARGING, 0x11, 0x5253),
            Some(MchoseReading {
                level: 100,
                charging: true,
                model: 0x0031,
                flags: 0x09,
                firmware: 0x0007_0502,
            })
        );
    }

    #[test]
    fn mchose_long_and_short_channels_stay_paired_to_report_id() {
        let raw = mchose_long(80, 1);
        assert_eq!(
            parse_mchose_reply(&raw, 0x12, 0x5253).map(|r| (r.level, r.charging)),
            Some((80, true))
        );
        assert_eq!(parse_mchose_reply(&raw, 0x11, 0x5253), None);
        assert_eq!(
            parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING, 0x12, 0x5253),
            None
        );
    }

    #[test]
    fn mchose_zero_level_with_identity_is_valid() {
        let mut raw = MCHOSE_OK_100_DISCHARGING;
        raw[11] = 0xFF;
        assert_eq!(
            parse_mchose_reply(&raw, 0x11, 0x5253).map(|r| (r.level, r.charging)),
            Some((0, false))
        );
    }

    #[test]
    fn mchose_sleep_and_all_zero_frames_are_rejected() {
        let mut sleep = [0x11u8; 21];
        sleep[1] = 0xF9;
        for byte in &mut sleep[2..] {
            *byte = 0xFF;
        }
        assert_eq!(parse_mchose_reply(&sleep, 0x11, 0x5253), None);
        assert_eq!(parse_mchose_reply(&[0u8; 21], 0x11, 0x5253), None);
    }

    #[test]
    fn mchose_rejects_bad_header_length_vid_and_range() {
        let mut wrong_cmd = MCHOSE_OK_100_DISCHARGING;
        wrong_cmd[1] = 0xF8;
        assert_eq!(parse_mchose_reply(&wrong_cmd, 0x11, 0x5253), None);

        let mut wrong_report = MCHOSE_OK_100_DISCHARGING;
        wrong_report[0] = 0x12;
        assert_eq!(parse_mchose_reply(&wrong_report, 0x11, 0x5253), None);
        assert_eq!(
            parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING[..20], 0x11, 0x5253),
            None
        );

        let mut out_of_range = MCHOSE_OK_100_DISCHARGING;
        out_of_range[11] = 0x9A;
        assert_eq!(parse_mchose_reply(&out_of_range, 0x11, 0x5253), None);

        let mut bad_charge = MCHOSE_OK_100_DISCHARGING;
        bad_charge[12] = 0xFD;
        assert_eq!(parse_mchose_reply(&bad_charge, 0x11, 0x5253), None);

        assert_eq!(parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING, 0x11, 0x3837), None);
    }

    #[test]
    fn mchose_candidate_uses_pid_whitelist_not_vid_family() {
        assert!(mchose_candidate(&hid_interface(0x5253, 0x1020, 0xFF01, 1, 65)));
        assert!(mchose_candidate(&hid_interface(0x3837, 0x100B, 0xFF01, 1, 65)));
        assert!(!mchose_candidate(&hid_interface(0x3837, 0x600A, 0xFF01, 1, 65)));
        assert!(!mchose_candidate(&hid_interface(0x5253, 0x9999, 0xFF01, 1, 65)));
        assert!(!mchose_candidate(&hid_interface(0x2023, 0xF013, 0xFF01, 1, 65)));
        assert!(!mchose_candidate(&hid_interface(0x5253, 0x1020, 0xFF0B, 0x104, 65)));
        assert!(!mchose_candidate(&hid_interface(0x5253, 0x1020, 0xFF01, 0, 65)));

        let request = mchose_request(0x11, 21);
        assert_eq!(request.len(), 21);
        assert_eq!(&request[..2], &[0x11, 0xF9]);
        assert!(request[2..].iter().all(|byte| *byte == 0xFF));
        let long = mchose_request(0x12, 65);
        assert_eq!(long.len(), 65);
        assert_eq!(&long[..2], &[0x12, 0xF9]);
        assert!(long[2..].iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn mchose_does_not_fall_back_to_comp_x() {
        assert!(!is_compX_vid(0x5253));
        assert_eq!(compX_name(0x5253, 0x1020), None);
        assert_eq!(compX_name(0x5253, 0x0031), None);
    }

    fn delux_reply(level: u8, charging: u8) -> Vec<u8> {
        let mut raw = vec![0u8; 33];
        raw[0] = 0x0C;
        raw[1] = 0x01;
        raw[2] = 0x20;
        raw[4] = 0x01;
        raw[18] = level;
        raw[19] = charging;
        raw
    }

    #[test]
    fn delux_valid_frames_use_raw_eighteen_and_nineteen() {
        assert_eq!(
            parse_delux_reply(&delux_reply(80, 0)),
            Some(DeluxReading {
                level: 80,
                charging: false,
            })
        );
        assert_eq!(
            parse_delux_reply(&delux_reply(80, 1)),
            Some(DeluxReading {
                level: 80,
                charging: true,
            })
        );
        assert_eq!(
            parse_delux_reply(&delux_reply(100, 0)),
            Some(DeluxReading {
                level: 100,
                charging: false,
            })
        );
    }

    #[test]
    fn delux_rejects_wrong_report_id_and_length() {
        let mut wrong = delux_reply(80, 0);
        wrong[0] = 0x0B;
        assert_eq!(parse_delux_reply(&wrong), None);
        assert_eq!(parse_delux_reply(&delux_reply(80, 0)[..32]), None);
        let mut long = delux_reply(80, 0);
        long.push(0);
        assert_eq!(parse_delux_reply(&long), None);
        assert_eq!(parse_delux_reply(&[]), None);
    }

    #[test]
    fn delux_offline_out_of_range_and_zero_65_are_rejected() {
        assert_eq!(parse_delux_reply(&delux_reply(0, 0)), None);
        assert_eq!(parse_delux_reply(&delux_reply(101, 0)), None);
        assert_eq!(parse_delux_reply(&delux_reply(80, 2)), None);
        assert_eq!(parse_delux_reply(&delux_reply(80, 0x65)), None);
    }

    #[test]
    fn delux_candidate_requires_interface_one_and_33_bytes() {
        let mut info = hid_interface(0x248A, 0x5B2F, 0xFF00, 0x0001, 33);
        info.interface_number = Some(1);
        assert!(delux_candidate(&info));
        assert!(!pixart_candidate(&info));

        info.interface_number = Some(0);
        assert!(!delux_candidate(&info));
        info.interface_number = None;
        assert!(!delux_candidate(&info));
        info.interface_number = Some(1);
        info.feature_len = 32;
        assert!(!delux_candidate(&info));
        info.feature_len = 33;
        info.vendor_id = 0x258A;
        assert!(!delux_candidate(&info));
        info.vendor_id = 0x248A;
        info.product_id = 0x5B30;
        assert!(!delux_candidate(&info));

        let request = delux_request(0);
        assert_eq!(&request[..5], &[0x0C, 0x01, 0x20, 0x00, 0x01]);
        assert_eq!(delux_request(7)[4], 8);
    }

    fn oem_interface(
        path: &str,
        interface_number: u16,
        container: &str,
        product: &str,
        usage: u16,
    ) -> HidInterface {
        let mut info = hid_interface(0x2023, 0xF013, 0xFFFF, usage, 65);
        info.path = path.to_owned();
        info.interface_number = Some(interface_number);
        info.container_id = Some(container.to_owned());
        info.product = product.to_owned();
        info
    }

    #[test]
    fn a5_selects_usage_zero_even_when_usage_one_is_enumerated_first() {
        let usage_one = oem_interface("p1", 1, "c1", "MCHOSE A5 2.4G", 1);
        let usage_zero = oem_interface("p2", 2, "c1", "MCHOSE A5 2.4G", 0);

        let forward = select_oem_paths(&[usage_one.clone(), usage_zero.clone()]);
        let reverse = select_oem_paths(&[usage_zero.clone(), usage_one.clone()]);

        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert!(forward.contains("p2"));
        assert!(!forward.contains("p1"));
    }

    #[test]
    fn a5_usage_one_is_only_a_fallback_for_the_same_container() {
        let usage_one = oem_interface("p1", 1, "c1", "MCHOSE A5 2.4G", 1);
        let selected = select_oem_paths(&[usage_one]);
        assert_eq!(selected.len(), 1);
        assert!(selected.contains("p1"));

        let mut wrong_family = hid_interface(0x36A7, 0xA880, 0xFFFF, 1, 65);
        wrong_family.path = "wl".to_owned();
        wrong_family.container_id = Some("c2".to_owned());
        assert!(select_oem_paths(&[wrong_family]).is_empty());
    }

    #[test]
    fn mchose_and_delux_select_one_path_per_container() {
        let mut m2 = hid_interface(0x5253, 0x1020, 0xFF01, 1, 65);
        m2.path = "m2".to_owned();
        m2.interface_number = Some(2);
        m2.container_id = Some("m".to_owned());
        let mut m3 = m2.clone();
        m3.path = "m3".to_owned();
        m3.interface_number = Some(3);
        let selected = select_oem_paths(&[m2.clone(), m3.clone()]);
        assert_eq!(selected.len(), 1);
        assert!(selected.contains("m2"));

        let mut d2 = hid_interface(0x248A, 0x5B2F, 0xFF00, 1, 33);
        d2.path = "d2".to_owned();
        d2.interface_number = Some(1);
        d2.container_id = Some("d".to_owned());
        let mut d3 = d2.clone();
        d3.path = "d3".to_owned();
        let selected = select_oem_paths(&[d2, d3]);
        assert_eq!(selected.len(), 1);
        assert!(selected.contains("d2"));
    }

    #[test]
    fn mchose_headset_pid_is_never_selected() {
        let mut headset = hid_interface(0x3837, 0x600A, 0xFF01, 1, 65);
        headset.path = "headset".to_owned();
        headset.container_id = Some("h".to_owned());
        assert!(select_oem_paths(&[headset]).is_empty());
    }
}
