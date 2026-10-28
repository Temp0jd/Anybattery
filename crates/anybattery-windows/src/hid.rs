use anybattery_core::{BatterySource, Confidence, DeviceId, DeviceKind, DeviceSnapshot, Transport};
use std::ffi::c_void;
use std::mem::size_of;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_IDW, CR_BUFFER_SMALL, CR_SUCCESS,
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_REPORT_TYPE, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS,
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidD_SetFeature, HidP_Feature, HidP_GetCaps,
    HidP_GetUsageValue, HidP_GetValueCaps, HidP_Input, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds,
    DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_LIST,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{GUID, HRESULT, PCWSTR};

pub const HID_USAGE_PAGE_GENERIC_DEVICE: u16 = 0x0006;
pub const HID_USAGE_GENERIC_DEVICE_BATTERY_STRENGTH: u16 = 0x0020;
pub const HID_USAGE_PAGE_DIGITIZER: u16 = 0x000D;
pub const HID_USAGE_DIGITIZER_BATTERY: u16 = 0x003B;
pub const HID_USAGE_PAGE_BATTERY_SYSTEM: u16 = 0x0085;
pub const HID_USAGE_BATTERY_SYSTEM_CHARGING: u16 = 0x0044;
pub const HID_USAGE_BATTERY_SYSTEM_DISCHARGING: u16 = 0x0045;
pub const HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE: u16 = 0x0064;
pub const HID_USAGE_BATTERY_SYSTEM_ABSOLUTE_STATE_OF_CHARGE: u16 = 0x0065;

const STANDARD_BATTERY_CANDIDATES: [(u16, u16); 4] = [
    (
        HID_USAGE_PAGE_BATTERY_SYSTEM,
        HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
    ),
    (
        HID_USAGE_PAGE_BATTERY_SYSTEM,
        HID_USAGE_BATTERY_SYSTEM_ABSOLUTE_STATE_OF_CHARGE,
    ),
    (
        HID_USAGE_PAGE_GENERIC_DEVICE,
        HID_USAGE_GENERIC_DEVICE_BATTERY_STRENGTH,
    ),
    (HID_USAGE_PAGE_DIGITIZER, HID_USAGE_DIGITIZER_BATTERY),
];

#[derive(Clone, Debug, Default)]
pub struct HidInterface {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    pub product: String,
    pub manufacturer: String,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: u16,
    pub output_len: u16,
    pub feature_len: u16,
    pub interface_number: Option<u16>,
    pub container_id: Option<String>,
    pub instance_id: Option<String>,
}

impl HidInterface {
    pub fn label(&self) -> String {
        if !self.product.is_empty() {
            return self.product.clone();
        }
        if !self.manufacturer.is_empty() {
            return format!(
                "{} {:04X}:{:04X}",
                self.manufacturer, self.vendor_id, self.product_id
            );
        }
        format!("HID {:04X}:{:04X}", self.vendor_id, self.product_id)
    }

    pub fn is_vendor_collection(&self) -> bool {
        self.usage_page >= 0xFF00
    }
}

pub fn enumerate() -> Vec<HidInterface> {
    let mut out = Vec::new();
    let guid = unsafe { HidD_GetHidGuid() };
    let Ok(set) = (unsafe {
        SetupDiGetClassDevsW(
            Some(&guid),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    }) else {
        return out;
    };

    let mut index = 0u32;
    loop {
        let mut iface = SP_DEVICE_INTERFACE_DATA::default();
        iface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInterfaces(set, None, &guid, index, &mut iface) }.is_err() {
            break;
        }
        index += 1;

        let mut required = 0u32;
        let mut devinfo = SP_DEVINFO_DATA::default();
        devinfo.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        let _ = unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &iface,
                None,
                0,
                Some(&mut required),
                Some(&mut devinfo),
            )
        };
        if required == 0 {
            continue;
        }

        let mut buffer = vec![0u8; required as usize];
        let cb = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        buffer[0..4].copy_from_slice(&cb.to_le_bytes());
        let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &iface,
                Some(detail),
                required,
                Some(&mut required),
                Some(&mut devinfo),
            )
        }
        .is_err()
        {
            continue;
        }

        let path = utf16_path(&buffer[4..]);
        if path.is_empty() {
            continue;
        }
        let container_id = devnode_guid(devinfo.DevInst).map(guid_string);
        let instance_id = devnode_instance_id(devinfo.DevInst);
        let Some(mut info) = inspect(&path) else {
            continue;
        };
        info.container_id = container_id;
        info.instance_id = instance_id;
        out.push(info);
    }

    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
    out
}

fn inspect(path: &str) -> Option<HidInterface> {
    // Attributes, strings and preparsed caps do not require read access. A
    // zero-access synchronous handle also matches the working Razer reference
    // and avoids drivers that report empty caps on an overlapped read handle.
    let handle = open_sync(path).ok()?;
    let mut attrs = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    let has_attrs = unsafe { HidD_GetAttributes(handle, &mut attrs) };
    let product = product_string(handle);
    let manufacturer = manufacturer_string(handle);

    let mut caps = HIDP_CAPS::default();
    let mut preparsed = PHIDP_PREPARSED_DATA(0);
    let mut caps_ok = false;
    if unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
        caps_ok = unsafe { HidP_GetCaps(preparsed, &mut caps) } == HIDP_STATUS_SUCCESS;
        unsafe { HidD_FreePreparsedData(preparsed) };
    }
    unsafe {
        let _ = CloseHandle(handle);
    };

    if !has_attrs && !caps_ok && product.is_empty() {
        return None;
    }

    Some(HidInterface {
        path: path.to_owned(),
        vendor_id: attrs.VendorID,
        product_id: attrs.ProductID,
        version: attrs.VersionNumber,
        product,
        manufacturer,
        usage_page: caps.UsagePage,
        usage: caps.Usage,
        input_len: caps.InputReportByteLength,
        output_len: caps.OutputReportByteLength,
        feature_len: caps.FeatureReportByteLength,
        interface_number: interface_number(path),
        container_id: None,
        instance_id: None,
    })
}

