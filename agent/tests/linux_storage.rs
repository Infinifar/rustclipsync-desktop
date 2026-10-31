#![cfg(all(target_os = "linux", not(feature = "check-windows")))]
use rustclipsync_agent::{
    config::HistorySettings,
    linux::{
        pipe,
        storage::{self, InstanceLock},
    },
    local_history::LocalHistory,
};
use rustclipsync_core::adapters::{Direction, HistorySink};
use std::{
    fs,
    io::Cursor,
    os::unix::fs::{symlink, MetadataExt},
};

#[test]
fn private_atomic_files_are_bounded_and_links_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("profile");
    storage::ensure_private_dir(&dir).unwrap();
    assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    let path = dir.join("state");
    assert!(storage::read_private(&path, 32).unwrap().is_none());
    for value in [b"old".as_slice(), b"new encrypted bytes"] {
        storage::atomic_write(&path, value).unwrap();
        assert_eq!(storage::read_private(&path, 32).unwrap().unwrap(), value);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    }
    assert!(storage::read_private(&path, 2).is_err());
    let link = dir.join("link");
    symlink(&path, &link).unwrap();
    assert!(storage::read_private(&link, 32).is_err());
    assert!(storage::atomic_write(&link, b"must not overwrite").is_err());
    let hard = dir.join("hardlink");
    fs::hard_link(&path, &hard).unwrap();
    assert!(storage::read_private(&path, 32).is_err());
    assert!(storage::atomic_write(&hard, b"must not overwrite").is_err());
    assert!(!fs::read_dir(&dir).unwrap().any(|p| p
        .unwrap()
        .path()
        .extension()
        .is_some_and(|s| s == "tmp")));
}
#[test]
fn profile_lock_prevents_parallel_writers_and_releases_after_drop() {
    let temp = tempfile::tempdir().unwrap();
    let first = InstanceLock::acquire(temp.path()).unwrap();
    assert!(InstanceLock::acquire(temp.path()).is_err());
    drop(first);
    assert!(InstanceLock::acquire(temp.path()).is_ok());
}
#[test]
fn shared_history_remains_encrypted_and_excludes_sensitive_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("history");
    let key = rustclipsync_core::state::random_key().unwrap();
    let config = HistorySettings {
        enabled: true,
        ..Default::default()
    };
    let history = LocalHistory::open(path.clone(), key, config.clone());
    history
        .record_marked("private fixture", Direction::Download, 4_000_000_000, true)
        .unwrap();
    assert!(!path.exists());
    history
        .record("ordinary fixture", Direction::Upload, 4_000_000_000)
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(!bytes.windows(8).any(|s| s == b"ordinary"));
    assert_eq!(
        LocalHistory::open(path.clone(), key, config.clone())
            .data()
            .unwrap()
            .entries
            .len(),
        1
    );
    let mut broken = bytes;
    broken[20] ^= 1;
    fs::write(&path, &broken).unwrap();
    let history = LocalHistory::open(path.clone(), key, config);
    assert!(history.failed());
    assert!(history
        .record(
            "must not reset corrupt history",
            Direction::Upload,
            4_000_000_000
        )
        .is_err());
    assert_eq!(fs::read(path).unwrap(), broken);
}
#[test]
fn unix_ipc_frames_are_bounded_and_truncation_is_rejected() {
    let mut frame = Vec::new();
    pipe::write_frame(&mut frame, b"{\"ok\":true}").unwrap();
    assert_eq!(
        pipe::read_frame(&mut Cursor::new(&frame)).unwrap(),
        b"{\"ok\":true}"
    );
    frame.pop();
    assert!(pipe::read_frame(&mut Cursor::new(frame)).is_err());
    for header in [0u32, rustclipsync_agent::ipc::MAX_FRAME as u32 + 1] {
        assert!(pipe::read_frame(&mut Cursor::new(header.to_le_bytes())).is_err());
    }
}
#[test]
fn private_directory_rejects_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let link = temp.path().join("link");
    symlink(temp.path(), &link).unwrap();
    assert!(storage::ensure_private_dir(&link).is_err());
}
