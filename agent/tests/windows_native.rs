#![cfg(any(windows, feature = "check-windows"))]
// These tests are only executed on Windows. check-windows permits Linux type checks.
use rustclipsync_agent::windows::{
    path_wide, security,
    storage::{self, FileStateStore, InstanceLock},
};
use rustclipsync_core::{adapters::StateStore, state};
use std::{mem, ptr};
use windows_sys::Win32::Security::*;
fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("RustClipSync");
    storage::ensure_private_dir(&root).unwrap();
    (temp, root)
}
#[test]
fn dpapi_authenticates_and_roundtrips_without_plaintext_file() {
    let value = state::random_key().unwrap();
    let protected = security::protect(&value).unwrap();
    assert!(!protected.windows(value.len()).any(|v| v == value));
    assert_eq!(&*security::unprotect(&protected).unwrap(), &value);
    let mut tampered = protected;
    let i = tampered.len() / 2;
    tampered[i] ^= 0x80;
    assert!(security::unprotect(&tampered).is_err());
}
#[test]
fn atomic_state_replaces_and_authenticates_without_temp_leaks() {
    let (_temp, root) = root();
    let store = FileStateStore(root.join("state"));
    let key = state::random_key().unwrap();
    let mut saved = state::Persisted {
        server: "https://example.test".into(),
        token: state::random_token().unwrap(),
        paired: true,
        state: state::State::new(
            state::random_uuid().unwrap(),
            state::random_uuid().unwrap(),
            0,
        ),
    };
    for cursor in 0..5 {
        saved.state.cursor = cursor;
        store.save(&state::seal(&key, &saved).unwrap()).unwrap();
        assert_eq!(
            state::open(&key, store.load().unwrap().unwrap()).unwrap(),
            saved
        );
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}
#[test]
fn one_profile_has_one_writer_even_when_lock_file_already_exists() {
    let (_temp, root) = root();
    let first = InstanceLock::acquire(&root).unwrap();
    assert!(InstanceLock::acquire(&root).is_err());
    drop(first);
    let second = InstanceLock::acquire(&root).unwrap();
    drop(second);
}
#[test]
fn missing_key_never_resets_existing_identity() {
    let (_temp, root) = root();
    let key = storage::load_key(&root, "fixture").unwrap();
    assert_eq!(storage::load_key(&root, "fixture").unwrap(), key);
    storage::atomic_write(&root.join("fixture.state"), b"existing ciphertext").unwrap();
    std::fs::remove_file(root.join("fixture.key.dpapi")).unwrap();
    assert!(storage::load_key(&root, "fixture").is_err());
}
#[test]
fn private_files_have_protected_single_user_dacl() {
    unsafe {
        let (_temp, root) = root();
        let path = root.join("data");
        storage::atomic_write(&path, b"fixture").unwrap();
        for path in [&root, &path] {
            let path = path_wide(path).unwrap();
            let mut needed = 0;
            GetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                0,
                &mut needed,
            );
            let mut buffer = vec![0usize; (needed as usize).div_ceil(mem::size_of::<usize>())];
            let descriptor = buffer.as_mut_ptr().cast();
            assert_ne!(
                GetFileSecurityW(
                    path.as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    descriptor,
                    needed,
                    &mut needed
                ),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
        }
    }
}
#[test]
fn oversized_private_file_is_rejected_before_decoding() {
    let (_temp, root) = root();
    let path = root.join("large");
    storage::atomic_write(&path, &[0; 100]).unwrap();
    assert!(storage::read_private(&path, 99).is_err());
}
