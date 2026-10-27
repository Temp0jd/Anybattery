use anybattery_core::dedup::clean_bluetooth_name;
use anybattery_core::{
    BatteryReading, BatterySource, BatteryStatus, Confidence, DeviceId, DeviceKind, DeviceSnapshot,
    Transport,
};
use std::mem::size_of;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW,
    CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPKEY_DeviceContainer_IsConnected,
    DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING,
};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, PCWSTR};

pub const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

#[derive(Clone, Debug)]
pub struct BluetoothDevice {
    pub instance_id: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub container_id: Option<String>,
    pub enumerator: &'static str,
    /// Stable identity for the card. Root nodes keep `bt:<address>`; service-only
    /// nodes fall back to their device container id.
    pub id: String,
    /// Lets `merge_bluetooth` prefer a real root name over a service name and a
    /// service name over the generated fallback without inspecting the text.
    name_quality: NameQuality,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum NameQuality {
    Placeholder,
    Service,
    Root,
}

#[derive(Clone, Debug)]
struct BluetoothNode {
    instance_id: String,
    address: Option<String>,
    name: String,
    /// True only for the generated "蓝牙设备 XXXX" fallback name.
    name_is_placeholder: bool,
    connected: bool,
    battery: Option<u8>,
    container_id: Option<String>,
    enumerator: &'static str,
}

pub fn enumerate() -> Vec<BluetoothDevice> {
    associate(enumerate_nodes())
}

fn enumerate_nodes() -> Vec<BluetoothNode> {
    let mut out = Vec::new();
    for enumerator in ["BTHENUM", "BTHLE"] {
        for instance_id in enumerate_instance_ids(enumerator) {
            let Some(devinst) = locate(&instance_id) else {
                continue;
            };
            let address = bluetooth_address(&instance_id);
            let container_id =
                usable_container_id(guid_property(devinst, &DEVPKEY_Device_ContainerId));
            // A node without either a DEV_ address or a usable container cannot be
            // identified or safely associated, so leave it out of the UI.
            if address.is_none() && container_id.is_none() {
                continue;
            }
            let mut name_is_placeholder = false;
            let name = friendly_name(devinst)
                .filter(|name| !name.trim().is_empty())
                .or_else(|| string_property(devinst, &DEVPKEY_Device_FriendlyName))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| {
                    name_is_placeholder = true;
                    match &address {
                        Some(address) => {
                            format!("蓝牙设备 {}", &address[address.len().saturating_sub(4)..])
                        }
                        None => "蓝牙设备".to_owned(),
                    }
                });
            let connected =
                bool_property(devinst, &DEVPKEY_DeviceContainer_IsConnected).unwrap_or(true);
            let battery = valid_battery(byte_property(devinst, &DEVPKEY_BLUETOOTH_BATTERY));
            out.push(BluetoothNode {
                instance_id,
                address,
                name,
                name_is_placeholder,
                connected,
                battery,
                container_id,
                enumerator,
            });
        }
    }
    out
}

fn valid_battery(level: Option<u8>) -> Option<u8> {
    level.filter(|level| (1..=100).contains(level))
}

/// Zero, empty and local-device container ids are not real device identities.
/// Treating them as keys would merge unrelated Bluetooth nodes into one card.
/// Normalize before comparing so braces, whitespace and case cannot split or
/// merge groups differently between nodes.
fn usable_container_id(value: Option<String>) -> Option<String> {
    const NULL_CONTAINER: &str = "00000000-0000-0000-ffff-ffffffffffff";
    let id = value?
        .trim()
        .trim_matches(|c: char| c.is_whitespace() || c == '{' || c == '}')
        .to_ascii_lowercase();
    if id.is_empty() || id == NULL_CONTAINER {
        return None;
    }
    if id.chars().all(|c| c == '0' || c == '-') {
        return None;
    }
    Some(id)
}