pub fn interface_number(path: &str) -> Option<u16> {
    let lower = path.to_ascii_lowercase();
    let idx = lower.find("&mi_")?;
    let rest = &lower[idx + 4..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u16::from_str_radix(&hex, 16).ok()
}

pub fn is_bluetooth_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("vid&")
        || lower.contains("00001124-0000-1000-8000-00805f9b34fb")
        || lower.contains("00001812-0000-1000-8000-00805f9b34fb")
        || lower.contains("bthle")
}

pub fn open_path(path: &str, write: bool) -> Option<HANDLE> {
    let wide = to_wide(path);
    let access = if write {
        windows::Win32::Foundation::GENERIC_READ.0 | windows::Win32::Foundation::GENERIC_WRITE.0
    } else {
        windows::Win32::Foundation::GENERIC_READ.0
    };
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
            None,
        )
    };
    if let Ok(handle) = handle {
        return Some(handle);
    }
    if write {
        return open_path(path, false);
    }
    None
}

pub fn with_handle<T>(path: &str, write: bool, f: impl FnOnce(HANDLE) -> Option<T>) -> Option<T> {
    let handle = open_path(path, write)?;
    let result = f(handle);
    unsafe {
        let _ = CloseHandle(handle);
    };
    result
}

/// Open a HID collection exactly as FerrisPulse does for Razer queries:
/// zero desired access and no overlapped flag. Razer SetFeature/GetFeature is
/// synchronous and some receivers do not answer on an overlapped read/write
/// handle. Keep this separate from the shared overlapped path.
pub fn open_sync(path: &str) -> Result<HANDLE, u32> {
    let wide = to_wide(path);
    match unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    } {
        Ok(handle) => Ok(handle),
        Err(_) => Err(unsafe { windows::Win32::Foundation::GetLastError() }.0),
    }
}

pub fn with_sync_handle<T>(path: &str, f: impl FnOnce(HANDLE) -> Option<T>) -> Option<T> {
    let handle = open_sync(path).ok()?;
    let result = f(handle);
    close(handle);
    result
}

pub fn close(handle: HANDLE) {
    let _ = unsafe { CloseHandle(handle) };
}

fn product_string(handle: HANDLE) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe {
        HidD_GetProductString(
            handle,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 2) as u32,
        )
    };
    if ok { trim_wide(&buf) } else { String::new() }
}

fn manufacturer_string(handle: HANDLE) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe {
        windows::Win32::Devices::HumanInterfaceDevice::HidD_GetManufacturerString(
            handle,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 2) as u32,
        )
    };
    if ok { trim_wide(&buf) } else { String::new() }
}

