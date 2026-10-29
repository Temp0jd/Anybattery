use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model::DeviceKind;

pub const MIN_POLL_SECS: u64 = 15;
pub const MAX_POLL_SECS: u64 = 1800;
pub const DEFAULT_POLL_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    ZhCn,
    En,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub poll_secs: u64,
    pub notify_low_battery: bool,
    pub low_threshold: u8,
    pub critical_threshold: u8,
    pub autostart: bool,
    pub show_unknown: bool,
    pub active_ble: bool,
    pub animations: bool,
    pub theme: Theme,
    pub language: Language,
    pub tray_per_device: bool,
    /// 用户手动指定的设备图标；键是 `DeviceSnapshot.id`，值覆盖自动识别结果。
    #[serde(default)]
    pub icon_overrides: BTreeMap<String, DeviceKind>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_secs: DEFAULT_POLL_SECS,
            notify_low_battery: true,
            low_threshold: 20,
            critical_threshold: 10,
            autostart: false,
            show_unknown: false,
            active_ble: true,
            animations: true,
            theme: Theme::System,
            language: Language::ZhCn,
            tray_per_device: false,
            icon_overrides: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn validate(&mut self) {
        self.poll_secs = self.poll_secs.clamp(MIN_POLL_SECS, MAX_POLL_SECS);
        // A critical tier must stay strictly below the low tier.  With
        // low < 2 there is no room for it, so clamp low to 2 first.
        self.low_threshold = self.low_threshold.clamp(2, 100);
        self.critical_threshold = self.critical_threshold.clamp(1, self.low_threshold - 1);
    }

    pub fn poll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.poll_secs.clamp(MIN_POLL_SECS, MAX_POLL_SECS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_overrides_round_trip_and_default_to_empty() {
        let default = Settings::default();
        assert!(default.icon_overrides.is_empty());

        let mut settings = Settings::default();
        settings
            .icon_overrides
            .insert("hid:ABCD:1234:deadbeef".to_owned(), DeviceKind::Headset);
        let text = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed.icon_overrides.get("hid:ABCD:1234:deadbeef"),
            Some(&DeviceKind::Headset)
        );
    }

    #[test]
    fn legacy_settings_without_icon_overrides_are_accepted() {
        let parsed: Settings = serde_json::from_str(r#"{"poll_secs":60}"#).unwrap();
        assert!(parsed.icon_overrides.is_empty());
    }

    #[test]
    fn clamps_poll_interval() {
        let mut s = Settings {
            poll_secs: 1,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.poll_secs, MIN_POLL_SECS);

        let mut s = Settings {
            poll_secs: 99_999,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.poll_secs, MAX_POLL_SECS);
    }

    #[test]
    fn keeps_thresholds_ordered() {
        let mut s = Settings {
            low_threshold: 10,
            critical_threshold: 20,
            ..Default::default()
        };
        s.validate();
        assert!(s.critical_threshold < s.low_threshold);
    }

    #[test]
    fn low_threshold_never_collides_with_critical() {
        let mut s = Settings {
            low_threshold: 1,
            critical_threshold: 1,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.low_threshold, 2);
        assert_eq!(s.critical_threshold, 1);

        let mut s = Settings {
            low_threshold: 2,
            critical_threshold: 0,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.critical_threshold, 1);
    }
}
