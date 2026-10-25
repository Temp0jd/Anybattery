use anybattery_core::{
    BatteryReading, BatterySource, Confidence, DeviceId, DeviceKind, DeviceSnapshot, Transport,
};
use windows::Gaming::Input::RawGameController;
use windows::System::Power::BatteryStatus;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

pub fn enumerate(now: u64) -> Vec<DeviceSnapshot> {
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let Ok(controllers) = RawGameController::RawGameControllers() else {
        return Vec::new();
    };
    let Ok(count) = controllers.Size() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..count {
        let Ok(controller) = controllers.GetAt(index) else {
            continue;
        };
        let display = controller
            .DisplayName()
            .map(|name| name.to_string())
            .unwrap_or_else(|_| "Xbox 控制器".to_owned());
        let name = if display.trim().is_empty() {
            "Xbox 控制器".to_owned()
        } else {
            display
        };
        let vendor = controller.HardwareVendorId().unwrap_or(0);
        let product = controller.HardwareProductId().unwrap_or(0);
        let id = controller
            .NonRoamableId()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| format!("wgi:{:04X}:{:04X}:{}", vendor, product, index));

        let Ok(report) = controller.TryGetBatteryReport() else {
            out.push(unknown(DeviceId(id), name, vendor, product, now));
            continue;
        };
        let status = report.Status().ok();
        let remaining = report
            .RemainingCapacityInMilliwattHours()
            .ok()
            .and_then(|value| value.Value().ok());
        let full = report
            .FullChargeCapacityInMilliwattHours()
            .ok()
            .and_then(|value| value.Value().ok());

        let charging = match status {
            Some(BatteryStatus::Charging) => Some(true),
            Some(BatteryStatus::Discharging) => Some(false),
            Some(BatteryStatus::Idle) => None,
            Some(BatteryStatus::NotPresent) | None => None,
            _ => None,
        };
        let level = match (remaining, full) {
            (Some(remaining), Some(full)) if full > 0 && remaining >= 0 => {
                Some((((remaining as i64 * 100) + full as i64 / 2) / full as i64).clamp(0, 100) as u8)
            }
            _ => None,
        };

        let mut snapshot = DeviceSnapshot::new(
            DeviceId(id),
            name,
            DeviceKind::Controller,
            Transport::Usb,
            BatterySource::new(
                "Windows.Gaming.Input",
                Confidence::OsReported,
                format!(
                    "VID {:04X} PID {:04X} · {} / {} mWh · {:?}",
                    vendor, product, remaining.unwrap_or(0), full.unwrap_or(0), status
                ),
            ),
        )
        .with_detail("Xbox / GameSir / FlyDigi · 系统电池报告")
        .with_path(format!("wgi:{:04X}:{:04X}", vendor, product));

        if let Some(level) = level {
            snapshot = snapshot.with_battery(BatteryReading::percent(level, charging), now);
        } else {
            snapshot.battery = BatteryReading::Unknown;
            snapshot.status = anybattery_core::BatteryStatus::Unsupported;
            snapshot.last_seen_unix = Some(now);
        }
        out.push(snapshot);
    }
    out
}

fn unknown(id: DeviceId, name: String, vendor: u16, product: u16, now: u64) -> DeviceSnapshot {
    let mut snapshot = DeviceSnapshot::new(
        id,
        name,
        DeviceKind::Controller,
        Transport::Usb,
        BatterySource::new(
            "Windows.Gaming.Input",
            Confidence::OsReported,
            format!("VID {:04X} PID {:04X}", vendor, product),
        ),
    )
    .with_detail("手柄已连接，但系统没有电池报告")
    .with_path(format!("wgi:{:04X}:{:04X}", vendor, product));
    snapshot.status = anybattery_core::BatteryStatus::Unsupported;
    snapshot.last_seen_unix = Some(now);
    snapshot
}
