#![cfg(any(windows, feature = "check-windows"))]
use rustclipsync_agent::windows::{clipboard::WindowsClipboard, runtime::Control, window::Window};
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError},
    state,
};
use std::sync::{mpsc, Arc};
use windows_sys::Win32::System::{DataExchange::*, Memory::*};

fn sensitive_flags(window: &Window) {
    // Inspect each published Win32 format, rather than inferring all three from
    // the adapter's combined sensitive flag.
    unsafe {
        assert_ne!(OpenClipboard(window.hwnd() as _), 0);
        for (name, expected) in [
            ("ExcludeClipboardContentFromMonitorProcessing", 1u32),
            ("CanIncludeInClipboardHistory", 0),
            ("CanUploadToCloudClipboard", 0),
        ] {
            let format = RegisterClipboardFormatW(rustclipsync_agent::windows::wide(name).as_ptr());
            let handle = GetClipboardData(format);
            assert!(!handle.is_null(), "missing {name}");
            assert!(GlobalSize(handle) >= 4);
            let memory = GlobalLock(handle);
            assert!(!memory.is_null());
            let actual = std::ptr::read_unaligned(memory.cast::<u32>());
            GlobalUnlock(handle);
            assert_eq!(actual, expected, "{name}");
        }
        assert_ne!(CloseClipboard(), 0);
    }
}
#[test]
#[ignore = "Writes the real Windows clipboard; run only in a disposable Windows session"]
fn writes_origin_marker_and_rejects_stale_sequence() {
    assert_eq!(std::env::var("RCS_WINDOWS_DISPOSABLE").as_deref(), Ok("1"));
    let (sender, _receiver) = mpsc::sync_channel(8);
    let control = Arc::new(Control::new(
        &rustclipsync_agent::config::Config::new("https://example.test".into(), "fixture".into()),
        sender,
    ));
    let window = Window::create(control, false).unwrap();
    let formats = window.formats().unwrap();
    let a = WindowsClipboard::new(window.hwnd(), formats);
    let b = WindowsClipboard::new(window.hwnd(), formats);
    let _ = a.snapshot(false);
    a.write("fixture 中文 😀", &state::random_uuid().unwrap(), false)
        .unwrap();
    let own = a.observe(false).unwrap();
    assert!(own.unchanged);
    assert_eq!(b.snapshot(false).unwrap().unwrap().0, "fixture 中文 😀");
    assert!(!b.observe(false).unwrap().unchanged);
    assert!(b.observe(false).unwrap().unchanged);
    let _ = a.snapshot(false);
    b.write("newer local text", &state::random_uuid().unwrap(), false)
        .unwrap();
    assert!(matches!(
        a.write("stale download", &state::random_uuid().unwrap(), false),
        Err(ClipboardError::Unavailable)
    ));
    assert_eq!(b.snapshot(false).unwrap().unwrap().0, "newer local text");
}

#[test]
#[ignore = "Writes the real Windows clipboard; run only in a disposable Windows session"]
fn sensitive_auto_write_is_opt_in_marked_and_suppresses_its_own_notifications() {
    assert_eq!(std::env::var("RCS_WINDOWS_DISPOSABLE").as_deref(), Ok("1"));
    let (sender, _receiver) = mpsc::sync_channel(8);
    let control = Arc::new(Control::new(
        &rustclipsync_agent::config::Config::new("https://example.test".into(), "fixture".into()),
        sender,
    ));
    let window = Window::create(control, false).unwrap();
    let formats = window.formats().unwrap();
    let a = WindowsClipboard::new(window.hwnd(), formats);
    let reader = WindowsClipboard::new(window.hwnd(), formats);
    a.snapshot(true).unwrap();
    let id = state::random_uuid().unwrap();
    assert!(matches!(
        a.write("marked fixture", &id, true),
        Err(ClipboardError::Sensitive)
    ));
    a.set_auto_apply_sensitive(true);
    a.write("marked fixture", &id, true).unwrap();
    sensitive_flags(&window);
    assert!(a.observe(true).unwrap().unchanged);
    assert!(a.observe(true).unwrap().unchanged);
    assert!(matches!(
        reader.snapshot(false),
        Err(ClipboardError::Sensitive)
    ));
    assert_eq!(
        reader.snapshot(true).unwrap(),
        Some(("marked fixture".into(), true))
    );
    a.set_auto_apply_sensitive(false);
    assert!(matches!(
        a.write("must not replace", &state::random_uuid().unwrap(), true),
        Err(ClipboardError::Sensitive)
    ));
    assert_eq!(reader.snapshot(true).unwrap().unwrap().0, "marked fixture");
}