pub fn feature_get(path: &str, length: usize) -> Option<Vec<u8>> {
    with_handle(path, false, |handle| {
        let mut buf = vec![0u8; length.max(1)];
        let ok =
            unsafe { HidD_GetFeature(handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        if ok { Some(buf) } else { None }
    })
}

pub fn feature_set(path: &str, bytes: &[u8]) -> bool {
    with_handle(path, true, |handle| {
        feature_set_on(handle, bytes).then_some(())
    })
    .is_some()
}

/// Set one feature report on an already-open handle.
///
/// Callers that must also read the matching input report (which some receivers
/// deliver only on the handle that sent the Feature Set) use this instead of
/// reopening the path.
pub fn feature_set_on(handle: HANDLE, bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 4096 {
        return false;
    }
    unsafe { HidD_SetFeature(handle, bytes.as_ptr() as *const c_void, bytes.len() as u32) }
}

pub fn feature_get_on(handle: HANDLE, report_id: u8, length: usize) -> Option<Vec<u8>> {
    if length == 0 || length > 4096 {
        return None;
    }
    let mut reply = vec![0u8; length];
    reply[0] = report_id;
    unsafe {
        HidD_GetFeature(
            handle,
            reply.as_mut_ptr() as *mut c_void,
            reply.len() as u32,
        )
    }
    .then_some(reply)
}

/// Set one feature report and read replies from the same open handle.
///
/// Some receivers only answer a Feature Set when the matching Feature Get
/// follows on the same file handle. `attempts` and the settle delay are clamped
/// so a missing or stale reply stays bounded. The first byte of `request` is the
/// report ID and is requested back in the reply buffer.
pub fn feature_set_get<T>(
    path: &str,
    request: &[u8],
    report_len: usize,
    settle_ms: u32,
    attempts: u8,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    if request.is_empty() || request.len() > 4096 || report_len == 0 || report_len > 4096 {
        return None;
    }
    with_handle(path, true, |handle| {
        let ok = unsafe {
            HidD_SetFeature(
                handle,
                request.as_ptr() as *const c_void,
                request.len() as u32,
            )
        };
        if !ok {
            return None;
        }
        let delay = std::time::Duration::from_millis(settle_ms.min(1000) as u64);
        for _ in 0..attempts.clamp(1, 8) {
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let mut buffer = vec![0u8; report_len];
            buffer[0] = request[0];
            let ok = unsafe {
                HidD_GetFeature(
                    handle,
                    buffer.as_mut_ptr() as *mut c_void,
                    report_len as u32,
                )
            };
            if ok {
                if let Some(value) = parse(&buffer) {
                    return Some(value);
                }
            }
        }
        None
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureIoError {
    InvalidLength,
    SetFailed,
    GetFailed,
}

/// The feature-report operations used by the shared length helpers.
///
/// Kept as a trait so the caps padding and write/read failure paths can be
/// exercised without a real HID handle. `HidD_SetFeature`/`HidD_GetFeature`
/// are synchronous and cannot be cancelled, so callers must keep attempts
/// bounded; see `feature_poll`/`feature_exchange`.
pub(crate) trait FeatureTransport {
    fn set_feature(&mut self, bytes: &[u8]) -> bool;
    fn get_feature(&mut self, length: usize, report_id: u8) -> Option<Vec<u8>>;
}

struct HidFeatureTransport {
    handle: HANDLE,
}

impl FeatureTransport for HidFeatureTransport {
    fn set_feature(&mut self, bytes: &[u8]) -> bool {
        unsafe {
            HidD_SetFeature(
                self.handle,
                bytes.as_ptr() as *const c_void,
                bytes.len() as u32,
            )
        }
    }

    fn get_feature(&mut self, length: usize, report_id: u8) -> Option<Vec<u8>> {
        let mut reply = vec![0u8; length];
        reply[0] = report_id;
        let ok = unsafe {
            HidD_GetFeature(
                self.handle,
                reply.as_mut_ptr() as *mut c_void,
                reply.len() as u32,
            )
        };
        ok.then_some(reply)
    }
}

/// Build a caps-length Feature Set and read a caps-length Feature Get.
///
/// Windows requires the Set buffer to be at least `FeatureReportByteLength`
/// even when the protocol frame is shorter (hidapi pads to caps for the same
/// reason). The returned report is truncated to `protocol_len` before parsing.
pub(crate) fn feature_set_get_caps<T: FeatureTransport>(
    io: &mut T,
    request: &[u8],
    caps_len: usize,
    protocol_len: usize,
    delay: std::time::Duration,
) -> Result<Vec<u8>, FeatureIoError> {
    if request.is_empty()
        || caps_len == 0
        || caps_len > 4096
        || request.len() > caps_len
        || protocol_len == 0
        || protocol_len > caps_len
    {
        return Err(FeatureIoError::InvalidLength);
    }
    let mut framed = vec![0u8; caps_len];
    framed[..request.len()].copy_from_slice(request);
    if !io.set_feature(&framed) {
        return Err(FeatureIoError::SetFailed);
    }
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    let Some(mut reply) = io.get_feature(caps_len, request[0]) else {
        return Err(FeatureIoError::GetFailed);
    };
    if reply.len() < protocol_len {
        return Err(FeatureIoError::GetFailed);
    }
    reply.truncate(protocol_len);
    Ok(reply)
}

/// Set a feature report and read one reply on an already-open handle.
///
/// The report ID is the first byte of both buffers, as required by
/// `HidD_SetFeature`/`HidD_GetFeature`. `request.len()` must equal `read_len`;
/// callers that need the caps-reported length must build the request at that
/// length rather than padding after the fact.
pub fn feature_set_get_on(
    handle: HANDLE,
    request: &[u8],
    read_len: usize,
    delay: std::time::Duration,
) -> Option<Vec<u8>> {
    if request.len() != read_len {
        return None;
    }
    let mut io = HidFeatureTransport { handle };
    feature_set_get_caps(&mut io, request, read_len, read_len, delay).ok()
}

/// Set a caps-sized Feature report and read a caps-sized reply, then return
/// only the first `protocol_len` bytes for the protocol parser.
pub fn feature_set_get_caps_on(
    handle: HANDLE,
    request: &[u8],
    caps_len: usize,
    protocol_len: usize,
    delay: std::time::Duration,
) -> Result<Vec<u8>, FeatureIoError> {
    let mut io = HidFeatureTransport { handle };
    feature_set_get_caps(&mut io, request, caps_len, protocol_len, delay)
}

/// Send one feature report once, then poll replies on the same handle.
///
/// PixArt/JM answers with a delayed result, so `attempts` reads follow a single
/// Set. Parse failures (waiting/ACK frames) simply consume one attempt.
pub fn feature_poll<T>(
    path: &str,
    request: &[u8],
    read_len: usize,
    attempts: u8,
    delay: std::time::Duration,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    if request.is_empty() || request.len() != read_len || read_len > 4096 {
        return None;
    }
    with_handle(path, true, |handle| {
        let set_ok = unsafe {
            HidD_SetFeature(
                handle,
                request.as_ptr() as *const c_void,
                request.len() as u32,
            )
        };
        if !set_ok {
            return None;
        }
        for _ in 0..attempts.clamp(1, 15) {
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let mut reply = vec![0u8; read_len];
            reply[0] = request[0];
            let get_ok = unsafe {
                HidD_GetFeature(
                    handle,
                    reply.as_mut_ptr() as *mut c_void,
                    reply.len() as u32,
                )
            };
            if get_ok {
                if let Some(value) = parse(&reply) {
                    return Some(value);
                }
            }
        }
        None
    })
}

/// Run a bounded Set+Get exchange on one handle, rebuilding the request for
/// each attempt. MCHOSE must re-send its query before every read; Delux embeds
/// an incrementing sequence number in the request.
pub fn feature_exchange<T>(
    path: &str,
    read_len: usize,
    attempts: u8,
    delay: std::time::Duration,
    mut build_request: impl FnMut(u32) -> Vec<u8>,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    if read_len == 0 || read_len > 4096 {
        return None;
    }
    with_handle(path, true, |handle| {
        for attempt in 0..attempts.clamp(1, 8) as u32 {
            let request = build_request(attempt);
            match feature_set_get_caps_on(handle, &request, read_len, read_len, delay) {
                Ok(reply) => {
                    if let Some(value) = parse(&reply) {
                        return Some(value);
                    }
                }
                // A failed Set (or a malformed request) is not retryable; a
                // failed read can be retried with a fresh Set.
                Err(FeatureIoError::SetFailed) | Err(FeatureIoError::InvalidLength) => break,
                Err(FeatureIoError::GetFailed) => {}
            }
        }
        None
    })
}

pub fn output_write(path: &str, bytes: &[u8]) -> bool {
    with_handle(path, true, |handle| {
        write_report(handle, bytes).then_some(())
    })
    .is_some()
}

pub fn input_read(path: &str, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    with_handle(path, false, |handle| {
        read_overlapped(handle, length, timeout_ms)
    })
}

pub fn write_report(handle: HANDLE, bytes: &[u8]) -> bool {
    let Ok(event) = (unsafe { CreateEventW(None, true, false, PCWSTR::null()) }) else {
        return false;
    };
    let mut overlapped = OVERLAPPED::default();
    overlapped.hEvent = event;
    let result = unsafe { WriteFile(handle, Some(bytes), None, Some(&mut overlapped)) };
    let ok = match result {
        Ok(()) => true,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
            if unsafe { WaitForSingleObject(event, 1000) } == WAIT_OBJECT_0 {
                let mut written = 0u32;
                unsafe { GetOverlappedResult(handle, &overlapped, &mut written, false) }.is_ok()
            } else {
                let _ = unsafe { CancelIoEx(handle, Some(&overlapped)) };
                let _ = unsafe { WaitForSingleObject(event, 1000) };
                false
            }
        }
        Err(_) => false,
    };
    let _ = unsafe { CloseHandle(event) };
    ok
}

pub fn read_report(handle: HANDLE, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    read_overlapped(handle, length, timeout_ms)
}

pub fn listen<T>(
    path: &str,
    length: usize,
    total_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    with_handle(path, false, |handle| {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(total_ms as u64);
        while std::time::Instant::now() < deadline {
            if let Some(report) = read_overlapped(handle, length, 150) {
                if let Some(value) = parse(&report) {
                    return Some(value);
                }
            }
        }
        None
    })
}

pub fn write_and_read(
    path: &str,
    request: &[u8],
    read_len: usize,
    timeout_ms: u32,
) -> Option<Vec<u8>> {
    with_handle(path, true, |handle| {
        if !write_report(handle, request) {
            return None;
        }
        read_overlapped(handle, read_len, timeout_ms)
    })
}

/// Write one output report and keep reading input reports until `parse` accepts
/// a packet or the total window expires. Echo packets are left to the parser.
pub fn write_and_listen<T>(
    path: &str,
    request: &[u8],
    read_len: usize,
    total_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    with_handle(path, true, |handle| {
        if !write_report(handle, request) {
            return None;
        }
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(total_ms as u64);
        while std::time::Instant::now() < deadline {
            if let Some(report) = read_overlapped(handle, read_len, 150) {
                if let Some(value) = parse(&report) {
                    return Some(value);
                }
            }
        }
        None
    })
}

fn read_overlapped(handle: HANDLE, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;
    let mut overlapped = OVERLAPPED::default();
    overlapped.hEvent = event;
    let mut buffer = vec![0u8; length.max(1)];
    let mut read = 0u32;
    let result = unsafe {
        ReadFile(
            handle,
            Some(buffer.as_mut_slice()),
            Some(&mut read),
            Some(&mut overlapped),
        )
    };
    let ok = match result {
        Ok(()) => true,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
            if unsafe { WaitForSingleObject(event, timeout_ms) } == WAIT_OBJECT_0 {
                unsafe { GetOverlappedResult(handle, &overlapped, &mut read, false) }.is_ok()
            } else {
                let _ = unsafe { CancelIoEx(handle, Some(&overlapped)) };
                let _ = unsafe { WaitForSingleObject(event, 1000) };
                false
            }
        }
        Err(_) => false,
    };
    let _ = unsafe { CloseHandle(event) };
    if ok && read > 0 {
        buffer.truncate(read as usize);
        Some(buffer)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct StandardBattery {
    pub level: u8,
    pub charging: Option<bool>,
    pub usage_page: u16,
    pub usage: u16,
    pub report_id: u8,
    pub raw: u32,
    pub logical_min: i32,
    pub logical_max: i32,
}

pub fn read_standard_battery(info: &HidInterface) -> Option<StandardBattery> {
    with_handle(&info.path, false, |handle| {
        let mut preparsed = PHIDP_PREPARSED_DATA(0);
        if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
            return None;
        }
        let result = (|| {
            let mut caps = HIDP_CAPS::default();
            if unsafe { HidP_GetCaps(preparsed, &mut caps) } != HIDP_STATUS_SUCCESS {
                return None;
            }
            for (page, usage) in STANDARD_BATTERY_CANDIDATES {
                if let Some(battery) =
                    read_feature_usage(handle, preparsed, caps.FeatureReportByteLength, page, usage)
                {
                    return Some(battery);
                }
                if let Some(battery) =
                    read_input_usage(handle, preparsed, caps.InputReportByteLength, page, usage)
                {
                    return Some(battery);
                }
            }
            None
        })();
        unsafe { HidD_FreePreparsedData(preparsed) };
        result
    })
}

fn value_caps(
    preparsed: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
    count: u16,
) -> Vec<HIDP_VALUE_CAPS> {
    if count == 0 {
        return Vec::new();
    }
    let mut caps = vec![HIDP_VALUE_CAPS::default(); count as usize];
    let mut len = count;
    let status = unsafe { HidP_GetValueCaps(report_type, caps.as_mut_ptr(), &mut len, preparsed) };
    if status == HIDP_STATUS_SUCCESS {
        caps.truncate(len as usize);
        caps
    } else {
        Vec::new()
    }
}

fn cap_matches(cap: &HIDP_VALUE_CAPS, page: u16, usage: u16) -> bool {
    if cap.UsagePage != page {
        return false;
    }
    let actual = if cap.IsRange {
        unsafe { cap.Anonymous.Range.UsageMin <= usage && cap.Anonymous.Range.UsageMax >= usage }
    } else {
        unsafe { cap.Anonymous.NotRange.Usage == usage }
    };
    actual && cap.BitSize > 0 && cap.BitSize <= 32
}

fn find_caps<'a>(
    caps: &'a [HIDP_VALUE_CAPS],
    page: u16,
    usage: u16,
) -> Option<&'a HIDP_VALUE_CAPS> {
    caps.iter().find(|cap| cap_matches(cap, page, usage))
}

/// Identity of the link collection that owns one battery value.
///
/// Battery System flags live next to the percentage inside the same collection.
/// When a report exposes several batteries (left/right/case), only flags with
/// the same report and collection may be attached to that percentage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BatteryCollectionKey {
    report_id: u8,
    link_collection: u16,
    link_usage_page: u16,
    link_usage: u16,
}

impl BatteryCollectionKey {
    fn of(cap: &HIDP_VALUE_CAPS) -> Self {
        Self {
            report_id: cap.ReportID,
            link_collection: cap.LinkCollection,
            link_usage_page: cap.LinkUsagePage,
            link_usage: cap.LinkUsage,
        }
    }

    fn matches(&self, cap: &HIDP_VALUE_CAPS) -> bool {
        cap.ReportID == self.report_id
            && cap.LinkCollection == self.link_collection
            && cap.LinkUsagePage == self.link_usage_page
            && cap.LinkUsage == self.link_usage
    }
}

fn flag_cap<'a>(
    caps: &'a [HIDP_VALUE_CAPS],
    battery: BatteryCollectionKey,
    usage: u16,
) -> Option<&'a HIDP_VALUE_CAPS> {
    let mut matching = caps.iter().filter(|cap| {
        battery.matches(cap) && cap_matches(cap, HID_USAGE_PAGE_BATTERY_SYSTEM, usage)
    });
    let cap = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    Some(cap)
}

