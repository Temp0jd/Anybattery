#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod widgets;

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    use std::sync::{Arc, Mutex};

    use anybattery_core::Settings;
    use crossbeam_channel::unbounded;
    use eframe::egui;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "anybattery=info".into()),
        )
        .try_init();

    let settings = Settings::default();
    let wake: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    let (command_tx, command_rx) = unbounded();
    let wake_for_tray = {
        let wake = wake.clone();
        Arc::new(move || {
            if let Ok(guard) = wake.lock() {
                if let Some(ctx) = guard.as_ref() {
                    ctx.request_repaint();
                }
            }
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let tray = tray::build(command_tx, wake_for_tray, settings.autostart);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([430.0, 680.0])
            .with_min_inner_size([380.0, 520.0])
            .with_decorations(false)
            .with_resizable(true)
            .with_title("AnyBattery")
            .with_icon(Arc::new(icons::window_icon())),
        ..Default::default()
    };

    eframe::run_native(
        "AnyBattery",
        options,
        Box::new(move |cc| {
            let app = app::AnyBatteryApp::new(cc, command_rx, tray, wake);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(not(windows))]
fn main() {
    eprintln!("AnyBattery 仅支持 Windows 10/11。");
}
