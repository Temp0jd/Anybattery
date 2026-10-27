use anybattery_core::{
    BatteryReading, BatterySource, Confidence, DeviceId, DeviceKind, DeviceSnapshot, Transport,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEScanningMode,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

use crate::bluetooth::BluetoothDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApplePart {
    level: u8,
    charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppleContinuity {
    model_id: u16,
    left: Option<ApplePart>,
    right: Option<ApplePart>,
    case: Option<ApplePart>,
    lid_open: bool,
}

#[derive(Debug, Clone)]
struct TimedContinuity {
    state: AppleContinuity,
    received_unix: u64,
}

static STATES: OnceLock<Mutex<HashMap<u16, TimedContinuity>>> = OnceLock::new();
static WATCHER: OnceLock<Mutex<Option<BluetoothLEAdvertisementWatcher>>> = OnceLock::new();

fn states() -> &'static Mutex<HashMap<u16, TimedContinuity>> {
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn apple_part(nibble: u8, charging: bool) -> Option<ApplePart> {
    (nibble <= 10).then_some(ApplePart {
        level: nibble * 10,
        charging,
    })
}

fn parse_continuity(data: &[u8]) -> Option<AppleContinuity> {
    if data.len() < 27 || data[0] != 0x07 {
        return None;
    }
    let model_id = u16::from_le_bytes([data[3], data[4]]);
    let from_left = (data[5] >> 5) & 1 == 1;
    let current = data[6] & 0x0F;
    let another = data[6] >> 4;
    let current_charging = data[7] & 0x10 != 0;
    let another_charging = data[7] & 0x20 != 0;
    let (left, right) = if from_left {
        (
            apple_part(current, current_charging),
            apple_part(another, another_charging),
        )
    } else {
        (
            apple_part(another, another_charging),
            apple_part(current, current_charging),
        )
    };
    Some(AppleContinuity {
        model_id,
        left,
        right,
        case: apple_part(data[7] & 0x0F, data[7] & 0x40 != 0),
        lid_open: (data[8] >> 3) & 1 == 0,
    })
}

fn model_name(model_id: u16) -> String {
    match model_id {
        0x2002 => "AirPods (第 1 代)".to_owned(),
        0x200F => "AirPods (第 2 代)".to_owned(),
        0x2013 => "AirPods (第 3 代)".to_owned(),
        0x2019 => "AirPods 4".to_owned(),
        0x201B => "AirPods 4 (主动降噪版)".to_owned(),
        0x2030 | 0x2036 => "AirPods (新代)".to_owned(),
        0x200E => "AirPods Pro (第 1 代)".to_owned(),
        0x2014 => "AirPods Pro 2 (Lightning)".to_owned(),
        0x2024 => "AirPods Pro 2 (USB-C)".to_owned(),
        0x2027 => "AirPods Pro 3".to_owned(),
        0x200A => "AirPods Max".to_owned(),
        0x201F => "AirPods Max (USB-C)".to_owned(),
        0x2011 => "Beats Studio Buds".to_owned(),
        0x2017 => "Beats Studio Buds +".to_owned(),
        0x2012 => "Beats Fit Pro".to_owned(),
        0x2029 | 0x2032 => "Beats Solo Buds".to_owned(),
        0x2026 => "Beats Solo 4".to_owned(),
        0x2006 => "Beats Solo3 Wireless".to_owned(),
        0x200C => "Beats Solo Pro".to_owned(),
        0x2018 => "Beats Studio Pro".to_owned(),
        0x2009 => "Beats Studio3 Wireless".to_owned(),
        0x200B => "Powerbeats Pro".to_owned(),
        0x2003 => "Powerbeats 3 Wireless".to_owned(),
        0x200D => "Powerbeats 4".to_owned(),
        0x2010 => "Beats Flex".to_owned(),
        0x2005 => "BeatsX".to_owned(),
        _ => format!("Apple / Beats 音频设备 ({model_id:04X})"),
    }
}

fn receive(args: &BluetoothLEAdvertisementReceivedEventArgs) -> windows::core::Result<()> {
    let advertisement = args.Advertisement()?;
    for manufacturer in advertisement.ManufacturerData()? {
        if manufacturer.CompanyId()? != 0x004C {
            continue;
        }
        let buffer = manufacturer.Data()?;
        let reader = DataReader::FromBuffer(&buffer)?;
        let mut data = vec![0u8; reader.UnconsumedBufferLength()? as usize];
        reader.ReadBytes(&mut data)?;
        if let Some(state) = parse_continuity(&data) {
            if let Ok(mut states) = states().lock() {
                states.insert(
                    state.model_id,
                    TimedContinuity {
                        state,
                        received_unix: anybattery_core::now_unix(),
                    },
                );
            }
        }
    }
    Ok(())
}

fn ensure_watcher() {
    let watcher = WATCHER.get_or_init(|| Mutex::new(None));
    let Ok(mut slot) = watcher.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    let Ok(instance) = BluetoothLEAdvertisementWatcher::new() else {
        return;
    };
    let _ = instance.SetScanningMode(BluetoothLEScanningMode::Active);
    let handler: TypedEventHandler<
        BluetoothLEAdvertisementWatcher,
        BluetoothLEAdvertisementReceivedEventArgs,
    > = TypedEventHandler::new(
        move |_, args: windows::core::Ref<'_, BluetoothLEAdvertisementReceivedEventArgs>| {
            let args = args.ok()?;
            let _ = receive(args);
            Ok(())
        },
    );
    if instance.Received(&handler).is_err() || instance.Start().is_err() {
        return;
    }
    *slot = Some(instance);
}

fn is_paired_apple_audio(device: &BluetoothDevice) -> bool {
    if !device.connected {
        return false;
    }
    let id = device.instance_id.to_ascii_uppercase();
    let name = device.name.to_ascii_lowercase();
    let apple_hardware = id.contains("VID&0001004C")
        || id.contains("VID&000205AC")
        || id.contains("VID_004C")
        || id.contains("VID_05AC");
    let apple_name = ["airpods", "beats", "powerbeats"]
        .iter()
        .any(|word| name.contains(word));
    let magic = ["magic mouse", "magic keyboard", "magic trackpad"]
        .iter()
        .any(|word| name.contains(word));
    (apple_hardware || apple_name) && !magic
}

fn owner<'a>(
    state: &AppleContinuity,
    paired: &'a [BluetoothDevice],
) -> Option<&'a BluetoothDevice> {
    let candidates: Vec<_> = paired
        .iter()
        .filter(|device| is_paired_apple_audio(device))
        .collect();
    let pid_amp = format!("PID&{:04X}", state.model_id);
    let pid_under = format!("PID_{:04X}", state.model_id);
    if let Some(device) = candidates.iter().find(|device| {
        let id = device.instance_id.to_ascii_uppercase();
        id.contains(&pid_amp) || id.contains(&pid_under)
    }) {
        return Some(*device);
    }
    let model = model_name(state.model_id).to_ascii_lowercase();
    let family = if model.contains("airpods") {
        Some("airpods")
    } else if model.contains("beats") || model.contains("powerbeats") {
        Some("beats")
    } else {
        None
    };
    if let Some(family) = family {
        let matches: Vec<_> = candidates
            .iter()
            .filter(|device| {
                let name = device.name.to_ascii_lowercase();
                name.contains(family) || (family == "beats" && name.contains("powerbeats"))
            })
            .collect();
        if matches.len() == 1 {
            return Some(*matches[0]);
        }
    }
    (candidates.len() == 1).then_some(candidates[0])
}

