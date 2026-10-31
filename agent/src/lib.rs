//! RustClipSync per-user Windows agent. OS handles never enter the shared core.
pub mod config;
pub mod text;
#[cfg(any(windows, feature = "check-windows"))]
pub mod windows;

pub mod credentials;

pub mod history;
pub mod inbox;
pub mod ipc;
