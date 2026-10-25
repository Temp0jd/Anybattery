use anybattery_core::{BatterySource, Confidence, DeviceId, DeviceKind, DeviceSnapshot, Transport};
use std::ffi::c_void;
use std::mem::size_of;
use windows::core::{GUID, HRESULT, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_DevNode_PropertyW, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    CR_BUFFER_SMALL, CR_SUCCESS, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidD_SetFeature, HidP_GetCaps,
    HidP_GetUsageValue, HidP_GetValueCaps, HidP_Feature, HidP_Input, HIDD_ATTRIBUTES,
    HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds,
    DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_LIST,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

pub const HID_USAGE_PAGE_BATTERY: u16 = 0x0006;
pub const HID_USAGE_BATTERY_STRENGTH: u16 = 0x0020;
pub const HID_USAGE_PAGE_DIGITIZER: u16 = 0x000D;
pub const HID_USAGE_DIGITIZER_BATTERY: u16 = 0x003B;
pub const HID_USAGE_PAGE_POWER: u16 = 0x0085;
pub const HID_USAGE_POWER_RELATIVE: u16 = 0x0066;
pub const HID_USAGE_POWER_BATTERY_LEVEL: u16 = 0x0065;
pub const HID_USAGE_POWER_ABSOLUTE: u16 = 0x0001;

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
    pub container_id: Option<String>,
    pub instance_id: Option<String>,
}

impl HidInterface {
    pub fn label(&self) -> String {
        if !self.product.is_empty() {
            return self.product.clone();
        }
        if !self.manufacturer.is_empty() {
            return format!("{} {:04X}:{:04X}", self.manufacturer, self.vendor_id, self.product_id);
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
        if unsafe {
            SetupDiEnumDeviceInterfaces(set, None, &guid, index, &mut iface)
        }
        .is_err()
        {
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
    let handle = open_path(path, false)?;
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
    unsafe { CloseHandle(handle) };

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
        container_id: None,
        instance_id: None,
    })
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
            FILE_ATTRIBUTE_NORMAL,
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
    unsafe { CloseHandle(handle) };
    result
}

pub fn close(handle: HANDLE) {
    unsafe { CloseHandle(handle) };
}

fn product_string(handle: HANDLE) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe {
        HidD_GetProductString(handle, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as u32)
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
        let ok = unsafe {
            HidD_GetFeature(
                handle,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
            )
        };
        if ok { Some(buf) } else { None }
    })
}

pub fn feature_set(path: &str, bytes: &[u8]) -> bool {
    with_handle(path, true, |handle| {
        let ok = unsafe {
            HidD_SetFeature(
                handle,
                bytes.as_ptr() as *const c_void,
                bytes.len() as u32,
            )
        };
        ok.then_some(())
    })
    .is_some()
}

pub fn output_write(path: &str, bytes: &[u8]) -> bool {
    with_handle(path, true, |handle| {
        let result = unsafe { WriteFile(handle, Some(bytes), None, None) };
        result.is_ok().then_some(())
    })
    .is_some()
}

pub fn input_read(path: &str, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    with_handle(path, false, |handle| read_overlapped(handle, length, timeout_ms))
}

pub fn write_and_read(path: &str, request: &[u8], read_len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    with_handle(path, true, |handle| {
        if unsafe { WriteFile(handle, Some(request), None, None) }.is_err() {
            return None;
        }
        read_overlapped(handle, read_len, timeout_ms)
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
            let candidates = [
                (HID_USAGE_PAGE_BATTERY, HID_USAGE_BATTERY_STRENGTH),
                (HID_USAGE_PAGE_DIGITIZER, HID_USAGE_DIGITIZER_BATTERY),
                (HID_USAGE_PAGE_POWER, HID_USAGE_POWER_RELATIVE),
                (HID_USAGE_PAGE_POWER, HID_USAGE_POWER_BATTERY_LEVEL),
                (HID_USAGE_PAGE_POWER, HID_USAGE_POWER_ABSOLUTE),
            ];

            for (page, usage) in candidates {
                if let Some(battery) = read_feature_usage(handle, preparsed, caps.FeatureReportByteLength, page, usage) {
                    return Some(battery);
                }
                if let Some(battery) = read_input_usage(handle, preparsed, caps.InputReportByteLength, page, usage) {
                    return Some(battery);
                }
            }
            None
        })();
        unsafe { HidD_FreePreparsedData(preparsed) };
        result
    })
}

