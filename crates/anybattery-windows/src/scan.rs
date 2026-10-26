use anybattery_core::{
    now_unix, BatteryReading, BatterySource, BatteryStatus, Confidence, DeviceId, DeviceKind,
    DeviceSnapshot, Settings,
};
use std::collections::HashMap;

use crate::{bluetooth, hid, vendor, xbox};

#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub devices: Vec<DeviceSnapshot>,
    pub diagnostics: Vec<String>,
    pub bluetooth_count: usize,
    pub hid_count: usize,
    pub controller_count: usize,
}

pub fn scan(settings: &Settings) -> ScanResult {
    let now = now_unix();
    let mut diagnostics = Vec::new();
    let mut map: HashMap<String, DeviceSnapshot> = HashMap::new();

    let bluetooth_devices = bluetooth::merge_bluetooth(bluetooth::enumerate());
    diagnostics.push(format!("蓝牙节点: {}", bluetooth_devices.len()));
    for device in &bluetooth_devices {
        let snapshot = bluetooth::snapshot(device, now);
        let key = device
            .container_id
            .as_ref()
            .map(|id| format!("container:{}", id))
            .unwrap_or_else(|| snapshot.id.0.clone());
        diagnostics.push(format!(
            "  蓝牙 {} · {} · {} · 电量 {:?}",
            snapshot.name, snapshot.status.label_zh(), device.instance_id, device.battery
        ));
        insert(&mut map, key, snapshot);
    }

    let interfaces = hid::enumerate();
    diagnostics.push(format!("HID 接口: {}", interfaces.len()));
    for info in &interfaces {
        diagnostics.push(format!(
            "  HID {:04X}:{:04X} {} · usage {:04X}:{:04X} · in {} out {} feature {} · {}",
            info.vendor_id,
            info.product_id,
            info.product,
            info.usage_page,
            info.usage,
            info.input_len,
            info.output_len,
            info.feature_len,
            info.path
        ));
        let key = info
            .container_id
            .as_ref()
            .map(|id| format!("container:{}", id))
            .unwrap_or_else(|| format!("hid:{:04X}:{:04X}:{}", info.vendor_id, info.product_id, hid::short_hash(&info.path)));

        let readings = vendor::read_vendor(info);
        if !readings.is_empty() {
            for reading in &readings {
                let device_key = match reading.key_suffix.as_ref() {
                    Some(suffix) => format!("{}:{}", key, suffix),
                    None => key.clone(),
                };
                let snapshot = snapshot_from_vendor(info, reading, now);
                diagnostics.push(format!(
                    "    -> {} {}% ({}) {}",
                    reading.provider,
                    reading.level,
                    reading.verification.label_zh(),
                    reading.name.as_deref().unwrap_or("")
                ));
                insert(&mut map, device_key, snapshot);
            }
            continue;
        }

        if let Some(reading) = hid::read_standard_battery(info) {
            let snapshot = hid::standard_snapshot(info, &reading, now);
            diagnostics.push(format!(
                "    -> 标准 HID {}% (usage {:04X}:{:04X})",
                reading.level, reading.usage_page, reading.usage
            ));
            insert(&mut map, key, snapshot);
            continue;
        }

        if !settings.show_unknown {
            continue;
        }
        if likely_wireless(info) {
            let snapshot = unsupported_snapshot(info, now);
            diagnostics.push(format!("    -> 未识别（保留在列表）"));
            insert(&mut map, key, snapshot);
        }
    }

    let controllers = xbox::enumerate(now);
    diagnostics.push(format!("手柄: {}", controllers.len()));
    for controller in controllers {
        let key = controller.id.0.clone();
        diagnostics.push(format!(
            "  手柄 {} · {} · {:?}",
            controller.name,
            controller.status.label_zh(),
            controller.percent()
        ));
        insert(&mut map, key, controller);
    }

    let mut devices: Vec<DeviceSnapshot> = map
        .into_values()
        .filter(|device| settings.show_unknown || device.battery.is_known())
        .collect();
    devices.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    ScanResult {
        devices,
        diagnostics,
        bluetooth_count: bluetooth_devices.len(),
        hid_count: interfaces.len(),
        controller_count: 0,
    }
}

