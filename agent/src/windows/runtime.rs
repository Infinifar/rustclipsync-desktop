use super::{
    clipboard::WindowsClipboard,
    storage::{self, FileStateStore},
};
use crate::config::Config;
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError, NoHistory},
    client::Client,
    session::{Session, SessionConfig, SyncError},
    worker::{Event, Worker},
    Error, Filter, FilterMode,
};
use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
pub const STATUS_CHANGED: u32 = WM_APP + 2;
pub const WORKER_STOPPED: u32 = WM_APP + 3;
pub struct Control {
    pub stop: AtomicBool,
    pub paused: AtomicBool,
    pub generation: Arc<AtomicU64>,
    pub hwnd: AtomicUsize,
    pub status: Mutex<&'static str>,
    pub wake: SyncSender<(u64, Event)>,
}
impl Control {
    pub fn notify(&self) {
        let _ = self.wake.try_send((0, Event::Changed));
    }
    pub fn set_status(&self, status: &'static str) {
        if let Ok(mut current) = self.status.lock() {
            if *current == status {
                return;
            }
            *current = status;
        }
        unsafe {
            PostMessageW(self.hwnd.load(Ordering::SeqCst) as _, STATUS_CHANGED, 0, 0);
        }
    }
}
type LiveSession = Session<WindowsClipboard, FileStateStore, NoHistory>;
struct PairingClipboard;
impl Clipboard for PairingClipboard {
    fn snapshot(&self, _: bool) -> Result<Option<(String, bool)>, ClipboardError> {
        Ok(None)
    }
    fn write(&self, _: &str, _: &str, _: bool) -> Result<(), ClipboardError> {
        Err(ClipboardError::Unavailable)
    }
}
pub fn pair(
    dir: &std::path::Path,
    config: &Config,
    password: String,
    reset: bool,
) -> Result<(), SyncError> {
    let identity = config.identity();
    let key = storage::load_key(dir, &identity)?;
    let remote = Arc::new(Client::new(
        &config.server,
        config.user.clone(),
        password,
        false,
    )?);
    let mut session = Session::open(
        SessionConfig {
            server: config.server.clone(),
            encryption_key: key,
            filter: Filter::new(FilterMode::Off, &[], false).expect("empty filter"),
            allow_sensitive: false,
        },
        remote,
        FileStateStore(dir.join(format!("{identity}.state"))),
        PairingClipboard,
        NoHistory,
        Arc::new(AtomicU64::new(0)),
    )?;
    if reset {
        session.re_pair()
    } else {
        session.check()
    }
}
fn open_live(
    dir: &std::path::Path,
    config: &Config,
    clipboard: WindowsClipboard,
    generation: Arc<AtomicU64>,
) -> Result<LiveSession, SyncError> {
    let identity = config.identity();
    let key = storage::load_key(dir, &identity)?;
    let remote = Arc::new(Client::new(
        &config.server,
        config.user.clone(),
        String::new(),
        false,
    )?);
    Session::open(
        SessionConfig {
            server: config.server.clone(),
            encryption_key: key,
            filter: Filter::new(FilterMode::Off, &[], false).expect("empty filter"),
            allow_sensitive: false,
        },
        remote,
        FileStateStore(dir.join(format!("{identity}.state"))),
        clipboard,
        NoHistory,
        generation,
    )
}
pub fn run(
    dir: PathBuf,
    mut config: Config,
    clipboard: WindowsClipboard,
    control: Arc<Control>,
    events: Receiver<(u64, Event)>,
) {
    struct Completion(Arc<Control>);
    impl Drop for Completion {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(
                    self.0.hwnd.load(Ordering::SeqCst) as _,
                    WORKER_STOPPED,
                    0,
                    0,
                );
            }
        }
    }
    let _completion = Completion(control.clone());
    let result = run_inner(&dir, &mut config, clipboard, &control, events);
    if let Err(error) = result {
        control.set_status(classify(&error));
        // Fail closed, keep the tray alive so the user can see the error/exit.
        while !control.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
fn run_inner(
    dir: &std::path::Path,
    config: &mut Config,
    clipboard: WindowsClipboard,
    control: &Control,
    events: Receiver<(u64, Event)>,
) -> Result<(), SyncError> {
    let mut session = open_live(dir, config, clipboard, control.generation.clone())?;
    if !session.persisted.paired {
        return Err(SyncError::Credentials);
    }
    if !config.uploads() || config.paused {
        session.record_clear()?;
    }
    let mut worker = None;
    let mut seen = 0;
    let mut next = Instant::now();
    let mut retry = 1;
    let mut was_paused = config.paused;
    while !control.stop.load(Ordering::SeqCst) {
        let paused = control.paused.load(Ordering::SeqCst);
        if was_paused != paused {
            session.record_clear()?;
            seen = control.generation.load(Ordering::SeqCst);
            config.paused = paused;
            storage::atomic_write(
                &dir.join("config.json"),
                &serde_json::to_vec_pretty(config).map_err(io::Error::other)?,
            )?;
            was_paused = paused;
            next = Instant::now();
        }
        let generation = control.generation.load(Ordering::SeqCst);
        if generation != seen {
            seen = generation;
            if !paused && config.uploads() {
                match session.clipboard.observe(false) {
                    Ok(observation) if observation.unchanged => {}
                    Ok(observation) => match observation.value {
                        Some((text, sensitive)) => {
                            session.record_local(
                                &text,
                                observation.marker.as_deref(),
                                sensitive,
                                Instant::now(),
                            )?;
                        }
                        None => {
                            session.record_clear()?;
                        }
                    },
                    Err(ClipboardError::Sensitive) => {
                        session.record_clear()?;
                    }
                    Err(error) => {
                        session.record_clear()?;
                        control.set_status("clipboard_error");
                        if matches!(
                            error,
                            ClipboardError::Timeout
                                | ClipboardError::Unavailable
                                | ClipboardError::Io(_)
                        ) {
                            seen = generation.wrapping_sub(1);
                            let _ = events.recv_timeout(Duration::from_millis(250));
                            continue;
                        }
                    }
                }
            }
        }
        if control.generation.load(Ordering::SeqCst) != seen {
            continue;
        }
        if paused {
            control.set_status("paused");
        } else if Instant::now() >= next {
            let observed = seen;
            let operation = (|| {
                session.check()?;
                if control.stop.load(Ordering::SeqCst) {
                    return Ok(());
                }
                session.flush(observed)?;
                if config.downloads() && !control.stop.load(Ordering::SeqCst) {
                    session.pull(observed, Instant::now())?;
                }
                Ok(())
            })();
            match operation {
                Ok(()) => {
                    retry = 1;
                    control.set_status("connected");
                    next = Instant::now() + Duration::from_secs(30);
                    if worker.is_none() && config.downloads() {
                        worker = Some(Worker::start(
                            session.remote.clone(),
                            session.persisted.notification_identity(),
                            0,
                            control.wake.clone(),
                        ));
                    }
                    if let Some(worker) = &worker {
                        worker.update(&session.persisted);
                    }
                    // Queued replacement/cancellation can need another serial step.
                    if session.has_pending() {
                        next = Instant::now() + Duration::from_millis(250);
                    }
                }
                Err(error) => {
                    let state = classify(&error);
                    control.set_status(state);
                    if matches!(
                        state,
                        "storage_error" | "conflict" | "expired" | "re_pair" | "credentials"
                    ) {
                        return Err(error);
                    }
                    if matches!(error, SyncError::Clipboard(_)) {
                        // Recover the durable applying journal rather than continuing
                        // with an ambiguous in-memory application forever.
                        drop(worker.take());
                        session =
                            open_live(dir, config, session.clipboard, control.generation.clone())?;
                    }
                    next = Instant::now() + Duration::from_secs(retry);
                    retry = (retry * 2).min(30);
                }
            }
        }
        let timeout = if paused {
            Duration::from_secs(30)
        } else {
            next.saturating_duration_since(Instant::now())
                .min(Duration::from_secs(30))
        };
        if events.recv_timeout(timeout).is_ok() && retry == 1 && !paused {
            next = Instant::now();
        }
    }
    drop(worker);
    Ok(())
}
pub fn classify(error: &SyncError) -> &'static str {
    match error {
        SyncError::Storage(_) => "storage_error",
        SyncError::Credentials | SyncError::Remote(Error::Status(401 | 403)) => "credentials",
        SyncError::Conflict | SyncError::Remote(Error::Status(409)) => "conflict",
        SyncError::Expired => "expired",
        SyncError::Epoch => "re_pair",
        SyncError::Clipboard(_) => "clipboard_error",
        _ => "offline",
    }
}
