//! All unsafe Win32 calls are confined to this platform boundary.
pub mod clipboard;
pub mod runtime;
pub mod security;
pub mod storage;
pub mod window;
use std::{io, path::Path};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
pub fn path_wide(path: &Path) -> io::Result<Vec<u16>> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::other("unsupported non-Unicode path"))?;
    if text.contains('\0') {
        return Err(io::Error::other("invalid path"));
    }
    Ok(wide(text))
}
pub struct Handle(pub HANDLE);
impl Handle {
    pub fn new(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
