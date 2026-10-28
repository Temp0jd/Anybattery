use anybattery_core::{
    BatteryReading, BatterySource, BatteryStatus, Confidence, DeviceId, DeviceKind, DeviceSnapshot,
    Settings, UserProfileCatalog, now_unix,
};
use std::collections::HashMap;
use std::time::Duration;

use crate::passive::ReadOnlyHid;
use crate::{apple, bluetooth, hid, user_profile, vendor, xbox};

#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub devices: Vec<DeviceSnapshot>,
    pub diagnostics: Vec<String>,
    pub bluetooth_count: usize,
    pub hid_count: usize,
    /// 本次枚举到的 HID 接口，交给上层做用户配置绑定，避免二次枚举。
    pub hid_interfaces: Vec<hid::HidInterface>,
}

#[derive(Clone, Debug)]
pub enum ScanEvent {
    Progress,
    Partial(Vec<DeviceSnapshot>),
}

pub fn scan(settings: &Settings) -> ScanResult {
    scan_with_progress(settings, &mut |_| {})
}

/// Full scan with coarse progress. Progress is emitted before each
/// potentially blocking read and Partial after each provider stage, so the
/// UI can show Bluetooth/PnP results before a slow vendor adapter finishes.
pub fn scan_with_progress(settings: &Settings, on_event: &mut dyn FnMut(ScanEvent)) -> ScanResult {
    scan_with_profiles_and_progress(settings, &UserProfileCatalog::default(), on_event)
}

/// Full scan that also resolves user-supplied passive profiles.
///
/// 内置厂商协议与标准 HID 电量永远优先：两者任一命中时，用户 profile
/// 分支不会打开句柄、不会读取任何报文。
pub fn scan_with_profiles_and_progress(
    settings: &Settings,
    profiles: &UserProfileCatalog,
    on_event: &mut dyn FnMut(ScanEvent),
) -> ScanResult {
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
            snapshot.name,
            snapshot.status.label_zh(),
            device.instance_id,
            device.battery
        ));
        insert(&mut map, key, snapshot);
    }

    let apple_devices = apple::snapshots(now, &bluetooth_devices);
    diagnostics.push(format!("Apple Continuity 电量卡: {}", apple_devices.len()));
    for snapshot in apple_devices {
        diagnostics.push(format!(
            "  Apple {} · {} · {:?}",
            snapshot.name,
            snapshot.status.label_zh(),
            snapshot.battery.level_percent()
        ));
        insert(&mut map, snapshot.id.0.clone(), snapshot);
    }
    on_event(ScanEvent::Progress);
    emit_partial(&map, settings, on_event);

    let interfaces = hid::enumerate();
    let oem_paths = vendor::select_oem_paths(&interfaces);
    let has_user_profiles = profiles.enabled_profiles().next().is_some();
    diagnostics.push(format!("HID 接口: {}", interfaces.len()));
    diagnostics.push(format!(
        "用户配置 profile: {} 启用 / {} 总数",
        profiles.enabled_profiles().count(),
        profiles.profiles().len()
    ));
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
            .unwrap_or_else(|| {
                format!(
                    "hid:{:04X}:{:04X}:{}",
                    info.vendor_id,
                    info.product_id,
                    hid::short_hash(&info.path)
                )
            });

        // Report progress before the blocking read: the watchdog measures the
        // gap between progress events, not a total scan deadline.
        on_event(ScanEvent::Progress);
        let readings = vendor::read_vendor_with_diagnostics(info, &oem_paths, &mut diagnostics);
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
        } else {
            let standard = hid::read_standard_battery(info);
            let standard_hit = standard.is_some();
            let mut user_hit = false;
            if let Some(reading) = standard {
                let snapshot = hid::standard_snapshot(info, &reading, now);
                diagnostics.push(format!(
                    "    -> 标准 HID {}% (usage {:04X}:{:04X})",
                    reading.level, reading.usage_page, reading.usage
                ));
                insert(&mut map, key.clone(), snapshot);
            }
            // 用户配置只补位：内置厂商/标准 HID 命中时零 I/O，且绝不覆盖已知结果。
            if should_read_user_profile(false, standard_hit, has_user_profiles) {
                if let Some(snapshot) = read_user_snapshot(info, profiles, now, &mut diagnostics) {
                    user_hit = true;
                    insert_user(&mut map, key.clone(), snapshot);
                }
            }
            if !standard_hit && !user_hit && settings.show_unknown && likely_wireless(info) {
                let snapshot = unsupported_snapshot(info, now);
                diagnostics.push("    -> 未识别（保留在列表）".to_owned());
                insert(&mut map, key, snapshot);
            }
        }

        on_event(ScanEvent::Progress);
        emit_partial(&map, settings, on_event);
    }

    on_event(ScanEvent::Progress);
    let controllers = xbox::enumerate(now);
    diagnostics.push(format!("手柄: {}", controllers.len()));
    for controller in &controllers {
        let key = controller.id.0.clone();
        diagnostics.push(format!(
            "  手柄 {} · {} · {:?}",
            controller.name,
            controller.status.label_zh(),
            controller.percent()
        ));
        insert(&mut map, key, controller.clone());
    }
    on_event(ScanEvent::Progress);
    emit_partial(&map, settings, on_event);

    let mut devices: Vec<DeviceSnapshot> = map
        .into_values()
        .filter(|device| settings.show_unknown || device.battery.is_known())
        .collect();
    devices.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let hid_count = interfaces.len();
    ScanResult {
        devices,
        diagnostics,
        bluetooth_count: bluetooth_devices.len(),
        hid_count,
        hid_interfaces: interfaces,
    }
}