fn read_feature_usage(
    handle: HANDLE,
    preparsed: PHIDP_PREPARSED_DATA,
    report_len: u16,
    page: u16,
    usage: u16,
) -> Option<StandardBattery> {
    if report_len == 0 {
        return None;
    }
    let caps = value_caps(preparsed, HidP_Feature, feature_value_count(preparsed));
    let cap = find_caps(&caps, page, usage)?;
    let report_id = cap.ReportID;
    let mut buffer = vec![0u8; report_len as usize];
    buffer[0] = report_id;
    let ok = unsafe {
        HidD_GetFeature(
            handle,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as u32,
        )
    };
    if !ok {
        return None;
    }
    extract_usage(preparsed, HidP_Feature, &buffer, &caps, cap, usage)
}

fn read_input_usage(
    handle: HANDLE,
    preparsed: PHIDP_PREPARSED_DATA,
    report_len: u16,
    page: u16,
    usage: u16,
) -> Option<StandardBattery> {
    if report_len == 0 {
        return None;
    }
    let caps = value_caps(preparsed, HidP_Input, input_value_count(preparsed));
    let cap = find_caps(&caps, page, usage)?;
    let report_id = cap.ReportID;
    let mut buffer = vec![0u8; report_len as usize];
    buffer[0] = report_id;
    // Input reports are not readable through HidD_GetFeature; listen briefly instead.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(900);
    while std::time::Instant::now() < deadline {
        let Some(mut report) = read_overlapped(handle, report_len as usize, 250) else {
            continue;
        };
        if report_id != 0 && report.first().copied() != Some(report_id) {
            continue;
        }
        if report.len() < report_len as usize {
            report.resize(report_len as usize, 0);
        }
        if let Some(battery) = extract_usage(preparsed, HidP_Input, &report, &caps, cap, usage) {
            return Some(battery);
        }
    }
    None
}