fn snapshot_from_vendor(info: &hid::HidInterface, reading: &vendor::VendorReading, now: u64) -> DeviceSnapshot {
    let name = reading
        .name
        .clone()
        .or_else(|| (!info.product.is_empty()).then(|| info.product.clone()))
        .unwrap_or_else(|| info.label());
    let mut id = info
        .container_id
        .clone()
        .unwrap_or_else(|| format!("{:04X}:{:04X}:{}", info.vendor_id, info.product_id, hid::short_hash(&info.path)));
    if let Some(suffix) = &reading.key_suffix {
        id.push(':');
        id.push_str(suffix);
    }
    DeviceSnapshot::new(
        DeviceId::new("hid", &id),
        name.clone(),
        reading.kind.unwrap_or_else(|| DeviceKind::infer_from_name(&name)),
        hid::transport_for(&info.path, info.vendor_id),
        BatterySource::new(reading.provider, reading.verification, reading.detail.clone()),
    )
    .with_battery(BatteryReading::percent(reading.level, reading.charging), now)
    .with_detail(reading.detail.clone())
    .with_path(&info.path)
}

fn unsupported_snapshot(info: &hid::HidInterface, now: u64) -> DeviceSnapshot {
    let id = info
        .container_id
        .clone()
        .unwrap_or_else(|| format!("{:04X}:{:04X}:{}", info.vendor_id, info.product_id, hid::short_hash(&info.path)));
    let mut snapshot = DeviceSnapshot::new(
        DeviceId::new("hid", &id),
        info.label(),
        DeviceKind::infer_from_name(&info.product),
        hid::transport_for(&info.path, info.vendor_id),
        BatterySource::new("HID 接口", Confidence::Unverified, "尚未匹配电量协议"),
    )
    .with_detail("已连接 · 未找到电量协议")
    .with_path(&info.path);
    snapshot.battery = BatteryReading::Unknown;
    snapshot.status = BatteryStatus::Unsupported;
    snapshot.last_seen_unix = Some(now);
    snapshot
}

fn likely_wireless(info: &hid::HidInterface) -> bool {
    let name = info.product.to_lowercase();
    let words = [
        "wireless", "hyperspeed", "lightspeed", "receiver", "dongle", "2.4g", "air", "pro",
    ];
    words.iter().any(|word| name.contains(word))
        || matches!(
            info.vendor_id,
            0x046D | 0x1532 | 0x3554 | 0x373B | 0x3710 | 0x3770 | 0x3057 | 0x3367 | 0x1915
                | 0x3654 | 0x1038 | 0x03F0 | 0x1B1C | 0x0B05 | 0x3434 | 0x388D | 0x1E71
                | 0x5253 | 0x3329 | 0x0ECB | 0x28DE
        )
}

fn insert(map: &mut HashMap<String, DeviceSnapshot>, key: String, snapshot: DeviceSnapshot) {
    match map.get_mut(&key) {
        Some(existing) => {
            if score(&snapshot) > score(existing) {
                let name = if snapshot.name.len() >= existing.name.len() {
                    snapshot.name.clone()
                } else {
                    existing.name.clone()
                };
                *existing = snapshot;
                existing.name = name;
            } else {
                if snapshot.name.len() > existing.name.len() {
                    existing.name = snapshot.name.clone();
                }
                if existing.detail.is_empty() {
                    existing.detail = snapshot.detail;
                }
                if existing.path.is_none() {
                    existing.path = snapshot.path;
                }
            }
        }
        None => {
            map.insert(key, snapshot);
        }
    }
}

fn score(snapshot: &DeviceSnapshot) -> i32 {
    let base = match snapshot.battery {
        BatteryReading::Percent { .. } => match snapshot.source.verification {
            Confidence::Verified => 100,
            Confidence::Community => 92,
            Confidence::Unverified => 84,
            Confidence::OsReported => 78,
            Confidence::Coarse => 60,
            Confidence::Cached => 10,
        },
        BatteryReading::Coarse { .. } => 50,
        BatteryReading::Unknown => 20,
    };
    if snapshot.status.is_live() { base + 5 } else { base }
}
