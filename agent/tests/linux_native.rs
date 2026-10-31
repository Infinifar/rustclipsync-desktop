#![cfg(all(target_os = "linux", not(feature = "check-windows")))]
use rustclipsync_agent::{
    config::Config,
    linux::{clipboard::PlatformClipboard, pipe, storage},
    runtime::Control,
};
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError},
    state::{self, InboxItem, Persisted, State},
};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};

fn disposable() {
    assert_eq!(
        std::env::var("RCS_LINUX_DISPOSABLE").as_deref(),
        Ok("1"),
        "use scripts/test-linux-native.py; never run against the active desktop/keyring"
    );
}
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(Instant::now() < deadline, "native operation timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn rpc(dir: &Path, method: &str, params: Value) -> Value {
    pipe::call(
        dir,
        json!({"id":state::random_uuid().unwrap(),"ipcVersion":1,"method":method,"params":params})
            .to_string()
            .as_bytes(),
    )
    .unwrap()
}

#[test]
#[ignore = "Uses a real clipboard and Secret Service; run only in an isolated session"]
fn native_agent_preserves_identity_controls_and_encrypted_inbox() {
    disposable();
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("profile");
    storage::ensure_private_dir(&dir).unwrap();
    let mut config = Config::new("https://127.0.0.1:1".into(), "fixture".into());
    config.paused = true;
    config.allow_sensitive = true;
    config.history.enabled = true;
    config.validate().unwrap();
    storage::atomic_write(
        &dir.join("config.json"),
        &serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let identity = config.identity();
    let key = storage::load_key(&dir, &identity).unwrap();
    let item = InboxItem {
        id: state::random_uuid().unwrap(),
        text: "synthetic private fixture 中文 😀\n".repeat(20000),
        received_at: 4_000_000_000,
    };
    let saved = Persisted {
        server: config.server.clone(),
        token: state::random_token().unwrap(),
        paired: true,
        state: State::new(
            state::random_uuid().unwrap(),
            state::random_uuid().unwrap(),
            42,
        ),
        inbox: Some(item.clone()),
    };
    storage::atomic_write(
        &dir.join(format!("{identity}.state")),
        &state::seal(&key, &saved).unwrap(),
    )
    .unwrap();
    let mut history = rustclipsync_agent::history::HistoryData::default();
    history
        .record(
            &config.history,
            "ordinary UI test fixture",
            "upload",
            4_000_000_000,
        )
        .unwrap();
    storage::atomic_write(
        &dir.join(format!("{identity}.history")),
        &rustclipsync_agent::history::seal(&key, &history).unwrap(),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_rustclipsync-agent"))
        .arg("--data-dir")
        .arg(&dir)
        .arg("run")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Guard(child);
    wait(|| {
        pipe::call(
            &dir,
            br#"{"id":"probe","ipcVersion":1,"method":"getStatus","params":{}}"#,
        )
        .ok()
        .is_some_and(|v| v["result"]["deviceId"] == saved.state.device_id)
    });
    let status = rpc(&dir, "getStatus", json!({}));
    assert_eq!(status["result"]["cursor"], 42);
    assert!(!status.to_string().contains(&saved.token));
    assert!(!status.to_string().contains("synthetic private"));
    assert_eq!(
        rpc(&dir, "readInbox", json!({"id":item.id,"offset":0}))["result"]["text"]
            .as_str()
            .unwrap()
            .len(),
        16 * 1024
    );
    assert_eq!(
        rpc(
            &dir,
            "readInbox",
            json!({"id":state::random_uuid().unwrap(),"offset":0})
        )["error"],
        "invalid_request"
    );
    if let Ok(ui) = std::env::var("RCS_LINUX_UI_BINARY") {
        let mut gui = Command::new(ui)
            .arg("--data-dir")
            .arg(&dir)
            .arg("--integration-test")
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("QT_QPA_PLATFORMTHEME", "generic")
            .env("QT_QUICK_BACKEND", "software")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(exit) = gui.try_wait().unwrap() {
                assert!(exit.success(), "native Qt UI verification failed: {exit}");
                break;
            }
            if Instant::now() >= deadline {
                let _ = gui.kill();
                panic!("native Qt UI timeout");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let status = rpc(&dir, "getStatus", json!({}));
    config.history.days = 14;
    let updated = rpc(
        &dir,
        "updateSettings",
        json!({"config":config,"expectedRevision":status["result"]["configRevision"]}),
    );
    assert_eq!(updated["ok"], true, "{updated}");
    assert_eq!(
        rpc(
            &dir,
            "updateSettings",
            json!({"config":config,"expectedRevision":status["result"]["configRevision"]})
        )["error"],
        "settings_changed"
    );
    assert_eq!(rpc(&dir, "copyInbox", json!({"id":item.id}))["ok"], true);
    assert_eq!(rpc(&dir, "clearInbox", json!({"id":item.id}))["ok"], true);
    assert!(rpc(&dir, "getStatus", json!({}))["result"]["inbox"].is_null());
    assert_eq!(rpc(&dir, "getStatus", json!({}))["result"]["cursor"], 42);
    assert_eq!(rpc(&dir, "listHistory", json!({}))["result"]["total"], 1);
    assert_eq!(rpc(&dir, "quit", json!({}))["ok"], true);
    wait(|| guard.0.try_wait().unwrap().is_some());
    assert!(guard.0.wait().unwrap().success());
    let bytes = storage::read_private(&dir.join(format!("{identity}.state")), 64 * 1024 * 1024)
        .unwrap()
        .unwrap();
    assert!(!bytes.windows(17).any(|b| b == b"synthetic private"));
    let persisted = state::open(&key, bytes).unwrap();
    assert_eq!(persisted.state.device_id, saved.state.device_id);
    assert_eq!(persisted.state.cursor, 42);
    assert!(persisted.inbox.is_none());
}

#[test]
#[ignore = "Uses a real clipboard; run only in an isolated display server"]
fn native_clipboard_roundtrips_large_unicode_sensitive_markers_and_own_events() {
    disposable();
    let config = Config::new("https://example.test".into(), "fixture".into());
    let (tx, _rx) = mpsc::sync_channel(32);
    let control = Arc::new(Control::new(&config, tx));
    let (clipboard, monitor) = PlatformClipboard::start(control.clone()).unwrap();
    let (tx, _rx) = mpsc::sync_channel(32);
    let other = Arc::new(Control::new(&config, tx));
    let (reader, reader_monitor) = PlatformClipboard::start(other.clone()).unwrap();
    // Initial selections are snapshots, not newly observed local copy events.
    std::thread::sleep(Duration::from_millis(200));
    let baseline = control.generation.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(control.generation.load(Ordering::SeqCst), baseline);
    let text = "large synthetic fixture 中文 😀\n".repeat(20000);
    let id = state::random_uuid().unwrap();
    assert!(matches!(
        clipboard.write(&text, &id, true),
        Err(ClipboardError::Sensitive)
    ));
    clipboard.set_auto_apply_sensitive(true);
    clipboard.write(&text, &id, true).unwrap();
    wait(|| {
        reader
            .snapshot(true)
            .ok()
            .flatten()
            .is_some_and(|(v, s)| v == text && s)
    });
    assert!(matches!(
        reader.snapshot(false),
        Err(ClipboardError::Sensitive)
    ));
    wait(|| {
        clipboard
            .observe(true)
            .ok()
            .is_some_and(|o| o.unchanged && o.marker.as_deref() == Some(&id))
    });
    let observation = reader.observe(true).unwrap();
    assert_eq!(observation.marker, Some(id));
    clipboard.set_auto_apply_sensitive(false);
    assert!(matches!(
        clipboard.write("must not replace", &state::random_uuid().unwrap(), true),
        Err(ClipboardError::Sensitive)
    ));
    assert_eq!(reader.snapshot(true).unwrap().unwrap().0, text);
    drop(reader_monitor);
    drop(monitor);
}

#[test]
#[ignore = "Uses Secret Service; run only in an isolated D-Bus/keyring"]
fn secret_service_roundtrip_and_missing_key_never_reset_encrypted_state() {
    disposable();
    let temp = tempfile::tempdir().unwrap();
    let key = storage::load_key(temp.path(), "fixture").unwrap();
    assert_eq!(key, storage::load_key(temp.path(), "fixture").unwrap());
    storage::atomic_write(
        &temp.path().join("missing.state"),
        b"existing encrypted state",
    )
    .unwrap();
    assert!(storage::load_key(temp.path(), "missing").is_err());
    assert!(!std::fs::read_dir(temp.path()).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_some_and(|s| s == "key")));
}

#[test]
#[ignore = "Uses an isolated D-Bus notification service; run only in the disposable session"]
fn desktop_notification_is_delivered_without_content_and_coalesces_bursts() {
    use dbus::{
        blocking::Connection,
        channel::{MatchingReceiver, Sender},
        message::MatchRule,
    };
    use std::sync::atomic::AtomicUsize;
    disposable();
    let connection = Connection::new_session().unwrap();
    connection
        .request_name("org.freedesktop.Notifications", false, true, false)
        .unwrap();
    let received = Arc::new(AtomicUsize::new(0));
    let count = received.clone();
    connection.start_receive(
        MatchRule::new_method_call()
            .with_interface("org.freedesktop.Notifications")
            .with_member("Notify"),
        Box::new(move |message, connection| {
            let (application, _, icon, summary, body, actions, hints, _): (
                String,
                u32,
                String,
                String,
                String,
                Vec<String>,
                dbus::arg::PropMap,
                i32,
            ) = message.read_all().unwrap();
            assert_eq!(application, "RustClipSync");
            assert_eq!(summary, "RustClipSync");
            assert_eq!(icon, "io.github.infinifar.rustclipsync");
            assert!(body.contains("敏感收件区"));
            assert!(actions.is_empty());
            assert_eq!(hints.len(), 1);
            assert!(hints.contains_key("desktop-entry"));
            count.fetch_add(1, Ordering::SeqCst);
            connection
                .send(message.method_return().append1(1u32))
                .unwrap();
            true
        }),
    );
    let signals = rustclipsync_agent::linux::Signals::default();
    rustclipsync_agent::linux::inbox_received(&signals);
    rustclipsync_agent::linux::inbox_received(&signals);
    let deadline = Instant::now() + Duration::from_secs(5);
    while received.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "desktop notification was not delivered"
        );
        connection.process(Duration::from_millis(50)).unwrap();
    }
    connection.process(Duration::from_millis(200)).unwrap();
    assert_eq!(received.load(Ordering::SeqCst), 1);
}