fn feature_value_count(preparsed: PHIDP_PREPARSED_DATA) -> u16 {
    let mut caps = HIDP_CAPS::default();
    if unsafe { HidP_GetCaps(preparsed, &mut caps) } == HIDP_STATUS_SUCCESS {
        caps.NumberFeatureValueCaps
    } else {
        0
    }
}

fn input_value_count(preparsed: PHIDP_PREPARSED_DATA) -> u16 {
    let mut caps = HIDP_CAPS::default();
    if unsafe { HidP_GetCaps(preparsed, &mut caps) } == HIDP_STATUS_SUCCESS {
        caps.NumberInputValueCaps
    } else {
        0
    }
}

fn extract_usage(
    preparsed: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
    report: &[u8],
    caps: &[HIDP_VALUE_CAPS],
    cap: &HIDP_VALUE_CAPS,
    usage: u16,
) -> Option<StandardBattery> {
    let collection = BatteryCollectionKey::of(cap);
    let value = usage_value(preparsed, report_type, report, cap, usage)?;
    let min = cap.LogicalMin;
    let max = cap.LogicalMax;
    if max <= min {
        return None;
    }
    let percent =
        ((value as i64 - min as i64) * 100 / (max as i64 - min as i64)).clamp(0, 100) as u8;
    if percent == 0 && value as i32 == min {
        // A zero reading is usually "not reported yet" rather than empty.
        return None;
    }
    let charging = resolve_charging(
        report_flag(
            preparsed,
            report_type,
            report,
            caps,
            collection,
            HID_USAGE_BATTERY_SYSTEM_CHARGING,
        ),
        report_flag(
            preparsed,
            report_type,
            report,
            caps,
            collection,
            HID_USAGE_BATTERY_SYSTEM_DISCHARGING,
        ),
    );
    Some(StandardBattery {
        level: percent,
        charging,
        usage_page: cap.UsagePage,
        usage,
        report_id: cap.ReportID,
        raw: value,
        logical_min: min,
        logical_max: max,
    })
}

fn usage_value(
    preparsed: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
    report: &[u8],
    cap: &HIDP_VALUE_CAPS,
    usage: u16,
) -> Option<u32> {
    let mut value = 0u32;
    let status = unsafe {
        HidP_GetUsageValue(
            report_type,
            cap.UsagePage,
            Some(cap.LinkCollection),
            usage,
            &mut value,
            preparsed,
            report,
        )
    };
    (status == HIDP_STATUS_SUCCESS).then_some(value)
}

fn report_flag(
    preparsed: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
    report: &[u8],
    caps: &[HIDP_VALUE_CAPS],
    battery: BatteryCollectionKey,
    usage: u16,
) -> Option<u32> {
    let cap = flag_cap(caps, battery, usage)?;
    usage_value(preparsed, report_type, report, cap, usage)
}

/// Resolve the standard Charging/Discharging booleans from one report.
/// Charging is decisive unless both flags claim 1; a discharge-only 1 means
/// not charging. A discharge-only 0 or absent flags stay unknown.
fn resolve_charging(charging: Option<u32>, discharging: Option<u32>) -> Option<bool> {
    match (charging, discharging) {
        (Some(1), Some(1)) => None,
        (Some(1), _) => Some(true),
        (Some(0), _) => Some(false),
        (None, Some(1)) => Some(false),
        _ => None,
    }
}

