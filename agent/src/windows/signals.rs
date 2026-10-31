//! Win32 notification and hotkey handoff, outside the shared runtime.
use crate::config::Shortcuts;
use rustclipsync_core::{session::SyncError, Error};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::Duration,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
pub const STATUS_CHANGED: u32 = WM_APP + 2;
pub const WORKER_STOPPED: u32 = WM_APP + 3;
pub const UPDATE_SHORTCUTS: u32 = WM_APP + 4;
pub struct ShortcutRequest {
    pub settings: Shortcuts,
    pub reply: SyncSender<Result<(), &'static str>>,
    pub cancelled: AtomicBool,
}

#[derive(Default)]
pub struct Signals {
    pub hwnd: AtomicUsize,
    pub shortcut_request: Mutex<Option<Arc<ShortcutRequest>>>,
}
pub enum Notification {
    StatusChanged,
    WorkerStopped,
}
pub fn notify(signals: &Signals, notification: Notification) {
    let message = match notification {
        Notification::StatusChanged => STATUS_CHANGED,
        Notification::WorkerStopped => WORKER_STOPPED,
    };
    unsafe {
        PostMessageW(signals.hwnd.load(Ordering::SeqCst) as _, message, 0, 0);
    }
}
pub fn inbox_received(_: &Signals) {}
pub fn bind_shortcuts(signals: &Signals, settings: &Shortcuts) -> Result<(), SyncError> {
    let (tx, rx) = mpsc::sync_channel(1);
    let request = Arc::new(ShortcutRequest {
        settings: settings.clone(),
        reply: tx,
        cancelled: AtomicBool::new(false),
    });
    *signals
        .shortcut_request
        .lock()
        .map_err(|_| io::Error::other("shortcuts unavailable"))? = Some(request.clone());
    if unsafe {
        PostMessageW(
            signals.hwnd.load(Ordering::SeqCst) as _,
            UPDATE_SHORTCUTS,
            0,
            0,
        )
    } == 0
    {
        request.cancelled.store(true, Ordering::SeqCst);
        return Err(io::Error::other("shortcut window unavailable").into());
    }
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(SyncError::Remote(Error::Status(423))),
        Err(_) => {
            request.cancelled.store(true, Ordering::SeqCst);
            Err(io::Error::other("shortcut update unconfirmed").into())
        }
    }
}
