/// Extract the 12-hex-digit Bluetooth address from a Windows instance id such as
/// BTHENUM\Dev_001122AABBCC\7&123456&0&00000000.
pub fn bluetooth_address(instance_id: &str) -> Option<String> {
    let upper = instance_id.to_ascii_uppercase();
    let idx = upper.find("DEV_")?;
    let rest = &upper[idx + 4..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() == 12 { Some(hex) } else { None }
}

/// Remove Windows' duplicated Bluetooth endpoint suffixes so one headset is one row.
pub fn clean_bluetooth_name(name: &str) -> String {
    let suffixes = [
        " Hands-Free AG Audio",
        " Hands-Free AG",
        " Avrcp Transport",
        " Stereo",
        " (LE)",
        " LE",
    ];
    let mut out = name.trim().to_owned();
    for suffix in suffixes {
        if out.to_lowercase().ends_with(&suffix.to_lowercase()) {
            out.truncate(out.len() - suffix.len());
            out = out.trim().to_owned();
        }
    }
    out
}

pub fn bluetooth_id(instance_id: &str) -> Option<String> {
    bluetooth_address(instance_id).map(|addr| format!("bt:{}", addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bluetooth_address() {
        let id = r"BTHENUM\Dev_001122AABBCC\7&123456&0&00000000";
        assert_eq!(bluetooth_address(id).as_deref(), Some("001122AABBCC"));
        assert_eq!(bluetooth_id(id).as_deref(), Some("bt:001122AABBCC"));
        assert!(bluetooth_address(r"USB\VID_1234&PID_5678").is_none());
    }

    #[test]
    fn cleans_duplicate_names() {
        assert_eq!(
            clean_bluetooth_name("WH-1000XM5 Hands-Free AG Audio"),
            "WH-1000XM5"
        );
        assert_eq!(clean_bluetooth_name("AirPods Pro (LE)"), "AirPods Pro");
        assert_eq!(
            clean_bluetooth_name("Xbox Wireless Controller"),
            "Xbox Wireless Controller"
        );
    }
}