pub fn standard_snapshot(
    info: &HidInterface,
    battery: &StandardBattery,
    now: u64,
) -> DeviceSnapshot {
    let kind = DeviceKind::infer_from_name(&info.product);
    let transport = transport_for_product(&info.path, info.vendor_id, info.product_id);
    DeviceSnapshot::new(
        DeviceId::new(
            "hid",
            &info.container_id.clone().unwrap_or_else(|| {
                format!(
                    "{:04X}:{:04X}:{}",
                    info.vendor_id,
                    info.product_id,
                    short_hash(&info.path)
                )
            }),
        ),
        info.label(),
        kind,
        transport,
        BatterySource::new(
            "标准 HID 电量",
            Confidence::OsReported,
            format!(
                "usage {:04X}:{:04X}, report {:#04X}, raw {} / {}",
                battery.usage_page,
                battery.usage,
                battery.report_id,
                battery.raw,
                battery.logical_max
            ),
        ),
    )
    .with_battery(
        anybattery_core::BatteryReading::percent(battery.level, battery.charging),
        now,
    )
    .with_detail(format!("HID {}:{:04X}", info.usage_page, info.usage))
    .with_path(&info.path)
}

pub fn transport_for(path: &str, vendor_id: u16) -> Transport {
    transport_for_product(path, vendor_id, 0)
}

/// Transport classification that can distinguish a wireless PID from a wired
/// sibling under the same vendor ID.
pub fn transport_for_product(path: &str, vendor_id: u16, product_id: u16) -> Transport {
    let lower = path.to_ascii_lowercase();
    if lower.contains("bthle") {
        Transport::BluetoothLe
    } else if is_bluetooth_path(&lower) || lower.contains("bthenum") {
        Transport::BluetoothClassic
    } else if is_wireless_vid(vendor_id) || is_wireless_product(vendor_id, product_id) {
        Transport::Usb24g
    } else {
        Transport::Usb
    }
}

fn is_wireless_vid(vendor_id: u16) -> bool {
    matches!(
        vendor_id,
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

/// Wireless products whose vendor ID also covers wired or unrelated devices.
pub fn is_wireless_product(vendor_id: u16, product_id: u16) -> bool {
    matches!(
        (vendor_id, product_id),
        (0x3837, 0x100C | 0x100B) | (0x248A, 0x5B2F)
    )
}

pub fn short_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016X}", hash)
}

fn devnode_guid(devinst: u32) -> Option<GUID> {
    let bytes = devnode_property(devinst, &DEVPKEY_Device_ContainerId, DEVPROP_TYPE_GUID)?;
    if bytes.len() < size_of::<GUID>() {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const GUID) })
}

fn devnode_instance_id(devinst: u32) -> Option<String> {
    let mut buf = [0u16; 512];
    if unsafe { CM_Get_Device_IDW(devinst, &mut buf, 0) } == CR_SUCCESS {
        Some(trim_wide(&buf))
    } else {
        None
    }
}

fn devnode_property(
    devinst: u32,
    key: &windows::Win32::Foundation::DEVPROPKEY,
    expected: windows::Win32::Devices::Properties::DEVPROPTYPE,
) -> Option<Vec<u8>> {
    let mut property_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
    let mut size = 0u32;
    let status =
        unsafe { CM_Get_DevNode_PropertyW(devinst, key, &mut property_type, None, &mut size, 0) };
    if status != CR_SUCCESS && status != CR_BUFFER_SMALL {
        return None;
    }
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            devinst,
            key,
            &mut property_type,
            Some(buffer.as_mut_ptr()),
            &mut size,
            0,
        )
    };
    if status != CR_SUCCESS || property_type != expected {
        return None;
    }
    buffer.truncate(size as usize);
    Some(buffer)
}

