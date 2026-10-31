use super::{
    path_wide,
    security::{self, Security},
    Handle,
};
use rustclipsync_core::{
    adapters::StateStore,
    state::{self, MAX_STATE},
};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ,
        GENERIC_WRITE,
    },
    Storage::FileSystem::*,
};

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    unsafe {
        // Caller supplies an existing LOCALAPPDATA parent (or an existing test parent).
        let security = Security::current_user()?;
        let attrs = security.attributes();
        let wide = path_wide(path)?;
        if CreateDirectoryW(wide.as_ptr(), &attrs) == 0
            && io::Error::last_os_error().raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32)
        {
            return Err(io::Error::last_os_error());
        }
        reject_reparse(path)?;
        security.protect_path(path)
    }
}
pub fn reject_reparse(path: &Path) -> io::Result<bool> {
    unsafe {
        let attrs = GetFileAttributesW(path_wide(path)?.as_ptr());
        if attrs == INVALID_FILE_ATTRIBUTES {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(),Some(n) if n==ERROR_FILE_NOT_FOUND as i32||n==ERROR_PATH_NOT_FOUND as i32)
            {
                return Ok(false);
            }
            return Err(error);
        }
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "reparse points are not allowed in private storage",
            ));
        }
        Ok(true)
    }
}
pub fn read_private(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    if !reject_reparse(path)? {
        return Ok(None);
    }
    Security::current_user()?.protect_path(path)?;
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("private file too large"));
    }
    Ok(Some(bytes))
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    unsafe {
        reject_reparse(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing parent"))?;
        let tmp = parent.join(format!(".{}.tmp", state::random_uuid()?));
        let security = Security::current_user()?;
        let attrs = security.attributes();
        let result = (|| {
            let file = Handle::new(CreateFileW(
                path_wide(&tmp)?.as_ptr(),
                GENERIC_WRITE,
                0,
                &attrs,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH,
                ptr::null_mut(),
            ))?;
            let mut offset = 0;
            while offset < bytes.len() {
                let mut written = 0;
                let count = (bytes.len() - offset).min(u32::MAX as usize) as u32;
                if WriteFile(
                    file.0,
                    bytes[offset..].as_ptr(),
                    count,
                    &mut written,
                    ptr::null_mut(),
                ) == 0
                    || written == 0
                {
                    return Err(io::Error::last_os_error());
                }
                offset += written as usize;
            }
            if FlushFileBuffers(file.0) == 0 {
                return Err(io::Error::last_os_error());
            }
            drop(file);
            if MoveFileExW(
                path_wide(&tmp)?.as_ptr(),
                path_wide(path)?.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}
/// Holding this handle denies other opens, including from another login session.
pub struct InstanceLock {
    _handle: Handle,
}
impl InstanceLock {
    pub fn acquire(dir: &Path) -> io::Result<Self> {
        unsafe {
            let path = dir.join("agent.lock");
            let exists = reject_reparse(&path)?;
            let security = Security::current_user()?;
            if exists {
                security.protect_path(&path)?;
            }
            let attrs = security.attributes();
            let handle = Handle::new(CreateFileW(
                path_wide(&path)?.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                &attrs,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            ))
            .map_err(|_| {
                io::Error::other("this profile is already in use, or the lock cannot be opened")
            })?;
            Ok(Self { _handle: handle })
        }
    }
}
#[derive(Clone)]
pub struct FileStateStore(pub PathBuf);
impl StateStore for FileStateStore {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        read_private(&self.0, MAX_STATE)
    }
    fn save(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() as u64 > MAX_STATE {
            return Err(io::Error::other("state too large"));
        }
        atomic_write(&self.0, bytes)
    }
}
pub fn load_key(dir: &Path, identity: &str) -> io::Result<[u8; 32]> {
    let path = dir.join(format!("{identity}.key.dpapi"));
    if let Some(blob) = read_private(&path, 64 * 1024)? {
        let clear = security::unprotect(&blob)?;
        return clear
            .as_slice()
            .try_into()
            .map_err(|_| io::Error::other("invalid protected state key"));
    }
    if reject_reparse(&dir.join(format!("{identity}.state")))? {
        return Err(io::Error::other(
            "state exists but its protected key is missing",
        ));
    }
    let key = state::random_key()?;
    atomic_write(&path, &security::protect(&key)?)?;
    Ok(key)
}
