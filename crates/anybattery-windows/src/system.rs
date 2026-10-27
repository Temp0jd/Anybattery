use std::ffi::c_void;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::{PCWSTR, w};

/// Reads HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme.
/// Returns true when the system is in dark mode (value 0).
pub fn is_dark_mode() -> bool {
    let mut value = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && value == 0
}

pub fn accent_color() -> Option<(u8, u8, u8)> {
    // Windows' accent color is stored as an ABGR DWORD.
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    Some((
        (value & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        ((value >> 16) & 0xFF) as u8,
    ))
}

pub fn config_dir() -> std::path::PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return std::path::PathBuf::from(appdata).join("AnyBattery");
    }
    std::path::PathBuf::from(".")
}

pub fn set_autostart(enabled: bool) -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    use windows::core::w;
    let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value = w!("AnyBattery");
    if !enabled {
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey, value) };
        return status == ERROR_SUCCESS || status.0 == 2; // ERROR_FILE_NOT_FOUND
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let command = format!("\"{}\"", exe.display());
    let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            REG_SZ.0,
            Some(wide.as_ptr() as *const c_void),
            (wide.len() * 2) as u32,
        )
    };
    status == ERROR_SUCCESS
}

pub fn autostart_enabled() -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            w!("AnyBattery"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && size > 0
}
