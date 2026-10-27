use anybattery_core::Settings;

fn main() {
    let settings = Settings {
        show_unknown: true,
        ..Settings::default()
    };
    let result = anybattery_windows::scan(&settings);
    for line in result.diagnostics {
        println!("{line}");
    }
    println!("设备结果: {}", result.devices.len());
    for device in result.devices {
        println!(
            "  {} · {} · 电量 {:?} · {}",
            device.name,
            device.status.label_zh(),
            device.battery,
            device.detail
        );
    }
}
