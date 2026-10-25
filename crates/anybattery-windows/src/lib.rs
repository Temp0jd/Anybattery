//! Windows platform layer: device enumeration, battery providers, and vendor adapters.

#[cfg(windows)]
mod bluetooth;
#[cfg(windows)]
mod hid;
#[cfg(windows)]
mod scan;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
mod vendor;
#[cfg(windows)]
mod xbox;

#[cfg(windows)]
pub use scan::{scan, ScanResult};