/// Associate service nodes (for example HFP) with the root card that shares their
/// container id. The root's instance id, address-derived id, name, path and
/// transport win; service nodes only fill a missing battery or fallback name.
fn associate(nodes: Vec<BluetoothNode>) -> Vec<BluetoothDevice> {
    let mut sorted = nodes;
    sorted.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

    let mut by_container: Vec<(String, Vec<BluetoothNode>)> = Vec::new();
    let mut loose = Vec::new();
    for node in sorted {
        let Some(container) = node.container_id.clone() else {
            loose.push(node);
            continue;
        };
        if let Some((_, group)) = by_container.iter_mut().find(|(id, _)| *id == container) {
            group.push(node);
        } else {
            by_container.push((container, vec![node]));
        }
    }

    let mut out = Vec::new();
    for (_, group) in by_container {
        let mut roots = Vec::new();
        let mut services = Vec::new();
        for node in group {
            if node.address.is_some() {
                roots.push(node);
            } else {
                services.push(node);
            }
        }
        let mut roots = merge_by_address(roots);
        match roots.len() {
            0 => {
                if !services.is_empty() {
                    out.push(canonical_card(services));
                }
            }
            1 => {
                let mut root = roots.pop().expect("one root");
                for service in &services {
                    adopt(&mut root, service);
                }
                out.push(root);
            }
            _ => {
                // Dual-mode headsets can expose a classic root and an LE root
                // with different addresses under one container. When exactly one
                // classic root exists, the HFP service still belongs to it;
                // multiple classic roots remain ambiguous and get no service data.
                let classic_roots: Vec<usize> = roots
                    .iter()
                    .enumerate()
                    .filter(|(_, root)| is_classic_device_root(root))
                    .map(|(index, _)| index)
                    .collect();
                if let [index] = classic_roots.as_slice() {
                    for service in &services {
                        adopt(&mut roots[*index], service);
                    }
                }
                out.append(&mut roots);
            }
        }
    }
    out.extend(merge_by_address(loose));
    out
}

fn merge_by_address(nodes: Vec<BluetoothNode>) -> Vec<BluetoothDevice> {
    let mut groups: Vec<(String, Vec<BluetoothNode>)> = Vec::new();
    for node in nodes {
        let Some(address) = node.address.clone() else {
            continue;
        };
        if let Some((_, group)) = groups.iter_mut().find(|(id, _)| *id == address) {
            group.push(node);
        } else {
            groups.push((address, vec![node]));
        }
    }
    groups
        .into_iter()
        .map(|(_, group)| canonical_card(group))
        .collect()
}

fn canonical_card(mut nodes: Vec<BluetoothNode>) -> BluetoothDevice {
    // Do not trust instance-id lexical order: a GATT service node such as
    // `...&0000180F` sorts before a real `...&<address>` root. Rank the real
    // BluetoothDevice/BTHLE root first so name, path and id come from it.
    nodes.sort_by(|a, b| {
        root_rank(b)
            .cmp(&root_rank(a))
            .then_with(|| name_quality(b).cmp(&name_quality(a)))
            .then_with(|| a.instance_id.cmp(&b.instance_id))
    });
    let root = nodes.remove(0);
    let connected = if root.address.is_some() {
        root.connected
    } else {
        // Service-only cards have no root to be authoritative; any live service
        // node is enough to mark the card connected.
        nodes
            .iter()
            .map(|node| node.connected)
            .fold(root.connected, |connected, node_connected| {
                connected || node_connected
            })
    };
    let quality = name_quality(&root);
    let mut device = BluetoothDevice {
        id: node_id(&root),
        instance_id: root.instance_id,
        name: root.name,
        connected,
        battery: root.battery,
        container_id: root.container_id,
        enumerator: root.enumerator,
        name_quality: quality,
    };
    for node in &nodes {
        adopt(&mut device, node);
    }
    device
}

fn adopt(device: &mut BluetoothDevice, node: &BluetoothNode) {
    // A service node only fills gaps. Its stale `connected`/battery values must
    // not turn a disconnected root into a connected card.
    if device.battery.is_none() && node.connected {
        device.battery = node.battery;
    }
    let quality = name_quality(node);
    if quality > device.name_quality {
        device.name = node.name.clone();
        device.name_quality = quality;
    }
    if device.container_id.is_none() {
        device.container_id = node.container_id.clone();
    }
}

fn is_device_root_instance(instance_id: &str, address: &str) -> bool {
    let upper = instance_id.to_ascii_uppercase();
    let address = address.to_ascii_uppercase();
    upper.ends_with(&format!("BLUETOOTHDEVICE_{address}"))
        || upper.ends_with(&format!("&{address}"))
}

