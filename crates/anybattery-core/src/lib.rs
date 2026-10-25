pub mod config;
pub mod dedup;
pub mod model;
pub mod protocol;

pub use config::{Language, Settings, Theme, DEFAULT_POLL_SECS, MAX_POLL_SECS, MIN_POLL_SECS};
pub use model::{
    age_text, now_unix, BatteryReading, BatterySource, BatteryStatus, CoarseLevel, Confidence,
    DeviceId, DeviceKind, DeviceSnapshot, Transport,
};