pub fn snapshots(now: u64, paired: &[BluetoothDevice]) -> Vec<DeviceSnapshot> {
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    ensure_watcher();
    let Ok(states) = states().lock() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for timed in states.values() {
        if now.saturating_sub(timed.received_unix) > 30 {
            continue;
        }
        let state = &timed.state;
        let Some(owner) = owner(state, paired) else {
            continue;
        };
        let model = model_name(state.model_id);
        for (suffix, label, part) in [
            ("left", "左耳", state.left),
            ("right", "右耳", state.right),
            ("case", "充电盒", state.case),
        ] {
            let Some(part) = part else {
                continue;
            };
            let detail = format!(
                "Apple Continuity 0x004C · model {:04X} · {label} · 盒盖{}",
                state.model_id,
                if state.lid_open { "打开" } else { "关闭" }
            );
            out.push(
                DeviceSnapshot::new(
                    DeviceId::new(
                        "ble",
                        &format!(
                            "apple-continuity:{}:{:04X}:{suffix}",
                            owner.id, state.model_id
                        ),
                    ),
                    format!("{model} {label}"),
                    DeviceKind::Earbuds,
                    Transport::BluetoothLe,
                    BatterySource::new("Apple Continuity", Confidence::Community, detail.clone()),
                )
                .with_battery(
                    BatteryReading::percent(part.level, Some(part.charging)),
                    now,
                )
                .with_detail(detail)
                .with_path(format!("paired:{}", owner.instance_id)),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{AppleContinuity, ApplePart, model_name, parse_continuity};

    fn advertisement(from_left: bool, current: u8, another: u8, case: u8, flags: u8) -> Vec<u8> {
        let mut data = vec![0u8; 27];
        data[0] = 0x07;
        data[3..5].copy_from_slice(&0x200Eu16.to_le_bytes());
        data[5] = if from_left { 0x20 } else { 0 };
        data[6] = current | (another << 4);
        data[7] = case | flags;
        data
    }

    #[test]
    fn continuity_parser_swaps_left_right_and_decodes_all_parts() {
        let data = advertisement(true, 8, 7, 6, 0x70);
        assert_eq!(
            parse_continuity(&data),
            Some(AppleContinuity {
                model_id: 0x200E,
                left: Some(ApplePart {
                    level: 80,
                    charging: true
                }),
                right: Some(ApplePart {
                    level: 70,
                    charging: true
                }),
                case: Some(ApplePart {
                    level: 60,
                    charging: true
                }),
                lid_open: true,
            })
        );
        let from_right = advertisement(false, 8, 7, 6, 0x70);
        let state = parse_continuity(&from_right).unwrap();
        assert_eq!(state.left.map(|part| part.level), Some(70));
        assert_eq!(state.right.map(|part| part.level), Some(80));
    }

    #[test]
    fn continuity_parser_rejects_wrong_type_short_and_invalid_nibbles() {
        assert_eq!(parse_continuity(&vec![0u8; 26]), None);
        let mut wrong_type = advertisement(true, 8, 7, 6, 0);
        wrong_type[0] = 0x10;
        assert_eq!(parse_continuity(&wrong_type), None);
        let invalid = advertisement(true, 15, 7, 11, 0);
        let state = parse_continuity(&invalid).unwrap();
        assert_eq!(state.left, None);
        assert_eq!(state.case, None);
        assert_eq!(state.right.map(|part| part.level), Some(70));
    }

    #[test]
    fn continuity_model_catalog_covers_airpods_and_beats() {
        assert_eq!(model_name(0x2024), "AirPods Pro 2 (USB-C)");
        assert_eq!(model_name(0x2012), "Beats Fit Pro");
        assert!(model_name(0x9999).contains("9999"));
    }
}
