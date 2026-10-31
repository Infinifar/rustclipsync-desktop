#![cfg(any(windows, feature = "check-windows"))]
use rustclipsync_agent::windows::{clipboard::WindowsClipboard, runtime::Control, window::Window};
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError},
    state,
};
use std::sync::{mpsc, Arc};
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
