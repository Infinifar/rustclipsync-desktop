//! Local-only, logon-scoped, framed named pipe. All I/O is overlapped/bounded.
use super::{runtime::Control, security::Security, wide, Handle};
use crate::ipc::{Request, Response, MAX_FRAME};
use std::{
    io, mem, ptr,
    sync::{atomic::Ordering, Arc},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::*, IO::*},
};
fn session_id() -> io::Result<u32> {
    let mut id = 0;
    unsafe {
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut id) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(id)
}
pub fn name() -> io::Result<String> {
    Ok(format!(
        "RustClipSync.{}.{}",
        &rustclipsync_core::hash(&super::security::user_sid()?)[..16],
        session_id()?
    ))
}
struct AsyncIo {
    event: Handle,
    overlap: OVERLAPPED,
}
impl AsyncIo {
    fn new() -> io::Result<Self> {
        unsafe {
            let event = Handle::new(CreateEventW(ptr::null(), 1, 0, ptr::null()))?;
            let mut overlap: OVERLAPPED = mem::zeroed();
            overlap.hEvent = event.0;
            Ok(Self { event, overlap })
        }
    }
    fn finish(
        &mut self,
        pipe: HANDLE,
        success: bool,
        deadline: Option<Instant>,
        control: &Control,
    ) -> io::Result<u32> {
        unsafe {
            if !success && GetLastError() != ERROR_IO_PENDING {
                return Err(io::Error::last_os_error());
            }
            if !success {
                loop {
                    if control.stop.load(Ordering::SeqCst)
                        || deadline.is_some_and(|t| Instant::now() >= t)
                    {
                        CancelIoEx(pipe, &self.overlap);
                        WaitForSingleObject(self.event.0, INFINITE);
                        let mut ignored = 0;
                        GetOverlappedResult(pipe, &self.overlap, &mut ignored, 0);
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "pipe deadline"));
                    }
                    match WaitForSingleObject(
                        self.event.0,
                        if deadline.is_some() { 100 } else { 1000 },
                    ) {
                        WAIT_OBJECT_0 => break,
                        WAIT_TIMEOUT => {}
                        _ => return Err(io::Error::last_os_error()),
                    }
                }
            }
            let mut count = 0;
            if GetOverlappedResult(pipe, &self.overlap, &mut count, 0) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(count)
        }
    }
}
fn transfer(pipe: HANDLE, bytes: &mut [u8], write: bool, control: &Control) -> io::Result<()> {
    let end = Instant::now() + Duration::from_secs(4);
    let mut offset = 0;
    while offset < bytes.len() {
        let mut op = AsyncIo::new()?;
        let len = (bytes.len() - offset) as u32;
        let success = unsafe {
            if write {
                WriteFile(
                    pipe,
                    bytes[offset..].as_ptr(),
                    len,
                    ptr::null_mut(),
                    &mut op.overlap,
                )
            } else {
                ReadFile(
                    pipe,
                    bytes[offset..].as_mut_ptr(),
                    len,
                    ptr::null_mut(),
                    &mut op.overlap,
                )
            }
        };
        let count = op.finish(pipe, success != 0, Some(end), control)? as usize;
        if count == 0 {
            return Err(io::Error::other("closed pipe"));
        }
        offset += count;
    }
    Ok(())
}
fn peer_allowed(pipe: HANDLE) -> bool {
    unsafe {
        let (mut pid, mut session) = (0, 0);
        if GetNamedPipeClientProcessId(pipe, &mut pid) == 0
            || GetNamedPipeClientSessionId(pipe, &mut session) == 0
            || session_id().ok() != Some(session)
        {
            return false;
        }
        let Ok(process) = Handle::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))
        else {
            return false;
        };
        let mut text = vec![0u16; 32768];
        let mut size = text.len() as u32;
        if QueryFullProcessImageNameW(process.0, 0, text.as_mut_ptr(), &mut size) == 0 {
            return false;
        }
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&text[..size as usize]));
        let Ok(agent) = std::env::current_exe() else {
            return false;
        };
        path.to_string_lossy()
            .eq_ignore_ascii_case(&agent.to_string_lossy())
            || path
                .to_string_lossy()
                .eq_ignore_ascii_case(&agent.with_file_name("RustClipSync.exe").to_string_lossy())
    }
}
pub struct Server {
    worker: Option<thread::JoinHandle<()>>,
    control: Arc<Control>,
}
impl Server {
    pub fn start(control: Arc<Control>) -> io::Result<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let shared = control.clone();
        let worker = thread::spawn(move || {
            let opened = (|| {
                let security = Security::current_logon()?;
                let attrs = security.attributes();
                let pipe = unsafe {
                    Handle::new(CreateNamedPipeW(
                        wide(&format!(r"\\.\pipe\{}", name()?)).as_ptr(),
                        PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                        PIPE_TYPE_BYTE
                            | PIPE_READMODE_BYTE
                            | PIPE_WAIT
                            | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        MAX_FRAME as u32,
                        MAX_FRAME as u32,
                        0,
                        &attrs,
                    ))?
                };
                Ok::<_, io::Error>(pipe)
            })();
            let pipe = match opened {
                Ok(p) => {
                    let _ = tx.send(Ok(()));
                    p
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            while !shared.stop.load(Ordering::SeqCst) {
                let Ok(mut op) = AsyncIo::new() else {
                    break;
                };
                let connected = unsafe {
                    let ok = ConnectNamedPipe(pipe.0, &mut op.overlap) != 0;
                    if !ok && GetLastError() == ERROR_PIPE_CONNECTED {
                        true
                    } else {
                        op.finish(pipe.0, ok, None, &shared).is_ok()
                    }
                };
                if !connected {
                    continue;
                }
                if peer_allowed(pipe.0) {
                    let _ = handle(pipe.0, &shared);
                }
                unsafe {
                    DisconnectNamedPipe(pipe.0);
                }
            }
        });
        rx.recv()
            .map_err(|_| io::Error::other("pipe worker stopped"))??;
        Ok(Self {
            worker: Some(worker),
            control,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.control.stop.store(true, Ordering::SeqCst);
        self.control.notify();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn handle(pipe: HANDLE, control: &Arc<Control>) -> io::Result<()> {
    let mut header = [0u8; 4];
    transfer(pipe, &mut header, false, control)?;
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::Error::other("pipe frame limit"));
    }
    let mut bytes = vec![0; size];
    transfer(pipe, &mut bytes, false, control)?;
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| io::Error::other("invalid control request"))?;
    let result = request
        .command()
        .and_then(|command| control.execute(command));
    let response = Response::new(request.id, result);
    let mut body = serde_json::to_vec(&response)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::other("pipe response limit"));
    }
    let mut length = (body.len() as u32).to_le_bytes();
    transfer(pipe, &mut length, true, control)?;
    transfer(pipe, &mut body, true, control)
}

/// CLI bridge used for local diagnostics and native protocol integration tests.
pub fn call(bytes: &[u8]) -> io::Result<serde_json::Value> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(io::Error::other("request size"));
    }
    let config = crate::config::Config::new("https://example.invalid".into(), "rpc".into());
    let (tx, _rx) = std::sync::mpsc::sync_channel(1);
    let control = Control::new(&config, tx);
    let pipe = unsafe {
        Handle::new(CreateFileW(
            wide(&format!(r"\\.\pipe\{}", name()?)).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        ))?
    };
    let mut header = (bytes.len() as u32).to_le_bytes();
    let mut body = bytes.to_vec();
    transfer(pipe.0, &mut header, true, &control)?;
    transfer(pipe.0, &mut body, true, &control)?;
    transfer(pipe.0, &mut header, false, &control)?;
    let count = u32::from_le_bytes(header) as usize;
    if count == 0 || count > MAX_FRAME {
        return Err(io::Error::other("response size"));
    }
    let mut result = vec![0; count];
    transfer(pipe.0, &mut result, false, &control)?;
    serde_json::from_slice(&result).map_err(io::Error::other)
}
