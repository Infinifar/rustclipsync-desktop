//! Linux desktop boundaries: no Linux dependency enters rustclipsync-core.
pub mod clipboard;
pub mod host;
mod notifications;
pub mod pipe;
pub mod storage;
mod wayland;
mod x11;

use crate::config::Shortcuts;
use rustclipsync_core::session::SyncError;

#[derive(Default)]
pub struct Signals {
    pub(crate) last_notification: std::sync::Mutex<Option<std::time::Instant>>,
}
pub enum Notification {
    StatusChanged,
    WorkerStopped,
}
pub fn notify(_: &Signals, _: Notification) {}
pub use notifications::inbox_received;
pub fn bind_shortcuts(_: &Signals, settings: &Shortcuts) -> Result<(), SyncError> {
    // Windows virtual-key bindings must not appear to succeed on Wayland.
    // Linux desktop bindings activate the public --history / --inbox commands.
    if settings.history.is_some() || settings.inbox.is_some() {
        return Err(SyncError::Remote(rustclipsync_core::Error::InvalidProfile));
    }
    Ok(())
}