/// Real roots outrank address-bearing service/GATT nodes. The classic
/// `BluetoothDevice_<address>` root wins when a classic and LE root collide.
fn root_rank(node: &BluetoothNode) -> u8 {
    let Some(address) = node.address.as_deref() else {
        return 0;
    };
    if !is_device_root_instance(&node.instance_id, address) {
        return 1;
    }
    if node.enumerator == "BTHENUM" { 3 } else { 2 }
}

fn name_quality(node: &BluetoothNode) -> NameQuality {
    if node.name_is_placeholder {
        NameQuality::Placeholder
    } else if node
        .address
        .as_deref()
        .is_some_and(|address| is_device_root_instance(&node.instance_id, address))
    {
        NameQuality::Root
    } else {
        NameQuality::Service
    }
}

fn is_classic_device_root(device: &BluetoothDevice) -> bool {
    device.enumerator == "BTHENUM"
        && bluetooth_address(&device.instance_id)
            .is_some_and(|address| is_device_root_instance(&device.instance_id, &address))
}

fn node_id(node: &BluetoothNode) -> String {
    node.address
        .as_ref()
        .map(|address| format!("bt:{}", address))
        .or_else(|| {
            node.container_id
                .as_ref()
                .map(|id| format!("bt:container:{}", id))
        })
        .unwrap_or_else(|| format!("bt:{}", node.instance_id))
}

fn enumerate_instance_ids(enumerator: &str) -> Vec<String> {
    let filter_wide = super::hid::to_wide(enumerator);
    let flags = CM_GETIDLIST_FILTER_PRESENT | CM_GETIDLIST_FILTER_ENUMERATOR;
    let mut len = 0u32;
    let status =
        unsafe { CM_Get_Device_ID_List_SizeW(&mut len, PCWSTR(filter_wide.as_ptr()), flags) };
    if status != CR_SUCCESS || len == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    let status =
        unsafe { CM_Get_Device_ID_ListW(PCWSTR(filter_wide.as_ptr()), &mut buffer, flags) };
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
        CM_Locate_DevNodeW(
            &mut devinst,
            PCWSTR(wide.as_ptr()),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };
    (status == CR_SUCCESS).then_some(devinst)
}

fn bluetooth_address(instance_id: &str) -> Option<String> {
    anybattery_core::dedup::bluetooth_address(instance_id)
}

fn friendly_name(devinst: u32) -> Option<String> {
    string_property(devinst, &DEVPKEY_Device_FriendlyName)
}

