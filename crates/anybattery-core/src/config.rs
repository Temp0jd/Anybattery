use serde::{Deserialize, Serialize};

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