fn emit_partial(
    map: &HashMap<String, DeviceSnapshot>,
    settings: &Settings,
    on_event: &mut dyn FnMut(ScanEvent),
) {
    let mut devices: Vec<DeviceSnapshot> = map
        .values()
        .filter(|device| settings.show_unknown || device.battery.is_known())
        .cloned()
        .collect();
    devices.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    on_event(ScanEvent::Partial(devices));
}

fn snapshot_from_vendor(
    info: &hid::HidInterface,
    reading: &vendor::VendorReading,
    now: u64,
) -> DeviceSnapshot {
    let name = reading
        .name
        .clone()
        .or_else(|| (!info.product.is_empty()).then(|| info.product.clone()))
        .unwrap_or_else(|| info.label());
    let mut id = info.container_id.clone().unwrap_or_else(|| {
        format!(
            "{:04X}:{:04X}:{}",
            info.vendor_id,
            info.product_id,
            hid::short_hash(&info.path)
        )
    });
    if let Some(suffix) = &reading.key_suffix {
        id.push(':');
        id.push_str(suffix);
    }
    DeviceSnapshot::new(
        DeviceId::new("hid", &id),
        name.clone(),
        reading
            .kind
            .unwrap_or_else(|| DeviceKind::infer_from_name(&name)),
        hid::transport_for_product(&info.path, info.vendor_id, info.product_id),
        BatterySource::new(
            reading.provider,
            reading.verification,
            reading.detail.clone(),
        ),
    )
    .with_battery(
        BatteryReading::percent(reading.level, reading.charging),
        now,
    )
    .with_detail(reading.detail.clone())
    .with_path(&info.path)
}

fn unsupported_snapshot(info: &hid::HidInterface, now: u64) -> DeviceSnapshot {
    let id = info.container_id.clone().unwrap_or_else(|| {
        format!(
            "{:04X}:{:04X}:{}",
            info.vendor_id,
            info.product_id,
            hid::short_hash(&info.path)
        )
    });
    let mut snapshot = DeviceSnapshot::new(
        DeviceId::new("hid", &id),
        info.label(),
        DeviceKind::infer_from_name(&info.product),
        hid::transport_for_product(&info.path, info.vendor_id, info.product_id),
        BatterySource::new("HID 接口", Confidence::Unverified, "尚未匹配电量协议"),
    )
    .with_detail("已连接 · 未找到电量协议")
    .with_path(&info.path);
    snapshot.battery = BatteryReading::Unknown;
    snapshot.status = BatteryStatus::Unsupported;
    snapshot.last_seen_unix = Some(now);
    snapshot
}

/// 用户 profile 是否应该在本接口上尝试被动读取。
///
/// 内置厂商协议或标准 HID 电量任一命中时都返回 false：内置结果优先，
/// 用户配置分支因此完全不产生 I/O。
fn should_read_user_profile(vendor_hit: bool, standard_hit: bool, has_profiles: bool) -> bool {
    !vendor_hit && !standard_hit && has_profiles
}

/// 被动读取用户配置电量；失败只写诊断，绝不伪造 0%。
fn read_user_snapshot(
    info: &hid::HidInterface,
    profiles: &UserProfileCatalog,
    now: u64,
    diagnostics: &mut Vec<String>,
) -> Option<DeviceSnapshot> {
    let profile = match user_profile::find_profile(profiles, info) {
        Ok(Some(profile)) => profile,
        Ok(None) => return None,
        Err(err) => {
            diagnostics.push(format!("    -> 用户配置匹配失败: {err}"));
            return None;
        }
    };
    let mut reader = match ReadOnlyHid::open(&info.path) {
        Some(reader) => reader,
        None => {
            diagnostics.push("    -> 用户配置读取失败: 无法以只读方式打开接口".to_owned());
            return None;
        }
    };
    // 单位是毫秒：下限避免空转，上限避免拖慢整轮扫描。
    let wait = Duration::from_millis(profile.spec.response.timeout_ms.clamp(100, 500));
    match user_profile::read_user_level_with(&mut reader, profile, wait) {
        Ok(Some(level)) => {
            diagnostics.push(format!(
                "    -> 用户配置 {}% · {} · 未验证",
                level, profile.spec.name
            ));
            Some(user_profile::snapshot_from_user(info, profile, level, now))
        }
        Ok(None) => {
            diagnostics.push(format!(
                "    -> 用户配置未读到有效报文（等待 {} ms）",
                wait.as_millis()
            ));
            None
        }
        Err(err) => {
            diagnostics.push(format!("    -> 用户配置读取失败: {}", err.label_zh()));
            None
        }
    }
}

