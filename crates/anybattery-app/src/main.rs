#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod logging;
#[cfg(windows)]
mod protocol_wizard;
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
    use anybattery_windows::instance::{AcquireInstance, acquire_or_signal};
    use crossbeam_channel::unbounded;
    use eframe::egui;

    logging::init();
    logging::install_panic_hook();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %storage::data_dir().display(),
        "starting AnyBattery"
    );

    // The first process owns the mutex/event; a second one only asks the
    // primary to show its window and exits before creating a tray or a scan.
    let instance = match acquire_or_signal() {
        Ok(AcquireInstance::Primary(primary)) => Some(primary),
        Ok(AcquireInstance::Secondary) => return Ok(()),
        Err(err) => {
            // Single-instance protection is a usability guard, not a safety
            // boundary: a Win32 failure must not prevent the app from starting.
            tracing::error!(%err, "单实例保护不可用，继续启动");
            None
        }
    };

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
    if let Some(primary) = &instance {
        let tx = command_tx.clone();
        let wake = wake_for_tray.clone();
        if let Err(err) = primary.spawn_show_watcher(move || {
            if tx.send(app::UiCommand::Show).is_ok() {
                wake();
            }
        }) {
            tracing::error!(%err, "无法启动单实例唤醒监听");
        }
    }

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

    let result = eframe::run_native(
        "AnyBattery",
        options,
        Box::new(move |cc| {
            let app = app::AnyBatteryApp::new(cc, command_rx, tray, wake);
            Ok(Box::new(app))
        }),
    );
    // Keep the named mutex/event alive until the window loop has exited.
    drop(instance);
    result
}

#[cfg(not(windows))]
fn main() {
    eprintln!("AnyBattery 仅支持 Windows 10/11。");
}
