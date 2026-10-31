//! Same JSON command contract as Windows, over a private Unix socket (no TCP).
use super::storage;
use crate::{
    ipc::{Request, Response, MAX_FRAME},
    runtime::Control,
};
use std::{
    fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub fn socket_path(dir: &Path) -> io::Result<PathBuf> {
    let runtime = PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is required in a desktop session"))?,
    );
    let m = fs::symlink_metadata(&runtime)?;
    if !runtime.is_absolute()
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
    {
        return Err(io::Error::other("invalid private XDG_RUNTIME_DIR"));
    }
    let base = runtime.join("rustclipsync-desktop");
    storage::ensure_private_dir(&base)?;
    let path = base.join(format!(
        "{}.sock",
        &rustclipsync_core::hash(&dir.to_string_lossy())[..24]
    ));
    if path.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err(io::Error::other("runtime socket path too long"));
    }
    Ok(path)
}
fn same_user(stream: &UnixStream) -> io::Result<()> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    } != 0
        || credentials.uid != unsafe { libc::geteuid() }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "foreign IPC peer",
        ));
    }
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(4)))
}
pub fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::other("IPC frame limit"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}
pub fn write_frame(stream: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(io::Error::other("IPC frame limit"));
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(bytes)
}
pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}
impl Server {
    /// Caller holds the profile lock before removing a stale socket.
    pub fn start(dir: &Path, control: Arc<Control>) -> io::Result<Self> {
        let path = socket_path(dir)?;
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_socket() && m.uid() == unsafe { libc::geteuid() } => {
                fs::remove_file(&path)?
            }
            Ok(_) => return Err(io::Error::other("refusing to replace non-socket IPC path")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if same_user(&stream).is_err() || active.load(Ordering::SeqCst) >= 4 {
                            continue;
                        }
                        active.fetch_add(1, Ordering::SeqCst);
                        let active = active.clone();
                        let control = control.clone();
                        thread::spawn(move || {
                            let _ = serve(&mut stream, &control);
                            active.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            path,
        })
    }
}
fn serve(stream: &mut UnixStream, control: &Control) -> io::Result<()> {
    // Bound the entire incoming frame, including a peer sending one byte at a time.
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut read_exact = |bytes: &mut [u8]| -> io::Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "IPC read deadline"));
            }
            stream.set_read_timeout(Some(left))?;
            match stream.read(&mut bytes[offset..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "closed IPC frame",
                    ))
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    };
    let mut header = [0; 4];
    read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::other("IPC frame limit"));
    }
    let mut bytes = vec![0; length];
    read_exact(&mut bytes)?;
    let request: Request = serde_json::from_slice(&bytes)?;
    let command = request.command();
    let quit = command
        .as_ref()
        .is_ok_and(|c| matches!(c, crate::ipc::Command::Quit {}));
    let result = command.and_then(|cmd| control.execute(cmd));
    let response = serde_json::to_vec(&Response::new(request.id, result))?;
    write_frame(stream, &response)?;
    if quit {
        control.stop.store(true, Ordering::SeqCst);
        control.notify();
    }
    Ok(())
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}
pub fn call(dir: &Path, bytes: &[u8]) -> io::Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket_path(dir)?)?;
    same_user(&stream)?;
    // A sync operation may wait on HTTPS; this bound matches the command deadline.
    stream.set_read_timeout(Some(Duration::from_secs(65)))?;
    write_frame(&mut stream, bytes)?;
    Ok(serde_json::from_slice(&read_frame(&mut stream)?)?)
}