fn property_bytes(
    devinst: u32,
    key: &DEVPROPKEY,
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
    let name = if clean.is_empty() {
        dev.name.clone()
    } else {
        clean
    };
    let id = dev.id.clone();
    let mut snapshot = DeviceSnapshot::new(
        DeviceId(id),
        name.clone(),
        DeviceKind::infer_from_name(&name),
        if dev.enumerator == "BTHLE" {
            Transport::BluetoothLe
        } else {
            Transport::BluetoothClassic
        },
        BatterySource::new(
            "Windows 蓝牙属性",
            Confidence::OsReported,
            format!("{} · {}", dev.enumerator, dev.instance_id),
        ),
    )
    .with_detail(if dev.connected {
        "已连接"
    } else {
        "未连接"
    })
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
    // Real root cards first so a duplicate service node cannot donate its name
    // or path when both carry the same stable id.
    devices.sort_by(|a, b| {
        b.name_quality
            .cmp(&a.name_quality)
            .then_with(|| a.instance_id.cmp(&b.instance_id))
    });
    let mut out: Vec<BluetoothDevice> = Vec::new();
    for device in devices {
        if let Some(existing) = out.iter_mut().find(|d| d.id == device.id) {
            if existing.battery.is_none() && device.battery.is_some() {
                existing.battery = device.battery;
            }
            if device.name_quality > existing.name_quality {
                existing.name = device.name.clone();
                existing.name_quality = device.name_quality;
            }
            existing.connected |= device.connected;
            if existing.container_id.is_none() {
                existing.container_id = device.container_id.clone();
            }
        } else {
            out.push(device);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_CONTAINER: &str = "7315D72A-1231-5D35-80BF-D3735A389609";
    const ZERO_CONTAINER: &str = "AABBCCDD-1111-2222-3333-444455556666";
    const ZERO_GUID: &str = "00000000-0000-0000-0000-000000000000";
    const NULL_CONTAINER: &str = "00000000-0000-0000-FFFF-FFFFFFFFFFFF";
    const OPPO_ROOT: &str = r"BTHENUM\DEV_407218C8A7BC\9&1e244907&0&BluetoothDevice_407218C8A7BC";
    const OPPO_HANDS_FREE: &str = r"BTHENUM\{0000111E-0000-1000-8000-00805F9B34FB}_VID&000102B0_PID&0000\9&1e244907&0&0000111E";
    const LE_ROOT: &str = r"BTHLE\DEV_CC3D601F1B0C\9&76eff9f&0&cc3d601f1b0c";
    const LE_GATT_BATTERY_SERVICE: &str = r"BTHLE\DEV_CC3D601F1B0C\9&76eff9f&0&0000180F";
    const LE_GATT_BATTERY_LEVEL: &str = r"BTHLE\DEV_CC3D601F1B0C\9&76eff9f&0&00002A19";

    fn node(
        instance_id: &str,
        name: &str,
        container: Option<&str>,
        battery: Option<u8>,
    ) -> BluetoothNode {
        BluetoothNode {
            instance_id: instance_id.to_owned(),
            address: bluetooth_address(instance_id),
            name: name.to_owned(),
            name_is_placeholder: false,
            connected: true,
            battery,
            container_id: usable_container_id(container.map(str::to_owned)),
            enumerator: if instance_id.starts_with("BTHLE") {
                "BTHLE"
            } else {
                "BTHENUM"
            },
        }
    }

    fn placeholder_node(
        instance_id: &str,
        container: Option<&str>,
        battery: Option<u8>,
    ) -> BluetoothNode {
        let mut node = node(instance_id, "蓝牙设备 AABB", container, battery);
        node.name_is_placeholder = true;
        node
    }

    fn disconnected(mut node: BluetoothNode) -> BluetoothNode {
        node.connected = false;
        node
    }

    fn oppo_root() -> BluetoothNode {
        node(OPPO_ROOT, "OPPO Enco X3", Some(ROOT_CONTAINER), None)
    }

    fn oppo_hands_free() -> BluetoothNode {
        node(
            OPPO_HANDS_FREE,
            "OPPO Enco X3 Hands-Free AG",
            Some(ROOT_CONTAINER),
            Some(90),
        )
    }

    #[test]
    fn root_and_hands_free_service_share_one_card() {
        let devices = associate(vec![oppo_hands_free(), oppo_root()]);
        assert_eq!(devices.len(), 1);
        let device = &devices[0];
        assert_eq!(device.instance_id, OPPO_ROOT);
        assert_eq!(device.id, "bt:407218C8A7BC");
        assert_eq!(device.name, "OPPO Enco X3");
        assert_eq!(device.battery, Some(90));
        assert_eq!(device.enumerator, "BTHENUM");
    }

    #[test]
    fn different_containers_do_not_merge() {
        let other_root = node(
            r"BTHENUM\DEV_001122AABBCC\7&42&0&BluetoothDevice_001122AABBCC",
            "Other Headset",
            Some(ZERO_CONTAINER),
            None,
        );
        let other_service = node(
            r"BTHENUM\{0000111E-0000-1000-8000-00805F9B34FB}_VID&000102B0_PID&0000\7&42&0&0000111E",
            "Other Headset Hands-Free AG",
            Some(ZERO_CONTAINER),
            Some(60),
        );
        let devices = associate(vec![
            oppo_root(),
            oppo_hands_free(),
            other_root,
            other_service,
        ]);
        assert_eq!(devices.len(), 2);
        assert_eq!(
            devices
                .iter()
                .find(|d| d.id == "bt:407218C8A7BC")
                .map(|d| d.battery),
            Some(Some(90))
        );
        assert_eq!(
            devices
                .iter()
                .find(|d| d.id == "bt:001122AABBCC")
                .map(|d| d.battery),
            Some(Some(60))
        );
    }

    #[test]
    fn bluetooth_le_device_stays_independent() {
        let zero = node(
            r"BTHLE\DEV_CC3D601F1B0C\9&76eff9f&0&cc3d601f1b0c",
            "Zero 1",
            Some(ZERO_CONTAINER),
            Some(46),
        );
        let devices = associate(vec![oppo_root(), oppo_hands_free(), zero]);
        assert_eq!(devices.len(), 2);
        let zero = devices.iter().find(|d| d.id == "bt:CC3D601F1B0C").unwrap();
        let snapshot = snapshot(zero, 0);
        assert_eq!(snapshot.name, "Zero 1");
        assert_eq!(snapshot.transport, Transport::BluetoothLe);
        assert_eq!(snapshot.battery.level_percent(), Some(46));
    }

    #[test]
    fn zero_and_empty_containers_do_not_merge() {
        assert_eq!(usable_container_id(Some(ZERO_GUID.to_owned())), None);
        assert_eq!(usable_container_id(Some(NULL_CONTAINER.to_owned())), None);
        assert_eq!(usable_container_id(Some("  ".to_owned())), None);

        let root = node(OPPO_ROOT, "OPPO Enco X3", Some(ZERO_GUID), None);
        let service = node(
            OPPO_HANDS_FREE,
            "OPPO Enco X3 Hands-Free AG",
            Some(ZERO_GUID),
            Some(90),
        );
        let devices = associate(vec![root, service]);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "bt:407218C8A7BC");
        assert_eq!(devices[0].battery, None);

        assert!(associate(vec![
            node(OPPO_HANDS_FREE, "OPPO Enco X3 Hands-Free AG", Some(ZERO_GUID), Some(90)),
            node(
                r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&000102B0_PID&0000\9&1&0&0000110B",
                "OPPO Enco X3 Stereo",
                Some(ZERO_GUID),
                Some(70),
            ),
        ])
        .is_empty());

        assert!(associate(vec![node(
            r"BTHENUM\{00001101-0000-1000-8000-00805F9B34FB}_LOCALMFG&0000\9&186e93ee&0&000000000000_00000000",
            "蓝牙链接上的标准串行 (COM4)",
            Some(NULL_CONTAINER),
            None,
        )])
        .is_empty());
    }

    #[test]
    fn service_only_device_uses_container_fallback_id() {
        let devices = associate(vec![oppo_hands_free()]);
        assert_eq!(devices.len(), 1);
        let device = &devices[0];
        assert_eq!(
            device.id,
            format!("bt:container:{}", ROOT_CONTAINER.to_ascii_lowercase())
        );
        assert_eq!(device.battery, Some(90));
        let snapshot = snapshot(device, 0);
        assert_eq!(snapshot.name, "OPPO Enco X3");
        assert_eq!(snapshot.transport, Transport::BluetoothClassic);
        assert_eq!(snapshot.battery.level_percent(), Some(90));
    }

    #[test]
    fn classic_and_le_roots_with_different_addresses_keep_hfp_battery() {
        let le_root = node(LE_ROOT, "OPPO Enco X3 LE", Some(ROOT_CONTAINER), Some(40));
        let devices = associate(vec![oppo_hands_free(), le_root, oppo_root()]);
        assert_eq!(devices.len(), 2);

        let classic = devices.iter().find(|d| d.id == "bt:407218C8A7BC").unwrap();
        assert_eq!(classic.instance_id, OPPO_ROOT);
        assert_eq!(classic.name, "OPPO Enco X3");
        assert_eq!(classic.battery, Some(90));
        assert_eq!(classic.enumerator, "BTHENUM");

        let le = devices.iter().find(|d| d.id == "bt:CC3D601F1B0C").unwrap();
        assert_eq!(le.instance_id, LE_ROOT);
        assert_eq!(le.name, "OPPO Enco X3 LE");
        assert_eq!(le.battery, Some(40));
        assert_eq!(le.enumerator, "BTHLE");
    }

    #[test]
    fn multiple_classic_roots_drop_service_association() {
        let other_root = node(
            r"BTHENUM\DEV_001122AABBCC\7&42&0&BluetoothDevice_001122AABBCC",
            "Other Headset",
            Some(ROOT_CONTAINER),
            None,
        );
        let devices = associate(vec![oppo_root(), other_root, oppo_hands_free()]);
        assert_eq!(devices.len(), 2);
        assert!(devices.iter().all(|device| device.battery.is_none()));
        assert!(
            devices
                .iter()
                .all(|device| !device.id.starts_with("bt:container:"))
        );
    }

    #[test]
    fn bthle_gatt_service_nodes_keep_the_device_root_identity() {
        let devices = associate(vec![
            node(
                LE_GATT_BATTERY_SERVICE,
                "Zero Battery Service",
                Some(ZERO_CONTAINER),
                Some(99),
            ),
            node(
                LE_GATT_BATTERY_LEVEL,
                "Zero Battery Level",
                Some(ZERO_CONTAINER),
                Some(88),
            ),
            node(LE_ROOT, "Zero 1", Some(ZERO_CONTAINER), Some(46)),
        ]);
        assert_eq!(devices.len(), 1);
        let device = &devices[0];
        assert_eq!(device.instance_id, LE_ROOT);
        assert_eq!(device.id, "bt:CC3D601F1B0C");
        assert_eq!(device.name, "Zero 1");
        assert_eq!(device.battery, Some(46));
        assert_eq!(device.enumerator, "BTHLE");
    }

    #[test]
    fn container_ids_are_trimmed_braced_and_lowercased() {
        let expected = ROOT_CONTAINER.to_ascii_lowercase();
        assert_eq!(
            usable_container_id(Some(format!(
                "  {{{}}}  ",
                ROOT_CONTAINER.to_ascii_lowercase()
            ))),
            Some(expected.clone())
        );
        assert_eq!(
            usable_container_id(Some(format!(
                " {{{}}} ",
                NULL_CONTAINER.to_ascii_lowercase()
            ))),
            None
        );
        assert_eq!(usable_container_id(Some(format!("{{{ZERO_GUID}}}"))), None);
        assert_eq!(usable_container_id(Some("   ".to_owned())), None);

        let braced = format!("  {{{}}}  ", ROOT_CONTAINER.to_ascii_lowercase());
        let spaced = format!(" {} ", ROOT_CONTAINER);
        let devices = associate(vec![
            node(OPPO_ROOT, "OPPO Enco X3", Some(&braced), None),
            node(
                OPPO_HANDS_FREE,
                "OPPO Enco X3 Hands-Free AG",
                Some(&spaced),
                Some(90),
            ),
        ]);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].battery, Some(90));
        assert_eq!(devices[0].container_id.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn placeholder_root_name_upgrades_from_service_name() {
        let devices = associate(vec![
            placeholder_node(OPPO_ROOT, Some(ROOT_CONTAINER), None),
            oppo_hands_free(),
        ]);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].instance_id, OPPO_ROOT);
        assert_eq!(devices[0].name, "OPPO Enco X3 Hands-Free AG");
        assert_eq!(devices[0].battery, Some(90));
    }

    #[test]
    fn merge_bluetooth_prefers_root_name_over_service_name() {
        let service = canonical_card(vec![node(
            LE_GATT_BATTERY_SERVICE,
            "Zero Battery Service",
            None,
            Some(99),
        )]);
        let root = canonical_card(vec![node(LE_ROOT, "Zero 1", None, None)]);
        let merged = merge_bluetooth(vec![service, root]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].instance_id, LE_ROOT);
        assert_eq!(merged[0].name, "Zero 1");
    }

    #[test]
    fn merge_bluetooth_placeholder_does_not_override_service_name() {
        let placeholder = canonical_card(vec![placeholder_node(LE_ROOT, None, None)]);
        let service = canonical_card(vec![node(
            LE_GATT_BATTERY_SERVICE,
            "Zero Battery Service",
            None,
            Some(99),
        )]);
        let merged = merge_bluetooth(vec![placeholder, service]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Zero Battery Service");
        assert_eq!(merged[0].battery, Some(99));
    }

    #[test]
    fn service_connection_does_not_flip_disconnected_root() {
        let devices = associate(vec![disconnected(oppo_root()), oppo_hands_free()]);
        assert_eq!(devices.len(), 1);
        assert!(!devices[0].connected);
        assert_eq!(devices[0].battery, Some(90));
    }

    #[test]
    fn battery_zero_and_out_of_range_are_rejected() {
        assert_eq!(valid_battery(Some(0)), None);
        assert_eq!(valid_battery(Some(101)), None);
        assert_eq!(valid_battery(Some(1)), Some(1));
        assert_eq!(valid_battery(Some(100)), Some(100));
    }
}
