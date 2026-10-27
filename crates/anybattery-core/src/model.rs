use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn new(kind: &str, key: &str) -> Self {
        Self(format!("{}:{}", kind, key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DeviceId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Mouse,
    Keyboard,
    Headset,
    Earbuds,
    Speaker,
    Controller,
    Pen,
    Unknown,
}

impl DeviceKind {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Mouse => "鼠标",
            Self::Keyboard => "键盘",
            Self::Headset => "耳机",
            Self::Earbuds => "耳塞",
            Self::Speaker => "音箱",
            Self::Controller => "手柄",
            Self::Pen => "触控笔",
            Self::Unknown => "设备",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Mouse => "Mouse",
            Self::Keyboard => "Keyboard",
            Self::Headset => "Headset",
            Self::Earbuds => "Earbuds",
            Self::Speaker => "Speaker",
            Self::Controller => "Controller",
            Self::Pen => "Pen",
            Self::Unknown => "Device",
        }
    }

    pub fn infer_from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
        if has(&[
            "mouse",
            "鼠标",
            "viper",
            "deathadder",
            "basilisk",
            "naga",
            "orochi",
        ]) {
            Self::Mouse
        } else if has(&["keyboard", "键盘", "blackwidow", "huntsman", "keychron"]) {
            Self::Keyboard
        } else if has(&[
            "headset",
            "耳机",
            "headphone",
            "barracuda",
            "blackshark",
            "wh-1000",
            "airpods",
        ]) {
            Self::Headset
        } else if has(&["earbud", "buds", "耳塞", "airpods"]) {
            Self::Earbuds
        } else if has(&["speaker", "音箱", "soundbar"]) {
            Self::Speaker
        } else if has(&[
            "controller",
            "gamepad",
            "手柄",
            "xbox",
            "dualsense",
            "dualshock",
            "joy-con",
            "steam controller",
        ]) {
            Self::Controller
        } else if has(&["pen", "stylus", "触控笔"]) {
            Self::Pen
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    BluetoothClassic,
    BluetoothLe,
    Usb24g,
    Usb,
    Unknown,
}

impl Transport {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::BluetoothClassic | Self::BluetoothLe => "蓝牙",
            Self::Usb24g => "2.4G",
            Self::Usb => "USB",
            Self::Unknown => "未知连接",
        }
    }

    pub fn is_bluetooth(self) -> bool {
        matches!(self, Self::BluetoothClassic | Self::BluetoothLe)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseLevel {
    Low,
    Medium,
    Full,
}

impl CoarseLevel {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::Full => "满",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatteryReading {
    Percent { level: u8, charging: Option<bool> },
    Coarse { level: CoarseLevel },
    Unknown,
}

impl BatteryReading {
    pub fn percent(level: u8, charging: Option<bool>) -> Self {
        Self::Percent {
            level: level.min(100),
            charging,
        }
    }

    pub fn level_percent(self) -> Option<u8> {
        match self {
            Self::Percent { level, .. } => Some(level),
            Self::Coarse { .. } | Self::Unknown => None,
        }
    }

    pub fn charging(self) -> Option<bool> {
        match self {
            Self::Percent { charging, .. } => charging,
            Self::Coarse { .. } | Self::Unknown => None,
        }
    }

    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryStatus {
    Available,
    Charging,
    Full,
    Asleep,
    Stale,
    Unsupported,
    Error,
    Unknown,
}

impl BatteryStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Available => "可用",
            Self::Charging => "充电中",
            Self::Full => "已充满",
            Self::Asleep => "设备休眠",
            Self::Stale => "上次电量",
            Self::Unsupported => "未找到电量协议",
            Self::Error => "读取失败",
            Self::Unknown => "未知",
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Available | Self::Charging | Self::Full)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Verified,
    Community,
    Unverified,
    OsReported,
    Coarse,
    Cached,
}

impl Confidence {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Verified => "已验证",
            Self::Community => "社区协议",
            Self::Unverified => "未验证",
            Self::OsReported => "系统报告",
            Self::Coarse => "粗略电量",
            Self::Cached => "缓存",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatterySource {
    pub provider: String,
    pub verification: Confidence,
    pub detail: String,
}

impl BatterySource {
    pub fn new(
        provider: impl Into<String>,
        verification: Confidence,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            verification,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub transport: Transport,
    pub battery: BatteryReading,
    pub status: BatteryStatus,
    pub source: BatterySource,
    pub last_seen_unix: Option<u64>,
    pub updated_at_unix: Option<u64>,
    pub detail: String,
    pub path: Option<String>,
}

impl DeviceSnapshot {
    pub fn new(
        id: DeviceId,
        name: impl Into<String>,
        kind: DeviceKind,
        transport: Transport,
        source: BatterySource,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            transport,
            battery: BatteryReading::Unknown,
            status: BatteryStatus::Unknown,
            source,
            last_seen_unix: None,
            updated_at_unix: None,
            detail: String::new(),
            path: None,
        }
    }

    pub fn with_battery(mut self, reading: BatteryReading, now: u64) -> Self {
        self.battery = reading;
        self.status = match reading {
            BatteryReading::Percent {
                level: 100,
                charging: Some(true),
            } => BatteryStatus::Full,
            BatteryReading::Percent {
                charging: Some(true),
                ..
            } => BatteryStatus::Charging,
            BatteryReading::Percent { .. } => BatteryStatus::Available,
            BatteryReading::Coarse { .. } => BatteryStatus::Available,
            BatteryReading::Unknown => BatteryStatus::Unsupported,
        };
        self.last_seen_unix = Some(now);
        self.updated_at_unix = Some(now);
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn percent(&self) -> Option<u8> {
        self.battery.level_percent()
    }

    pub fn sort_key(&self) -> (u8, u8, String) {
        let group = match self.status {
            BatteryStatus::Charging | BatteryStatus::Full => 2,
            BatteryStatus::Available => 0,
            BatteryStatus::Asleep | BatteryStatus::Stale => 1,
            BatteryStatus::Unsupported | BatteryStatus::Error | BatteryStatus::Unknown => 3,
        };
        let level = self.percent().unwrap_or(255);
        (group, level, self.name.to_lowercase())
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn age_text(updated_at_unix: Option<u64>, now: u64) -> String {
    let Some(updated) = updated_at_unix else {
        return "从未更新".to_owned();
    };
    let secs = now.saturating_sub(updated);
    match secs {
        0..=9 => "刚刚".to_owned(),
        10..=59 => format!("{} 秒前", secs),
        60..=3599 => format!("{} 分钟前", secs / 60),
        3600..=86399 => format!("{} 小时前", secs / 3600),
        86400..=172_799 => "昨天".to_owned(),
        _ => format!("{} 天前", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_percent_and_infers_status() {
        let source = BatterySource::new("test", Confidence::Verified, "fixture");
        let device = DeviceSnapshot::new(
            DeviceId::new("hid", "abc"),
            "Razer Viper",
            DeviceKind::Mouse,
            Transport::Usb24g,
            source,
        )
        .with_battery(BatteryReading::percent(120, Some(true)), 1_700_000_000);
        assert_eq!(device.percent(), Some(100));
        assert_eq!(device.status, BatteryStatus::Full);
    }

    #[test]
    fn age_text_is_human_readable() {
        assert_eq!(age_text(Some(100), 100), "刚刚");
        assert_eq!(age_text(Some(100), 160), "1 分钟前");
        assert_eq!(age_text(Some(100), 100 + 7200), "2 小时前");
        assert_eq!(age_text(None, 100), "从未更新");
    }

    #[test]
    fn infers_device_kind() {
        assert_eq!(
            DeviceKind::infer_from_name("Razer Viper V3 Pro"),
            DeviceKind::Mouse
        );
        assert_eq!(
            DeviceKind::infer_from_name("Xbox Wireless Controller"),
            DeviceKind::Controller
        );
        assert_eq!(
            DeviceKind::infer_from_name("Sony WH-1000XM5"),
            DeviceKind::Headset
        );
    }
}
