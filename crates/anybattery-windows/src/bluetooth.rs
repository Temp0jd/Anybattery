use anybattery_core::dedup::{bluetooth_id, clean_bluetooth_name};
use anybattery_core::{
    now_unix, BatteryReading, BatterySource, BatteryStatus, Confidence, DeviceId, DeviceKind,
    DeviceSnapshot, Transport,
};
use std::mem::size_of;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW, CM_Get_DevNode_PropertyW,
    CM_Locate_DevNodeW, CR_BUFFER_SMALL, CR_SUCCESS, CM_GETIDLIST_FILTER_ENUMERATOR,
    CM_GETIDLIST_FILTER_PRESENT, CM_LOCATE_DEVNODE_NORMAL,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_DeviceContainer_IsConnected, DEVPKEY_Device_ContainerId,
    DEVPKEY_Device_FriendlyName, DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID,
    DEVPROP_TYPE_STRING,
};
use windows::Win32::Foundation::DEVPROPKEY;

pub const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

#[derive(Clone, Debug)]
pub struct BluetoothDevice {
    pub instance_id: String,
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub container_id: Option<String>,
    pub enumerator: &'static str,
}

pub fn enumerate() -> Vec<BluetoothDevice> {
    let mut out = Vec::new();
    for (enumerator, transport) in [("BTHENUM", Transport::BluetoothClassic), ("BTHLE", Transport::BluetoothLe)] {
        for instance_id in enumerate_instance_ids(enumerator) {
            let Some(devinst) = locate(&instance_id) else {
                continue;
            };
            let Some(address) = bluetooth_address(&instance_id) else {
                continue;
            };
            let name = friendly_name(devinst)
                .or_else(|| string_property(devinst, &DEVPKEY_Device_FriendlyName))
                .unwrap_or_else(|| format!("蓝牙设备 {}", &address[address.len().saturating_sub(4)..]));
            let connected = bool_property(devinst, &DEVPKEY_DeviceContainer_IsConnected).unwrap_or(true);
            let battery = byte_property(devinst, &DEVPKEY_BLUETOOTH_BATTERY).filter(|v| (1..=100).contains(v));
            let container_id = guid_property(devinst, &DEVPKEY_Device_ContainerId);
            out.push(BluetoothDevice {
                instance_id,
                address,
                name,
                connected,
                battery,
                container_id,
                enumerator,
            });
            let _ = transport;
        }
    }
    out
}

fn enumerate_instance_ids(enumerator: &str) -> Vec<String> {
    let filter_wide = super::hid::to_wide(enumerator);
    let flags = CM_GETIDLIST_FILTER_PRESENT | CM_GETIDLIST_FILTER_ENUMERATOR;
    let mut len = 0u32;
    let status = unsafe {
        CM_Get_Device_ID_List_SizeW(&mut len, PCWSTR(filter_wide.as_ptr()), flags)
    };
    if status != CR_SUCCESS || len == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    let status = unsafe {
        CM_Get_Device_ID_ListW(PCWSTR(filter_wide.as_ptr()), &mut buffer, flags)
    };
    if status != CR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 0..buffer.len() {
        if buffer[i] == 0 {
            if i == start {
                break;
            }
            out.push(String::from_utf16_lossy(&buffer[start..i]));
            start = i + 1;
        }
    }
    out
}

fn locate(instance_id: &str) -> Option<u32> {
    let wide = super::hid::to_wide(instance_id);
    let mut devinst = 0u32;
    let status = unsafe {
        CM_Locate_DevNodeW(&mut devinst, PCWSTR(wide.as_ptr()), CM_LOCATE_DEVNODE_NORMAL)
    };
    (status == CR_SUCCESS).then_some(devinst)
}

fn bluetooth_address(instance_id: &str) -> Option<String> {
    anybattery_core::dedup::bluetooth_address(instance_id)
}

fn friendly_name(devinst: u32) -> Option<String> {
    string_property(devinst, &DEVPKEY_Device_FriendlyName)
}

fn property_bytes(devinst: u32, key: &DEVPROPKEY, expected: windows::Win32::Devices::Properties::DEVPROPTYPE) -> Option<Vec<u8>> {
    let mut property_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
    let mut size = 0u32;
    let status = unsafe {
        CM_Get_DevNode_PropertyW(devinst, key, &mut property_type, None, &mut size, 0)
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

fn string_property(devinst: u32, key: &DEVPROPKEY) -> Option<String> {
    let bytes = property_bytes(devinst, key, DEVPROP_TYPE_STRING)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn byte_property(devinst: u32, key: &DEVPROPKEY) -> Option<u8> {
    let bytes = property_bytes(devinst, key, DEVPROP_TYPE_BYTE)?;
    bytes.first().copied()
}

fn bool_property(devinst: u32, key: &DEVPROPKEY) -> Option<bool> {
    let bytes = property_bytes(devinst, key, DEVPROP_TYPE_BOOLEAN)?;
    bytes.first().map(|v| *v != 0)
}

fn guid_property(devinst: u32, key: &DEVPROPKEY) -> Option<String> {
    let bytes = property_bytes(devinst, key, DEVPROP_TYPE_GUID)?;
    if bytes.len() < size_of::<GUID>() {
        return None;
    }
    let guid = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const GUID) };
    Some(super::hid::guid_string(guid))
}

pub fn snapshot(dev: &BluetoothDevice, now: u64) -> DeviceSnapshot {
    let clean = clean_bluetooth_name(&dev.name);
    let name = if clean.is_empty() { dev.name.clone() } else { clean };
    let id = bluetooth_id(&dev.instance_id)
        .unwrap_or_else(|| format!("bt:{}", dev.address));
    let mut snapshot = DeviceSnapshot::new(
        DeviceId(id),
        name.clone(),
        DeviceKind::infer_from_name(&name),
        if dev.enumerator == "BTHLE" { Transport::BluetoothLe } else { Transport::BluetoothClassic },
        BatterySource::new(
            "Windows 蓝牙属性",
            Confidence::OsReported,
            format!("{} · {}", dev.enumerator, dev.instance_id),
        ),
    )
    .with_detail(if dev.connected { "已连接" } else { "未连接" })
    .with_path(&dev.instance_id);

    if let Some(level) = dev.battery {
        snapshot = snapshot.with_battery(BatteryReading::percent(level, None), now);
    } else {
        snapshot.battery = BatteryReading::Unknown;
        snapshot.status = if dev.connected {
            BatteryStatus::Unsupported
        } else {
            BatteryStatus::Asleep
        };
        snapshot.last_seen_unix = Some(now);
    }
    snapshot
}

pub fn merge_bluetooth(mut devices: Vec<BluetoothDevice>) -> Vec<BluetoothDevice> {
    devices.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    let mut out: Vec<BluetoothDevice> = Vec::new();
    for device in devices {
        if let Some(existing) = out.iter_mut().find(|d| d.address == device.address) {
            if existing.battery.is_none() && device.battery.is_some() {
                existing.battery = device.battery;
            }
            if existing.name.len() < device.name.len() {
                existing.name = device.name.clone();
            }
            existing.connected |= device.connected;
            if existing.container_id.is_none() {
                existing.container_id = device.container_id.clone();
            }
        } else {
            out.push(device);
        }
    }
    let _ = now_unix();
    out
}
