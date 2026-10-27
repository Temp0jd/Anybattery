use crossbeam_channel::Sender;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app::UiCommand;
use crate::icons;

pub struct TrayHandle {
    pub icon: TrayIcon,
    pub autostart: CheckMenuItem,
}

pub fn build(
    tx: Sender<UiCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
    autostart_checked: bool,
) -> Option<TrayHandle> {
    let show = MenuItem::with_id("show", "显示面板", true, None);
    let refresh = MenuItem::with_id("refresh", "立即刷新", true, None);
    let settings = MenuItem::with_id("settings", "设置", true, None);
    let diagnostics = MenuItem::with_id("diagnostics", "诊断", true, None);
    let autostart = CheckMenuItem::with_id("autostart", "开机自启", true, autostart_checked, None);
    let quit = MenuItem::with_id("quit", "退出 AnyBattery", true, None);

    let menu = Menu::new();
    menu.append(&show).ok()?;
    menu.append(&refresh).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&settings).ok()?;
    menu.append(&diagnostics).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&autostart).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&quit).ok()?;

    let menu_tx = tx.clone();
    let menu_wake = wake.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id.0.clone();
        if menu_tx.send(UiCommand::Menu(id)).is_ok() {
            menu_wake();
        }
    }));

    let tray_tx = tx;
    let tray_wake = wake;
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let command = match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => Some(UiCommand::Toggle),
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => Some(UiCommand::Show),
            _ => None,
        };
        if let Some(command) = command {
            if tray_tx.send(command).is_ok() {
                tray_wake();
            }
        }
    }));

    let icon: Icon = icons::tray_icon(None, false)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("AnyBattery · 正在扫描")
        .with_icon(icon)
        .build()
        .ok()?;

    Some(TrayHandle {
        icon: tray,
        autostart,
    })
}
