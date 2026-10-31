use super::wide;
use crate::text::decode_unicode;
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError},
    MAX_TEXT_BYTES,
};
use std::{
    cell::{Cell, RefCell},
    io, mem, ptr, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HANDLE, HWND},
    System::{DataExchange::*, Memory::*},
};
const CF_UNICODETEXT: u32 = 13;
#[derive(Clone, Copy)]
pub struct Formats {
    pub origin: u32,
    exclude: u32,
    history: u32,
    cloud: u32,
}
impl Formats {
    pub fn register() -> io::Result<Self> {
        unsafe {
            let register = |name: &str| {
                let id = RegisterClipboardFormatW(wide(name).as_ptr());
                if id == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(id)
                }
            };
            Ok(Self {
                origin: register("RustClipSync.MessageId")?,
                exclude: register("ExcludeClipboardContentFromMonitorProcessing")?,
                history: register("CanIncludeInClipboardHistory")?,
                cloud: register("CanUploadToCloudClipboard")?,
            })
        }
    }
}
pub struct Observation {
    pub value: Option<(String, bool)>,
    pub marker: Option<String>,
    pub unchanged: bool,
}
pub struct WindowsClipboard {
    hwnd: usize,
    formats: Formats,
    expected: Cell<Option<u32>>,
    observed: Cell<Option<u32>>,
    written: RefCell<Option<String>>,
}
impl WindowsClipboard {
    pub fn new(hwnd: usize, formats: Formats) -> Self {
        Self {
            hwnd,
            formats,
            expected: Cell::new(None),
            observed: Cell::new(None),
            written: RefCell::new(None),
        }
    }
    pub fn observe(&self, allow_sensitive: bool) -> Result<Observation, ClipboardError> {
        unsafe {
            let _lease = Lease::open(self.hwnd as HWND)?;
            let sequence = GetClipboardSequenceNumber();
            self.expected.set(Some(sequence));
            if self.observed.get() == Some(sequence) {
                return Ok(Observation {
                    value: None,
                    marker: None,
                    unchanged: true,
                });
            }
            let marker = self.marker()?;
            // Windows can advance the sequence when CloseClipboard completes.
            // Match the owner HWND and our unique origin marker under the lease,
            // then remember the final observed sequence for duplicate notices.
            let own = GetClipboardOwner() == self.hwnd as HWND
                && self
                    .written
                    .borrow()
                    .as_deref()
                    .is_some_and(|id| marker.as_deref() == Some(id));
            self.written.take();
            if own {
                self.observed.set(Some(sequence));
                return Ok(Observation {
                    value: None,
                    marker,
                    unchanged: true,
                });
            }
            let sensitive = IsClipboardFormatAvailable(self.formats.exclude) != 0
                || flag_is_zero(self.formats.history)?
                || flag_is_zero(self.formats.cloud)?;
            if sensitive && !allow_sensitive {
                return Err(ClipboardError::Sensitive);
            }
            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                self.observed.set(Some(sequence));
                return Ok(Observation {
                    value: None,
                    marker: None,
                    unchanged: false,
                });
            }
            let text = read_text()?;
            self.observed.set(Some(sequence));
            Ok(Observation {
                value: Some((text, sensitive)),
                marker,
                unchanged: false,
            })
        }
    }
    unsafe fn marker(&self) -> Result<Option<String>, ClipboardError> {
        if IsClipboardFormatAvailable(self.formats.origin) == 0 {
            return Ok(None);
        }
        let bytes = read_bytes(self.formats.origin, 64)?;
        Ok(bytes
            .iter()
            .position(|b| *b == 0)
            .and_then(|end| std::str::from_utf8(&bytes[..end]).ok())
            .map(str::to_owned))
    }
}
impl Clipboard for WindowsClipboard {
    fn snapshot(&self, allow: bool) -> Result<Option<(String, bool)>, ClipboardError> {
        // A receiver must be able to compare a clipboard it wrote itself too.
        let saved = self.written.take();
        let observed = self.observed.take();
        let value = self.observe(allow).map(|v| v.value);
        self.written.replace(saved);
        self.observed.set(observed);
        value
    }
    fn write(&self, text: &str, message_id: &str, sensitive: bool) -> Result<(), ClipboardError> {
        unsafe {
            if text.len() > MAX_TEXT_BYTES || text.contains('\0') {
                return Err(ClipboardError::TooLarge);
            }
            if sensitive {
                return Err(ClipboardError::Sensitive);
            } // Private inbox is a later core capability.
            if message_id.len() != 36
                || !message_id
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || b == b'-')
            {
                return Err(ClipboardError::Platform);
            }
            let units: Vec<_> = text.encode_utf16().chain([0]).collect();
            let mut content = Allocation::from_bytes(std::slice::from_raw_parts(
                units.as_ptr().cast(),
                units.len() * 2,
            ))?;
            let mut marker = Allocation::from_bytes(format!("{message_id}\0").as_bytes())?;
            let _lease = Lease::open(self.hwnd as HWND)?;
            // This check runs while holding the OS clipboard lock, closing the race
            // between the core's last snapshot and EmptyClipboard.
            if self.expected.get() != Some(GetClipboardSequenceNumber()) {
                return Err(ClipboardError::Unavailable);
            }
            if EmptyClipboard() == 0 {
                return Err(last_error());
            }
            // Publish the marker first. Partial writes are never reported applied.
            marker.publish(self.formats.origin)?;
            content.publish(CF_UNICODETEXT)?;
            self.written.replace(Some(message_id.to_owned()));
            Ok(())
        }
    }
}
struct Lease;
impl Lease {
    unsafe fn open(hwnd: HWND) -> Result<Self, ClipboardError> {
        let end = Instant::now() + Duration::from_millis(200);
        loop {
            if OpenClipboard(hwnd) != 0 {
                return Ok(Self);
            }
            if Instant::now() >= end {
                return Err(ClipboardError::Timeout);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn last_error() -> ClipboardError {
    ClipboardError::Io(io::Error::last_os_error())
}
unsafe fn read_bytes(format: u32, max: usize) -> Result<Vec<u8>, ClipboardError> {
    let memory = GetClipboardData(format);
    if memory.is_null() {
        return Err(last_error());
    }
    let size = GlobalSize(memory);
    if size == 0 || size > max {
        return Err(ClipboardError::TooLarge);
    }
    let data = GlobalLock(memory);
    if data.is_null() {
        return Err(last_error());
    }
    let bytes = std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec();
    GlobalUnlock(memory);
    Ok(bytes)
}
unsafe fn read_text() -> Result<String, ClipboardError> {
    let memory = GetClipboardData(CF_UNICODETEXT);
    if memory.is_null() {
        return Err(last_error());
    }
    let size = GlobalSize(memory);
    if size == 0 || size > (MAX_TEXT_BYTES + 1) * 2 {
        return Err(ClipboardError::TooLarge);
    }
    if !size.is_multiple_of(2) {
        return Err(ClipboardError::InvalidUtf8);
    }
    let data = GlobalLock(memory);
    if data.is_null() {
        return Err(last_error());
    }
    // GlobalAlloc returns suitably aligned memory. Decode under the clipboard
    // lease and retain only one bounded UTF-8 string, no full byte/UTF-16 copies.
    let result = decode_unicode(std::slice::from_raw_parts(data.cast::<u16>(), size / 2));
    GlobalUnlock(memory);
    result
}
unsafe fn flag_is_zero(format: u32) -> Result<bool, ClipboardError> {
    if IsClipboardFormatAvailable(format) == 0 {
        return Ok(false);
    }
    let bytes = read_bytes(format, 64)?;
    if bytes.len() < mem::size_of::<u32>() {
        return Ok(true);
    }
    Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()) == 0)
}
struct Allocation(HANDLE);
impl Allocation {
    unsafe fn from_bytes(bytes: &[u8]) -> Result<Self, ClipboardError> {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return Err(last_error());
        }
        let allocation = Self(handle);
        let memory = GlobalLock(handle);
        if memory.is_null() {
            return Err(last_error());
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast(), bytes.len());
        GlobalUnlock(handle);
        Ok(allocation)
    }
    unsafe fn publish(&mut self, format: u32) -> Result<(), ClipboardError> {
        if SetClipboardData(format, self.0).is_null() {
            return Err(last_error());
        }
        self.0 = ptr::null_mut();
        Ok(())
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}
