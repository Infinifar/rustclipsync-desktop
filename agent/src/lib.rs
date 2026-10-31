//! RustClipSync per-user desktop agent. OS handles never enter the shared core.
pub mod config;
pub mod text;
#[cfg(any(windows, feature = "check-windows"))]
pub mod windows;

pub mod credentials;

pub mod history;
pub mod inbox;
pub mod ipc;

#[cfg(any(windows, feature = "check-windows"))]
pub use windows as platform;
#[cfg(all(target_os = "linux", not(feature = "check-windows")))]
pub mod linux;
#[cfg(all(target_os = "linux", not(feature = "check-windows")))]
pub use linux as platform;
#[cfg(any(windows, target_os = "linux", feature = "check-windows"))]
pub mod local_history;
#[cfg(any(windows, target_os = "linux", feature = "check-windows"))]
pub mod runtime;
