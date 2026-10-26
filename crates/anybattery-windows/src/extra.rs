
use anybattery_core::{Confidence, DeviceKind};
use std::time::Duration;

use super::hid;
use super::vendor::VendorReading;

pub fn read_vendor_extra(info: &hid::HidInterface) -> Vec<VendorReading> {
    if let Some(reading) = read_controller(info) {
        return vec![reading];
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
    let parsed = hid::listen_idle(&info.path, 64, 3000, 450, |report| {
        if report.len() < 15 || report[0] != 0x43 {
            return None;
        }
        let charge = match report[1] {
            1 => Some(false),
            2 | 4 => Some(true),
            _ => None,
        };
        let level = report[2];
        if level <= 100 {
            Some((level, charge))
        } else {
            None
        }
    })?;
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
        if report.len() < 20 {
            return None;
        }
        let (rid, offset) = if dualsense {
            match report[0] {
                0x01 => (0x01, 53usize),
                0x31 => (0x31, 54usize),
                _ => return None,
            }
        } else {
            match report[0] {
                0x01 => (0x01, 30usize),
                0x11 => (0x11, 32usize),
                _ => return None,
            }
        };
        if report[0] != rid || report.len() <= offset {
            return None;
        }
        let byte = report[offset];
        if dualsense {
            let level = (byte & 0x0F).min(10) * 10;
            let charge = (byte >> 4) & 0x0F;
            Some((level, Some(charge == 1 || charge == 2)))
        } else {
            let level = (byte & 0x0F).min(10) * 10;
            let charging = byte & 0x10 != 0;
            Some((level, Some(charging)))
        }
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

fn read_eightbitdo(info: &hid::HidInterface) -> Option<VendorReading> {
    let name = match info.product_id {
        0x6000 | 0x6100 => "8BitDo SF30 Pro",
        0x6001 | 0x6101 => "8BitDo SN30 Pro",
        0x6003 | 0x6006 => "8BitDo Pro 2",
        0x6009 => "8BitDo Pro 3",
        _ => return None,
    };
    let bluetooth = hid::is_bluetooth_path(&info.path);
    let report_id = if info.product_id == 0x6009 {
        0x04
    } else if bluetooth {
        0x01
    } else {
        0x04
    };
    let parsed = hid::listen(&info.path, 64, 900, |report| {
        if report.len() <= 14 || report[0] != report_id {
            return None;
        }
        let byte = report[14];
        let level = byte & 0x7F;
        if !(1..=100).contains(&level) {
            return None;
        }
        Some((level, Some(byte & 0x80 != 0)))
    })?;
    Some(reading(
        parsed.0,
        parsed.1,
        "8BitDo 输入报告",
        Confidence::Verified,
        Some(name.to_owned()),
        Some(DeviceKind::Controller),
        format!("8bitdo:{:04X}", info.product_id),
        "listen only".to_owned(),
    ))
}

fn read_razer_pa(info: &hid::HidInterface) -> Option<VendorReading> {
    let (name, report_id, reply_offset, marker_offset, usage_ok) = match info.product_id {
        0x053A => ("Razer Barracuda Pro", 0x01u8, 13usize, 3usize, true),
        0x0555 | 0x0556 => ("Razer BlackShark V2 Pro (2023)", 0x02u8, 12usize, 5usize, true),
        _ => return None,
    };
    if !usage_ok || !info.is_vendor_collection() {
        return None;
    }
    if info.output_len < 64 && info.feature_len < 64 {
        return None;
    }
    let level = hid::with_handle(&info.path, true, |handle| {
        let mut remote = vec![0u8; 64];
        remote[0] = report_id;
        remote[1] = 0x80;
        remote[2] = 0x07;
        remote[marker_offset] = 0x50;
        remote[marker_offset + 1] = 0x41;
        remote[marker_offset + 2] = 0x0E;
        remote[marker_offset + 4] = 0x02;
        remote[marker_offset + 5] = 0xE1;
        remote[marker_offset + 6] = 1;
        if !hid::write_report(handle, &remote) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(35));

        let mut query = vec![0u8; 64];
        query[0] = report_id;
        query[1] = 0x80;
        query[2] = 8;
        query[marker_offset] = 0x50;
        query[marker_offset + 1] = 0x41;
        query[marker_offset + 2] = 0x08;
        query[marker_offset + 4] = 0x03;
        query[marker_offset + 5] = 0x21;
        if !hid::write_report(handle, &query) {
            return None;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(900);
        while std::time::Instant::now() < deadline {
            let Some(reply) = hid::read_report(handle, 64, 120) else {
                continue;
            };
            let offset = if reply.first() == Some(&report_id) {
                reply_offset
            } else {
                reply_offset.saturating_sub(1)
            };
            if reply.len() > offset + 3
                && reply[offset] == 0x21
                && (reply[offset + 1] == 0x01 || reply[offset + 1] == 0x02)
            {
                let length = reply[offset + 2] as usize;
                if length > 0 && reply.len() > offset + 3 {
                    let level = reply[offset + 3];
                    if level <= 100 {
                        return Some(level);
                    }
                }
            }
        }
        None
    })?;
    Some(reading(
        level,
        None,
        "Razer PA 耳机",
        Confidence::Community,
        Some(name.to_owned()),
        Some(DeviceKind::Headset),
        format!("razer-pa:{:04X}", info.product_id),
        "PA / 0x21".to_owned(),
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

fn read_keychron(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0xD028 && info.product_id != 0xD048 {
        return None;
    }
    let mut request = vec![0u8; 64];
    request[0] = 0xB3;
    request[1] = 0x06;
    if !hid::feature_set(&info.path, &request) {
        return None;
    }
    let reply = hid::input_read(&info.path, 64, 500)?;
    if reply.len() <= 20 || reply[0] != 0xB4 || reply[1] != 0x06 {
        return None;
    }
    let level = reply[20];
    if level > 100 {
        return None;
    }
    Some(reading(
        level,
        None,
        "Keychron 0xB3/0xB4",
        Confidence::Community,
        Some(if info.product.is_empty() { "Keychron Ultra-Link".to_owned() } else { info.product.clone() }),
        Some(DeviceKind::Keyboard),
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

fn read_steelseries(info: &hid::HidInterface) -> Option<VendorReading> {
    let mut request = vec![0u8; 64];
    request[1] = 0xB0;
    let reply = hid::write_and_read(&info.path, &request, 64, 600)?;
    if reply.len() >= 4 && reply[0] == 0xB0 {
        let link = reply[1];
        let level_byte = reply[2];
        let status = reply[3];
        if link == 0x03 && status != 0x00 {
            let level = if level_byte <= 4 { level_byte * 25 } else { level_byte.min(100) };
            return Some(reading(
                level,
                Some(matches!(status, 0x01 | 0x02)),
                "SteelSeries B0",
                Confidence::Community,
                Some(if info.product.is_empty() { "SteelSeries 无线耳机".to_owned() } else { info.product.clone() }),
                Some(DeviceKind::Headset),
                format!("steelseries:{:04X}", info.product_id),
                "Nova 7 reply".to_owned(),
            ));
        }
        if link == 0x02 && reply.len() >= 5 && level_byte != 0x02 {
            let level = reply[3].min(100);
            return Some(reading(
                level,
                Some(reply[4] == 0x01),
                "SteelSeries B0",
                Confidence::Community,
                Some(if info.product.is_empty() { "SteelSeries 无线耳机".to_owned() } else { info.product.clone() }),
                Some(DeviceKind::Headset),
                format!("steelseries:{:04X}", info.product_id),
                "Nova 5 reply".to_owned(),
            ));
        }
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

fn read_corsair(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.product_id != 0x1B7F && info.product_id != 0x0A40 && info.product_id != 0x0A3E && info.product_id != 0x0A64 {
        return None;
    }
    let mut request = vec![0u8; 65];
    request[0] = 0x00;
    request[1] = 0x02;
    request[2] = 0x09;
    request[3] = 0x02;
    request[4] = 0x0F;
    let reply = hid::write_and_read(&info.path, &request, 64, 500)?;
    if reply.len() < 6 || reply[1] != 0x02 || reply[2] != 0x00 {
        return None;
    }
    let raw = reply[4] as u16 | ((reply[5] as u16) << 8);
    if raw > 1000 {
        return None;
    }
    let level = (raw / 10).min(100) as u8;
    let name = match info.product_id {
        0x1B7F => "Corsair Dark Core RGB Pro SE",
        0x0A40 => "Corsair Virtuoso RGB Wireless SE",
        0x0A3E => "Corsair Virtuoso SE",
        0x0A64 => "Corsair Virtuoso XT",
        _ => "Corsair 无线设备",
    };
    Some(reading(
        level,
        None,
        "Corsair bragi",
        Confidence::Community,
        Some(name.to_owned()),
        Some(DeviceKind::Headset),
        format!("corsair:{:04X}", info.product_id),
        "battery 0x0F".to_owned(),
    ))
}
