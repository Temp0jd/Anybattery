use anybattery_core::Confidence;
use std::time::Duration;

use super::hid::{self, HidInterface};

#[derive(Clone, Debug)]
pub struct VendorReading {
    pub level: u8,
    pub charging: Option<bool>,
    pub provider: &'static str,
    pub verification: Confidence,
    pub detail: String,
}

pub fn read_vendor(info: &HidInterface) -> Option<VendorReading> {
    if !info.is_vendor_collection() && info.feature_len < 90 {
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
        if let Some(reading) = read_vaXee(info) {
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
    matches!(vid, 0x3554 | 0x373B | 0x3770 | 0x3710 | 0x5253)
}

// ---------------------------------------------------------------------------
// Razer: 90-byte feature report, command class 0x07.
// ---------------------------------------------------------------------------

fn razer_name(pid: u16) -> Option<(&'static str, u8, Confidence)> {
    let verified = Confidence::Verified;
    let community = Confidence::Community;
    Some(match pid {
        0x0528 => ("Razer BlackShark V2 Pro (2020)", 0x3F, verified),
        0x0555 => ("Razer BlackShark V2 Pro (2023)", 0x3F, verified),
        0x0556 => ("Razer BlackShark V2 Pro (2023)", 0x3F, verified),
        0x0557 => ("Razer BlackShark V2 Pro (2023)", 0x3F, verified),
        0x053A => ("Razer Barracuda Pro (2.4 GHz)", 0x3F, community),
        0x0290 => ("Razer DeathStalker V2 Pro", 0x9F, community),
        0x0292 => ("Razer DeathStalker V2 Pro", 0x1F, community),
        0x0296 => ("Razer DeathStalker V2 Pro TKL", 0x9F, verified),
        0x0298 => ("Razer DeathStalker V2 Pro TKL", 0x1F, verified),
        0x0271 => ("Razer BlackWidow V3 Mini HyperSpeed", 0x9F, community),
        0x0258 => ("Razer BlackWidow V3 Mini HyperSpeed", 0x1F, community),
        0x02BA => ("Razer BlackWidow V4 Mini HyperSpeed", 0x9F, community),
        0x02B9 => ("Razer BlackWidow V4 Mini HyperSpeed", 0x1F, community),
        0x02D5 => ("Razer BlackWidow V4 Tenkeyless HyperSpeed", 0x9F, verified),
        0x02D7 => ("Razer BlackWidow V4 Tenkeyless HyperSpeed", 0x1F, verified),
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
        0x00BE => ("Razer DeathAdder V4 Pro", 0x1F, verified),
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
        (0x5253, 0x1020) => ("MCHOSE M7 Ultra (2.4G)", community),
        (0x5253, 0x0031) => ("MCHOSE M7 Ultra (USB)", community),
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

fn read_vaXee(info: &HidInterface) -> Option<VendorReading> {
    let mut request = [0u8; 64];
    request[..5].copy_from_slice(&[0x0E, 0xA5, 0x0B, 0x01, 0x01]);
    let reply = feature_roundtrip(info, &request, 64)?;
    let level = *reply.get(5)?;
    if level > 100 {
        return None;
    }
    Some(VendorReading {
        level,
        charging: None,
        provider: "VAXEE/ZYGEN",
        verification: Confidence::Community,
        detail: format!("feature 0x0E · {}%", level),
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
    })
}
