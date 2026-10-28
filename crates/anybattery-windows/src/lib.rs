//! Windows platform layer: device enumeration, battery providers, and vendor adapters.

#[cfg(windows)]
mod apple;
#[cfg(windows)]
mod bluetooth;
#[cfg(windows)]
mod extra;
#[cfg(windows)]
mod hid;
#[cfg(windows)]
pub mod instance;
#[cfg(windows)]
pub mod monitor;
#[cfg(windows)]
mod passive;
#[cfg(windows)]
mod scan;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
mod user_profile;
#[cfg(windows)]
mod vendor;
#[cfg(windows)]
mod xbox;

#[cfg(windows)]
pub use hid::HidInterface;
#[cfg(windows)]
pub use instance::{AcquireInstance, InstanceError, PrimaryInstance, acquire_or_signal};
#[cfg(windows)]
pub use passive::{
    CaptureSummary, PassiveCaptureError, PassiveRead, READ_SLICE_MS, ReadOnlyHid, capture_with,
};
#[cfg(windows)]
pub use scan::{ScanEvent, ScanResult, scan, scan_with_profiles_and_progress, scan_with_progress};
#[cfg(windows)]
pub use user_profile::{binding_for, find_profile, read_user_level_with, snapshot_from_user};