pub fn friendly_name(devinst: u32) -> Option<String> {
    let bytes = devnode_property(devinst, &DEVPKEY_Device_FriendlyName, DEVPROP_TYPE_STRING)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

pub fn hardware_ids(devinst: u32) -> Vec<String> {
    let Some(bytes) = devnode_property(
        devinst,
        &DEVPKEY_Device_HardwareIds,
        DEVPROP_TYPE_STRING_LIST,
    ) else {
        return Vec::new();
    };
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 0..units.len() {
        if units[i] == 0 {
            if i == start {
                break;
            }
            out.push(String::from_utf16_lossy(&units[start..i]));
            start = i + 1;
        }
    }
    out
}

pub fn guid_string(guid: GUID) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

pub fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn trim_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn utf16_path(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

pub fn listen_idle<T>(
    path: &str,
    length: usize,
    total_ms: u32,
    idle_ms: u32,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    with_handle(path, false, |handle| {
        let started = std::time::Instant::now();
        let mut last_report = started;
        while started.elapsed() < std::time::Duration::from_millis(total_ms as u64) {
            if let Some(report) = read_overlapped(handle, length, 150) {
                last_report = std::time::Instant::now();
                if let Some(value) = parse(&report) {
                    return Some(value);
                }
            } else if last_report.elapsed() > std::time::Duration::from_millis(idle_ms as u64) {
                return None;
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anybattery_core::Transport;

    fn battery_link(index: u16) -> (u16, u16, u16) {
        (
            index,
            HID_USAGE_PAGE_GENERIC_DEVICE,
            HID_USAGE_GENERIC_DEVICE_BATTERY_STRENGTH,
        )
    }

    fn battery_cap(
        usage: u16,
        report_id: u8,
        (link_collection, link_usage_page, link_usage): (u16, u16, u16),
    ) -> HIDP_VALUE_CAPS {
        let mut cap = HIDP_VALUE_CAPS::default();
        cap.UsagePage = HID_USAGE_PAGE_BATTERY_SYSTEM;
        cap.ReportID = report_id;
        cap.LinkCollection = link_collection;
        cap.LinkUsagePage = link_usage_page;
        cap.LinkUsage = link_usage;
        cap.BitSize = 8;
        unsafe {
            cap.Anonymous.NotRange.Usage = usage;
        }
        cap
    }

    #[test]
    fn charging_flag_reports_charging() {
        assert_eq!(resolve_charging(Some(1), None), Some(true));
        assert_eq!(resolve_charging(Some(1), Some(0)), Some(true));
    }

    #[test]
    fn charging_false_without_conflict_reports_not_charging() {
        assert_eq!(resolve_charging(Some(0), None), Some(false));
        assert_eq!(resolve_charging(Some(0), Some(0)), Some(false));
        assert_eq!(resolve_charging(Some(0), Some(1)), Some(false));
    }

    #[test]
    fn discharging_alone_reports_not_charging() {
        assert_eq!(resolve_charging(None, Some(1)), Some(false));
    }

    #[test]
    fn charging_and_discharging_conflict_is_unknown() {
        assert_eq!(resolve_charging(Some(1), Some(1)), None);
    }

    #[test]
    fn discharging_false_alone_is_unknown() {
        assert_eq!(resolve_charging(None, Some(0)), None);
    }

    #[test]
    fn missing_flags_are_unknown() {
        assert_eq!(resolve_charging(None, None), None);
    }

    #[test]
    fn charging_flag_requires_same_report_and_link_collection() {
        let battery = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            3,
            battery_link(1),
        );
        let key = BatteryCollectionKey::of(&battery);
        let other_report = battery_cap(HID_USAGE_BATTERY_SYSTEM_CHARGING, 4, battery_link(1));
        let other_collection = battery_cap(HID_USAGE_BATTERY_SYSTEM_CHARGING, 3, battery_link(2));

        assert!(
            flag_cap(
                &[battery, other_report],
                key,
                HID_USAGE_BATTERY_SYSTEM_CHARGING
            )
            .is_none()
        );
        assert!(
            flag_cap(
                &[battery, other_collection],
                key,
                HID_USAGE_BATTERY_SYSTEM_CHARGING
            )
            .is_none()
        );
    }

    #[test]
    fn left_and_right_battery_flags_do_not_cross_collections() {
        let left = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            2,
            battery_link(1),
        );
        let right = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            2,
            battery_link(2),
        );
        let left_charging = battery_cap(HID_USAGE_BATTERY_SYSTEM_CHARGING, 2, battery_link(1));
        let right_charging = battery_cap(HID_USAGE_BATTERY_SYSTEM_CHARGING, 2, battery_link(2));
        let left_key = BatteryCollectionKey::of(&left);
        let right_key = BatteryCollectionKey::of(&right);
        let caps = [left, right, left_charging, right_charging];

        let left_flag = flag_cap(&caps, left_key, HID_USAGE_BATTERY_SYSTEM_CHARGING)
            .expect("left charging flag");
        let right_flag = flag_cap(&caps, right_key, HID_USAGE_BATTERY_SYSTEM_CHARGING)
            .expect("right charging flag");
        assert_eq!(left_flag.LinkCollection, 1);
        assert_eq!(right_flag.LinkCollection, 2);

        let only_right = [caps[0], caps[1], caps[3]];
        assert!(
            flag_cap(&only_right, left_key, HID_USAGE_BATTERY_SYSTEM_CHARGING).is_none(),
            "left percentage must not borrow the right battery charging flag"
        );
    }

    #[test]
    fn charging_flag_matches_the_full_link_usage_identity() {
        let battery = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            5,
            (
                1,
                HID_USAGE_PAGE_BATTERY_SYSTEM,
                HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            ),
        );
        let key = BatteryCollectionKey::of(&battery);
        let same_index_other_usage = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_CHARGING,
            5,
            (
                1,
                HID_USAGE_PAGE_GENERIC_DEVICE,
                HID_USAGE_GENERIC_DEVICE_BATTERY_STRENGTH,
            ),
        );
        let same_collection = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_CHARGING,
            5,
            (
                1,
                HID_USAGE_PAGE_BATTERY_SYSTEM,
                HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            ),
        );

        assert!(
            flag_cap(
                &[battery, same_index_other_usage],
                key,
                HID_USAGE_BATTERY_SYSTEM_CHARGING
            )
            .is_none()
        );
        assert!(
            flag_cap(
                &[battery, same_collection],
                key,
                HID_USAGE_BATTERY_SYSTEM_CHARGING
            )
            .is_some()
        );
    }

    #[test]
    fn ambiguous_charging_flags_in_one_collection_are_ignored() {
        let battery = battery_cap(
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE,
            1,
            battery_link(1),
        );
        let key = BatteryCollectionKey::of(&battery);
        let charging = battery_cap(HID_USAGE_BATTERY_SYSTEM_CHARGING, 1, battery_link(1));

        assert!(
            flag_cap(
                &[battery, charging, charging],
                key,
                HID_USAGE_BATTERY_SYSTEM_CHARGING
            )
            .is_none()
        );
    }

    #[test]
    fn range_cap_matches_only_contained_usages() {
        let mut cap = HIDP_VALUE_CAPS::default();
        cap.UsagePage = HID_USAGE_PAGE_BATTERY_SYSTEM;
        cap.BitSize = 8;
        cap.IsRange = true;
        unsafe {
            cap.Anonymous.Range.UsageMin = HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE;
            cap.Anonymous.Range.UsageMax = HID_USAGE_BATTERY_SYSTEM_ABSOLUTE_STATE_OF_CHARGE;
        }

        assert!(cap_matches(
            &cap,
            HID_USAGE_PAGE_BATTERY_SYSTEM,
            HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE
        ));
        assert!(cap_matches(
            &cap,
            HID_USAGE_PAGE_BATTERY_SYSTEM,
            HID_USAGE_BATTERY_SYSTEM_ABSOLUTE_STATE_OF_CHARGE
        ));
        assert!(!cap_matches(&cap, HID_USAGE_PAGE_BATTERY_SYSTEM, 0x0066));
    }

    #[test]
    fn battery_system_candidates_precede_generic_and_digitizer() {
        assert_eq!(
            STANDARD_BATTERY_CANDIDATES,
            [
                (
                    HID_USAGE_PAGE_BATTERY_SYSTEM,
                    HID_USAGE_BATTERY_SYSTEM_RELATIVE_STATE_OF_CHARGE
                ),
                (
                    HID_USAGE_PAGE_BATTERY_SYSTEM,
                    HID_USAGE_BATTERY_SYSTEM_ABSOLUTE_STATE_OF_CHARGE
                ),
                (
                    HID_USAGE_PAGE_GENERIC_DEVICE,
                    HID_USAGE_GENERIC_DEVICE_BATTERY_STRENGTH
                ),
                (HID_USAGE_PAGE_DIGITIZER, HID_USAGE_DIGITIZER_BATTERY),
            ]
        );
        assert!(!STANDARD_BATTERY_CANDIDATES.contains(&(HID_USAGE_PAGE_BATTERY_SYSTEM, 0x0066)));
        assert!(
            !STANDARD_BATTERY_CANDIDATES
                .iter()
                .any(|(_, usage)| *usage == 0x0001)
        );
    }

    #[test]
    fn bthle_hid_paths_are_bluetooth_le() {
        let path =
            r"\\?\BTHLE\DEV_001122334455\7&2e94cec3&0&0000#{00001812-0000-1000-8000-00805f9b34fb}";
        assert_eq!(transport_for(path, 0x046D), Transport::BluetoothLe);
    }

    #[test]
    fn oem_24g_vids_are_marked_as_usb_24g() {
        for vid in [0x36A7, 0x373E, 0x2023] {
            assert_eq!(
                transport_for(r"\\?\HID#VID_36A7&PID_A880\7&1&0&0000", vid),
                Transport::Usb24g,
                "{vid:#06X}"
            );
        }
        let path = r"\\?\HID#VID_248A&PID_5B2F\7&1&0&0000";
        assert_eq!(transport_for(path, 0x248A), Transport::Usb);
        assert_eq!(
            transport_for_product(path, 0x248A, 0x5B2F),
            Transport::Usb24g
        );
        assert_eq!(transport_for_product(path, 0x248A, 0x5B2E), Transport::Usb);
        assert_eq!(
            transport_for_product(path, 0x3837, 0x100B),
            Transport::Usb24g
        );
        assert_eq!(
            transport_for_product(path, 0x3837, 0x100C),
            Transport::Usb24g
        );
        assert_eq!(transport_for_product(path, 0x3837, 0x1101), Transport::Usb);
        assert_eq!(transport_for_product(path, 0x3837, 0x600A), Transport::Usb);
    }

    #[derive(Default)]
    struct FakeFeatureTransport {
        set_ok: bool,
        reply: Option<Vec<u8>>,
        set_calls: Vec<Vec<u8>>,
        get_calls: Vec<(usize, u8)>,
    }

    impl FeatureTransport for FakeFeatureTransport {
        fn set_feature(&mut self, bytes: &[u8]) -> bool {
            self.set_calls.push(bytes.to_vec());
            self.set_ok
        }

        fn get_feature(&mut self, length: usize, report_id: u8) -> Option<Vec<u8>> {
            self.get_calls.push((length, report_id));
            self.reply.clone()
        }
    }

    fn mchose_short_frame() -> Vec<u8> {
        let mut frame = vec![0xFFu8; 21];
        frame[0] = 0x11;
        frame[1] = 0xF9;
        frame
    }

    #[test]
    fn feature_set_get_caps_pads_set_and_reads_with_caps_buffer() {
        let request = mchose_short_frame();
        let mut reply = vec![0x11u8; 65];
        reply[20] = 0x42;
        reply[21] = 0x99;
        let mut io = FakeFeatureTransport {
            set_ok: true,
            reply: Some(reply),
            ..Default::default()
        };

        let parsed =
            feature_set_get_caps(&mut io, &request, 65, 21, std::time::Duration::ZERO).unwrap();

        assert_eq!(io.set_calls.len(), 1);
        assert_eq!(io.set_calls[0].len(), 65);
        assert_eq!(&io.set_calls[0][..21], request.as_slice());
        assert!(io.set_calls[0][21..].iter().all(|byte| *byte == 0));
        assert_eq!(io.get_calls, vec![(65, 0x11)]);
        assert_eq!(parsed.len(), 21);
        assert_eq!(parsed[20], 0x42);
    }

    #[test]
    fn feature_set_get_caps_keeps_exact_caps_length() {
        let request = mchose_short_frame();
        let mut io = FakeFeatureTransport {
            set_ok: true,
            reply: Some(request.clone()),
            ..Default::default()
        };
        let parsed =
            feature_set_get_caps(&mut io, &request, 21, 21, std::time::Duration::ZERO).unwrap();
        assert_eq!(io.set_calls[0].len(), 21);
        assert_eq!(io.get_calls, vec![(21, 0x11)]);
        assert_eq!(parsed, request);
    }

    #[test]
    fn feature_set_get_caps_reports_set_and_get_failures_separately() {
        let request = mchose_short_frame();
        let mut set_failed = FakeFeatureTransport {
            set_ok: false,
            reply: Some(vec![0x11; 65]),
            ..Default::default()
        };
        assert_eq!(
            feature_set_get_caps(&mut set_failed, &request, 65, 21, std::time::Duration::ZERO),
            Err(FeatureIoError::SetFailed)
        );
        assert!(set_failed.get_calls.is_empty());

        let mut get_failed = FakeFeatureTransport {
            set_ok: true,
            reply: None,
            ..Default::default()
        };
        assert_eq!(
            feature_set_get_caps(&mut get_failed, &request, 65, 21, std::time::Duration::ZERO),
            Err(FeatureIoError::GetFailed)
        );
    }

    #[test]
    fn feature_set_get_caps_rejects_short_set_buffer_and_bad_protocol_length() {
        let request = mchose_short_frame();
        let mut io = FakeFeatureTransport {
            set_ok: true,
            reply: Some(vec![0x11; 21]),
            ..Default::default()
        };
        assert_eq!(
            feature_set_get_caps(&mut io, &request, 20, 21, std::time::Duration::ZERO),
            Err(FeatureIoError::InvalidLength)
        );
        assert_eq!(
            feature_set_get_caps(&mut io, &request, 65, 66, std::time::Duration::ZERO),
            Err(FeatureIoError::InvalidLength)
        );
        assert!(io.set_calls.is_empty());
    }

    #[test]
    fn bthenum_paths_are_bluetooth_classic() {
        let path = r"\\?\HID#{00001124-0000-1000-8000-00805f9b34fb}_vid&000205cc_pid&0000\7&2e94cec3&0&0000";
        assert_eq!(transport_for(path, 0x046D), Transport::BluetoothClassic);
        assert_eq!(
            transport_for(r"\\?\BTHENUM\DEV_001122334455\8&1", 0x046D),
            Transport::BluetoothClassic
        );
    }
}
