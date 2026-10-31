#![cfg(any(windows, feature = "check-windows"))]
use rustclipsync_agent::{config::Config, windows::storage};
use rustclipsync_core::state::{self, InboxItem, Persisted, State};
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn rpc(method: &str, params: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustclipsync-agent"))
        .arg("rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request =
        json!({"id":state::random_uuid().unwrap(),"ipcVersion":1,"method":method,"params":params});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
#[ignore = "Creates a user/session named pipe and tray; run only on a disposable Windows runner"]
fn native_pipe_controls_real_agent_preserves_identity_and_never_exposes_secrets() {
    assert_eq!(std::env::var("RCS_WINDOWS_DISPOSABLE").as_deref(), Ok("1"));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("RustClipSync");
    storage::ensure_private_dir(&dir).unwrap();
    let mut config = Config::new("https://127.0.0.1:1".into(), "fixture".into());
    config.paused = true;
    config.validate().unwrap();
    storage::atomic_write(
        &dir.join("config.json"),
        &serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let identity = config.identity();
    let key = storage::load_key(&dir, &identity).unwrap();
    let device = state::random_uuid().unwrap();
    let saved = Persisted {
        server: config.server.clone(),
        token: state::random_token().unwrap(),
        paired: true,
        state: State::new(state::random_uuid().unwrap(), device.clone(), 42),
        inbox: Some(InboxItem {
            id: state::random_uuid().unwrap(),
            text: "private fixture, never in status".into(),
            received_at: 4000000000,
        }),
    };
    storage::atomic_write(
        &dir.join(format!("{identity}.state")),
        &state::seal(&key, &saved).unwrap(),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_rustclipsync-agent"))
        .arg("run")
        .env("LOCALAPPDATA", temp.path())
        .spawn()
        .unwrap();
    let mut guard = Guard(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let output = Command::new(env!("CARGO_BIN_EXE_rustclipsync-agent"))
            .arg("rpc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut c| {
                c.stdin.take().unwrap().write_all(
                    json!({"id":"probe","ipcVersion":1,"method":"getStatus","params":{}})
                        .to_string()
                        .as_bytes(),
                )?;
                c.wait_with_output()
            })
            .unwrap();
        if output.status.success()
            && serde_json::from_slice::<Value>(&output.stdout)
                .ok()
                .is_some_and(|v| v["result"]["deviceId"] == device)
        {
            break;
        }
        assert!(Instant::now() < deadline, "pipe did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    let snapshot = rpc("getStatus", json!({}));
    assert_eq!(snapshot["result"]["deviceId"], device);
    assert_eq!(snapshot["result"]["cursor"], 42);
    assert_eq!(snapshot["result"]["paused"], true);
    let serialized = snapshot.to_string();
    assert!(!serialized.contains(&saved.token));
    assert!(!serialized.contains("private fixture"));
    let item_id = &saved.inbox.as_ref().unwrap().id;
    let explicit = rpc("readInbox", json!({"id":item_id,"offset":0}));
    assert_eq!(
        explicit["result"]["text"],
        "private fixture, never in status"
    );
    assert_eq!(rpc("getStatus", json!({}))["result"]["cursor"], 42);
    assert_eq!(
        rpc(
            "readInbox",
            json!({"id":state::random_uuid().unwrap(),"offset":0})
        )["error"],
        "invalid_request"
    );
    assert_eq!(
        rpc("readInbox", json!({"id":item_id,"offset":1000}))["error"],
        "invalid_request"
    );
    let mut updated = config.clone();
    updated.interval_sec = 90;
    updated.history.enabled = true;
    updated.history.days = 3;
    updated.allow_sensitive = true;
    updated.auto_apply_sensitive = true;
    let response = rpc(
        "updateSettings",
        json!({"config":updated,"expectedRevision":1}),
    );
    assert_eq!(response["ok"], true);
    let stale = rpc(
        "updateSettings",
        json!({"config":config,"expectedRevision":1}),
    );
    assert_eq!(stale["error"], "settings_changed");
    let snapshot = rpc("getStatus", json!({}));
    assert_eq!(snapshot["result"]["config"]["intervalSec"], 90);
    assert_eq!(snapshot["result"]["config"]["autoApplySensitive"], true);
    assert_eq!(snapshot["result"]["cursor"], 42);
    assert_eq!(snapshot["result"]["deviceId"], device);
    assert_ne!(snapshot["result"]["state"], "conflict");
    updated.auto_apply_sensitive = false;
    assert_eq!(
        rpc(
            "updateSettings",
            json!({"config":updated,"expectedRevision":2})
        )["ok"],
        true
    );
    assert_eq!(
        rpc("getStatus", json!({}))["result"]["config"]["autoApplySensitive"],
        false
    );
    assert_eq!(
        rpc(
            "testFilter",
            json!({"filter":{"mode":"blacklist","patterns":["token"],"downloads":true},"text":"token fixture"})
        )["result"]["uploadAllowed"],
        false
    );
    updated.shortcuts.history = Some(rustclipsync_agent::config::KeyBinding {
        modifiers: 3,
        key: 0x78,
    });
    updated.shortcuts.inbox = Some(rustclipsync_agent::config::KeyBinding {
        modifiers: 3,
        key: 0x79,
    });
    assert_eq!(
        rpc(
            "updateSettings",
            json!({"config":updated,"expectedRevision":3})
        )["ok"],
        true
    );
    unsafe {
        assert_ne!(RegisterHotKey(std::ptr::null_mut(), 88, 3, 0x7a), 0);
    }
    let mut conflict = updated.clone();
    conflict.shortcuts.history.as_mut().unwrap().key = 0x7a;
    assert_eq!(
        rpc(
            "updateSettings",
            json!({"config":conflict,"expectedRevision":4})
        )["error"],
        "shortcut_conflict"
    );
    assert_eq!(
        rpc("getStatus", json!({}))["result"]["config"]["shortcuts"]["history"]["key"],
        0x78
    );
    assert_eq!(rpc("getStatus", json!({}))["result"]["configRevision"], 4);
    unsafe {
        assert_eq!(
            RegisterHotKey(std::ptr::null_mut(), 89, 3, 0x78),
            0,
            "old binding was lost on conflict"
        );
        UnregisterHotKey(std::ptr::null_mut(), 88);
    }
    assert_eq!(
        rpc(
            "updateSettings",
            json!({"config":updated,"expectedRevision":4,"rebindShortcuts":true})
        )["ok"],
        true
    );
    assert_eq!(
        rpc("getStatus", json!({}))["result"]["shortcutError"],
        false
    );
    assert_eq!(rpc("clearHistory", json!({}))["ok"], true);
    assert_eq!(rpc("getStatus", json!({}))["result"]["cursor"], 42);
    rpc("quit", json!({}));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if guard.0.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "quit did not complete");
        std::thread::sleep(Duration::from_millis(100));
    }
    let persisted = state::open(
        &key,
        storage::read_private(&dir.join(format!("{identity}.state")), state::MAX_STATE)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(persisted.state.device_id, device);
    assert_eq!(
        persisted.inbox.unwrap().text,
        "private fixture, never in status"
    );
    let config: Config =
        serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(config.history.days, 3);
    assert!(config.allow_sensitive);
    assert!(!config.auto_apply_sensitive);
    assert_eq!(config.shortcuts.history.unwrap().key, 0x78);
    unsafe {
        assert_ne!(
            RegisterHotKey(std::ptr::null_mut(), 90, 3, 0x78),
            0,
            "quit did not release shortcut"
        );
        UnregisterHotKey(std::ptr::null_mut(), 90);
    }
}