/// 写入用户配置结果：只补空位（未知/未找到协议），不覆盖任何已知内置电量。
fn insert_user(map: &mut HashMap<String, DeviceSnapshot>, key: String, snapshot: DeviceSnapshot) {
    match map.get_mut(&key) {
        Some(existing) => {
            if existing.battery.is_known() {
                // 保留更强的内置读数，只在名字更长时补充显示名。
                if snapshot.name.len() > existing.name.len() {
                    existing.name = snapshot.name;
                }
                return;
            }
            *existing = snapshot;
        }
        None => {
            map.insert(key, snapshot);
        }
    }
}

fn likely_wireless(info: &hid::HidInterface) -> bool {
    let name = info.product.to_lowercase();
    let words = [
        "wireless",
        "hyperspeed",
        "lightspeed",
        "receiver",
        "dongle",
        "2.4g",
        "air",
        "pro",
    ];
    words.iter().any(|word| name.contains(word))
        || hid::is_wireless_product(info.vendor_id, info.product_id)
        || matches!(
            info.vendor_id,
            0x046D
                | 0x1532
                | 0x3554
                | 0x373B
                | 0x3710
                | 0x3770
                | 0x3057
                | 0x3367
                | 0x1915
                | 0x3654
                | 0x1038
                | 0x03F0
                | 0x1B1C
                | 0x0B05
                | 0x3434
                | 0x388D
                | 0x1E71
                | 0x36A7
                | 0x373E
                | 0x2023
                | 0x5253
                | 0x3329
                | 0x0ECB
                | 0x28DE
                | 0x1D57
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
    if snapshot.status.is_live() {
        base + 5
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_profile::fixtures::{catalog, interface, profile};
    use anybattery_core::Transport;

    fn known_snapshot() -> DeviceSnapshot {
        DeviceSnapshot::new(
            DeviceId::new("hid", "container-a"),
            "示例接收器",
            DeviceKind::infer_from_name("示例接收器"),
            Transport::Usb,
            BatterySource::new("内置协议", Confidence::Verified, "厂商报文"),
        )
        .with_battery(BatteryReading::percent(90, None), 1)
    }

    #[test]
    fn insert_user_does_not_override_known_battery() {
        let key = "container:container-a".to_owned();
        let mut map = HashMap::new();
        map.insert(key.clone(), known_snapshot());
        let snapshot = user_profile::snapshot_from_user(&interface(), &profile(), 12, 2);
        insert_user(&mut map, key.clone(), snapshot);
        let stored = map.get(&key).expect("entry stays");
        assert_eq!(stored.percent(), Some(90));
        assert_eq!(stored.source.verification, Confidence::Verified);
        assert_eq!(stored.source.provider, "内置协议");
    }

    #[test]
    fn insert_user_fills_unknown() {
        let info = interface();
        let key = "container:container-a".to_owned();
        let mut map = HashMap::new();
        map.insert(key.clone(), unsupported_snapshot(&info, 1));
        let snapshot = user_profile::snapshot_from_user(&info, &profile(), 42, 2);
        insert_user(&mut map, key.clone(), snapshot);
        let stored = map.get(&key).expect("entry stays");
        assert_eq!(stored.percent(), Some(42));
        assert_eq!(stored.source.provider, "用户配置");
        assert_eq!(stored.source.verification, Confidence::Unverified);
        assert_ne!(stored.status, BatteryStatus::Unsupported);
    }

    #[test]
    fn insert_user_adds_new_entry() {
        let key = "container:container-a".to_owned();
        let mut map = HashMap::new();
        let snapshot = user_profile::snapshot_from_user(&interface(), &profile(), 7, 3);
        insert_user(&mut map, key.clone(), snapshot);
        assert_eq!(map.get(&key).and_then(DeviceSnapshot::percent), Some(7));
    }

    #[test]
    fn user_read_is_skipped_when_builtin_succeeded() {
        assert!(!should_read_user_profile(true, false, true));
        assert!(!should_read_user_profile(false, true, true));
        assert!(!should_read_user_profile(true, true, true));
        assert!(!should_read_user_profile(false, false, false));
        assert!(should_read_user_profile(false, false, true));
    }

    #[test]
    fn user_snapshot_is_unverified() {
        let snapshot = user_profile::snapshot_from_user(&interface(), &profile(), 33, 5);
        assert_eq!(snapshot.percent(), Some(33));
        assert_eq!(snapshot.source.verification, Confidence::Unverified);
        assert_eq!(snapshot.battery.charging(), None);
        assert_eq!(snapshot.detail, snapshot.source.detail);
        assert!(snapshot.detail.contains("被动输入"));
    }

    #[test]
    fn catalog_resolves_interface_profile() {
        let catalog = catalog();
        let found = user_profile::find_profile(&catalog, &interface()).expect("binding ok");
        assert!(found.is_some());
    }
}