fn value_caps(preparsed: PHIDP_PREPARSED_DATA, report_type: windows::Win32::Devices::HumanInterfaceDevice::HIDP_REPORT_TYPE, count: u16) -> Vec<HIDP_VALUE_CAPS> {
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

fn find_caps<'a>(caps: &'a [HIDP_VALUE_CAPS], page: u16, usage: u16) -> Option<&'a HIDP_VALUE_CAPS> {
    caps.iter().find(|cap| {
        if cap.UsagePage != page {
            return false;
        }
        let actual = if cap.IsRange {
            unsafe { cap.Anonymous.Range.UsageMin == usage && cap.Anonymous.Range.UsageMax >= usage }
        } else {
            unsafe { cap.Anonymous.NotRange.Usage == usage }
        };
        actual && cap.BitSize > 0 && cap.BitSize <= 32
    })
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
        HidD_GetFeature(handle, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32)
    };
    if !ok {
        return None;
    }
    extract_usage(preparsed, HidP_Feature, &buffer, cap)
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
        if let Some(battery) = extract_usage(preparsed, HidP_Input, &report, cap) {
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
    report_type: windows::Win32::Devices::HumanInterfaceDevice::HIDP_REPORT_TYPE,
    report: &[u8],
    cap: &HIDP_VALUE_CAPS,
) -> Option<StandardBattery> {
    let usage = if cap.IsRange {
        unsafe { cap.Anonymous.Range.UsageMin }
    } else {
        unsafe { cap.Anonymous.NotRange.Usage }
    };
    let mut value = 0u32;
    let status = unsafe {
        HidP_GetUsageValue(
            report_type,
            cap.UsagePage,
            None,
            usage,
            &mut value,
            preparsed,
            report,
        )
    };
    if status != HIDP_STATUS_SUCCESS {
        return None;
    }
    let min = cap.LogicalMin;
    let max = cap.LogicalMax;
    if max <= min {
        return None;
    }
    let percent = ((value as i64 - min as i64) * 100 / (max as i64 - min as i64)).clamp(0, 100) as u8;
    if percent == 0 && value as i32 == min {
        // A zero reading is usually "not reported yet" rather than empty.
        return None;
    }
    Some(StandardBattery {
        level: percent,
        charging: None,
        usage_page: cap.UsagePage,
        usage,
        report_id: cap.ReportID,
        raw: value,
        logical_min: min,
        logical_max: max,
    })
}

pub fn standard_snapshot(info: &HidInterface, battery: &StandardBattery, now: u64) -> DeviceSnapshot {
    let kind = DeviceKind::infer_from_name(&info.product);
    let transport = transport_for(&info.path, info.vendor_id);
    DeviceSnapshot::new(
        DeviceId::new(
            "hid",
            &info
                .container_id
                .clone()
                .unwrap_or_else(|| format!("{:04X}:{:04X}:{}", info.vendor_id, info.product_id, short_hash(&info.path))),
        ),
        info.label(),
        kind,
        transport,
        BatterySource::new(
            "标准 HID 电量",
            Confidence::OsReported,
            format!(
                "usage {:04X}:{:04X}, report {:#04X}, raw {} / {}",
                battery.usage_page, battery.usage, battery.report_id, battery.raw, battery.logical_max
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
    let lower = path.to_ascii_lowercase();
    if lower.contains("bthle") {
        Transport::BluetoothLe
    } else if lower.contains("bthenum") {
        Transport::BluetoothClassic
    } else if is_wireless_vid(vendor_id) {
        Transport::Usb24g
    } else {
        Transport::Usb
    }
}

fn is_wireless_vid(vendor_id: u16) -> bool {
    matches!(
        vendor_id,
        0x046D | 0x1532 | 0x3554 | 0x373B | 0x3710 | 0x3770 | 0x3057 | 0x3367 | 0x1915
            | 0x3654 | 0x1038 | 0x03F0 | 0x1B1C | 0x0B05 | 0x3434 | 0x388D | 0x1E71
            | 0x5253 | 0x3329 | 0x0ECB | 0x28DE
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

fn devnode_property(devinst: u32, key: &windows::Win32::Foundation::DEVPROPKEY, expected: windows::Win32::Devices::Properties::DEVPROPTYPE) -> Option<Vec<u8>> {
    let mut property_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
    let mut size = 0u32;
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            devinst,
            key,
            &mut property_type,
            None,
            &mut size,
            0,
        )
    };
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
    let Some(bytes) = devnode_property(devinst, &DEVPKEY_Device_HardwareIds, DEVPROP_TYPE_STRING_LIST) else {
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
