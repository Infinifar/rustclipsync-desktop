use super::{
    clipboard::WindowsClipboard,
    history::LocalHistory,
    storage::{self, FileStateStore},
};
use crate::{config::Config, ipc::Command};
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError, NoHistory},
    client::Client,
    session::{Session, SessionConfig, SyncError},
    worker::{Event, Worker},
    Error, Filter, FilterMode, Outcome,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
pub const STATUS_CHANGED: u32 = WM_APP + 2;
pub const WORKER_STOPPED: u32 = WM_APP + 3;
struct Job {
    command: Command,
    reply: SyncSender<Result<Value, &'static str>>,
    deadline: Instant,
}
struct Snapshot {
    config: Config,
    revision: u64,
    device: String,
    cursor: u64,
    pending: bool,
    last_sync: u64,
    history_error: bool,
    inbox: Option<Value>,
}
pub struct Control {
    pub stop: AtomicBool,
    pub paused: AtomicBool,
    pub generation: Arc<AtomicU64>,
    pub hwnd: AtomicUsize,
    pub status: Mutex<&'static str>,
    pub wake: SyncSender<(u64, Event)>,
    snapshot: Mutex<Snapshot>,
    commands: Mutex<VecDeque<Job>>,
    fatal: AtomicBool,
}
impl Control {
    pub fn new(config: &Config, wake: SyncSender<(u64, Event)>) -> Self {
        Self {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(config.paused),
            generation: Arc::new(AtomicU64::new(0)),
            hwnd: AtomicUsize::new(0),
            status: Mutex::new("connecting"),
            wake,
            snapshot: Mutex::new(Snapshot {
                config: config.clone(),
                revision: 1,
                device: String::new(),
                cursor: 0,
                pending: false,
                last_sync: 0,
                history_error: false,
                inbox: None,
            }),
            commands: Mutex::new(VecDeque::new()),
            fatal: AtomicBool::new(false),
        }
    }
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
    fn status_value(&self) -> Result<Value, &'static str> {
        let s = self.snapshot.lock().map_err(|_| "unavailable")?;
        Ok(
            json!({"state":*self.status.lock().map_err(|_|"unavailable")?,"paused":self.paused.load(Ordering::SeqCst),"config":s.config,"configRevision":s.revision,"deviceId":s.device,"cursor":s.cursor,"pending":s.pending,"lastSync":s.last_sync,"historyError":s.history_error,"inbox":s.inbox,"version":env!("CARGO_PKG_VERSION")}),
        )
    }
    pub fn execute(&self, command: Command) -> Result<Value, &'static str> {
        match command {
            Command::GetStatus {} => return self.status_value(),
            Command::TestFilter { filter, text } => {
                let f = filter.compile().map_err(|_| "invalid_filter")?;
                return Ok(
                    json!({"matched":f.matches(&text),"uploadAllowed":f.allows(&text,false),"downloadAllowed":f.allows(&text,true)}),
                );
            }
            Command::ExportDiagnostics {} => {
                let s = self.snapshot.lock().map_err(|_| "unavailable")?;
                return Ok(
                    json!({"version":env!("CARGO_PKG_VERSION"),"coreVersion":"0.2.0","state":*self.status.lock().map_err(|_|"unavailable")?,"paused":self.paused.load(Ordering::SeqCst),"pending":s.pending,"historyError":s.history_error}),
                );
            }
            Command::Quit {} => return Ok(json!({})),
            _ => {}
        }
        if self.fatal.load(Ordering::SeqCst) {
            return Err("needs_restart");
        }
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut jobs = self.commands.lock().map_err(|_| "unavailable")?;
            if jobs.len() >= 16 {
                return Err("busy");
            }
            jobs.push_back(Job {
                command,
                reply: tx,
                deadline: Instant::now() + Duration::from_secs(55),
            });
        }
        self.notify();
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| "operation_unconfirmed")?
    }
    fn refresh(&self, config: &Config, session: &LiveSession, outcome: Option<&Outcome>) {
        if let Ok(mut s) = self.snapshot.lock() {
            s.config = config.clone();
            s.device = session.persisted.state.device_id.clone();
            s.cursor = session.persisted.state.cursor;
            s.pending = session.has_pending();
            s.history_error = session.history_error || session.history.failed();
            s.inbox = session
                .persisted
                .inbox
                .as_ref()
                .map(|i| json!({"id":i.id,"receivedAt":i.received_at,"sensitive":true}));
            if matches!(
                outcome,
                Some(Outcome::Uploaded | Outcome::Downloaded | Outcome::Stored)
            ) {
                s.last_sync = unix_time();
            }
        }
    }
}
type LiveSession = Session<WindowsClipboard, FileStateStore, LocalHistory>;
struct PairingClipboard;
impl Clipboard for PairingClipboard {
    fn snapshot(&self, _: bool) -> Result<Option<(String, bool)>, ClipboardError> {
        Ok(None)
    }
    fn write(&self, _: &str, _: &str, _: bool) -> Result<(), ClipboardError> {
        Err(ClipboardError::Unavailable)
    }
}
pub fn pair(dir: &Path, config: &Config, password: String, reset: bool) -> Result<(), SyncError> {
    let identity = config.identity();
    let mut session = Session::open(
        SessionConfig {
            server: config.server.clone(),
            encryption_key: storage::load_key(dir, &identity)?,
            filter: Filter::new(FilterMode::Off, &[], false).expect("empty filter"),
            allow_sensitive: false,
        },
        Arc::new(Client::new(
            &config.server,
            config.user.clone(),
            password,
            false,
        )?),
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
    dir: &Path,
    config: &Config,
    clipboard: WindowsClipboard,
    generation: Arc<AtomicU64>,
) -> Result<LiveSession, SyncError> {
    let identity = config.identity();
    let key = storage::load_key(dir, &identity)?;
    let history = LocalHistory::open(
        dir.join(format!("{identity}.history")),
        key,
        config.history.clone(),
    );
    let mut session = Session::open(
        SessionConfig {
            server: config.server.clone(),
            encryption_key: key,
            filter: config.filter.compile()?,
            allow_sensitive: config.allow_sensitive,
        },
        Arc::new(Client::new(
            &config.server,
            config.user.clone(),
            String::new(),
            false,
        )?),
        FileStateStore(dir.join(format!("{identity}.state"))),
        clipboard,
        history,
        generation,
    )?;
    session.use_sensitive_inbox(true);
    Ok(session)
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
    if let Err(error) = run_inner(&dir, &mut config, clipboard, &control, events) {
        control.fatal.store(true, Ordering::SeqCst);
        control.set_status(classify(&error));
        while !control.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
fn run_inner(
    dir: &Path,
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
    control.refresh(config, &session, None);
    let mut worker = None;
    let mut seen = 0;
    let mut next = Instant::now();
    let mut retry = 1;
    let mut blocked = false;
    let mut cleanup = Instant::now();
    while !control.stop.load(Ordering::SeqCst) {
        let paused = control.paused.load(Ordering::SeqCst);
        if config.paused != paused {
            session.record_clear()?;
            seen = control.generation.load(Ordering::SeqCst);
            config.paused = paused;
            save_config(dir, config)?;
            if let Ok(mut s) = control.snapshot.lock() {
                s.revision += 1;
            }
            next = Instant::now();
        }
        loop {
            let job = control
                .commands
                .lock()
                .map_err(|_| io::Error::other("control queue"))?
                .pop_front();
            let Some(job) = job else {
                break;
            };
            if job.deadline <= Instant::now() {
                let _ = job.reply.send(Err("operation_unconfirmed"));
                continue;
            }
            let result = command(job.command, dir, config, &mut session, control);
            match &result {
                Ok(_) => {
                    if matches!(
                        *control
                            .status
                            .lock()
                            .map_err(|_| io::Error::other("status"))?,
                        "connected"
                    ) {
                        blocked = false;
                    }
                    next = Instant::now();
                }
                Err(error) => {
                    let state = classify(error);
                    if matches!(state, "conflict" | "expired" | "re_pair" | "credentials") {
                        blocked = true;
                        control.set_status(state);
                    }
                }
            }
            control.refresh(config, &session, None);
            let fatal = matches!(result, Err(SyncError::Storage(_)));
            let _ = job.reply.send(result.map_err(|e| classify(&e)));
            if fatal {
                return Err(SyncError::Storage(io::Error::other(
                    "control storage failure",
                )));
            }
        }
        let paused = control.paused.load(Ordering::SeqCst);
        let generation = control.generation.load(Ordering::SeqCst);
        if generation != seen {
            seen = generation;
            if !paused && config.uploads() {
                match session.clipboard.observe(config.allow_sensitive) {
                    Ok(o) if o.unchanged => {}
                    Ok(o) => {
                        if let Some((text, sensitive)) = o.value {
                            session.record_local(
                                &text,
                                o.marker.as_deref(),
                                sensitive,
                                Instant::now(),
                            )?;
                        } else {
                            session.record_clear()?;
                        }
                    }
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
        if Instant::now() >= cleanup {
            let now = unix_time();
            let _ = session.history.change(|data, cfg| {
                data.prune(cfg, now);
                Ok(())
            });
            if session
                .persisted
                .inbox
                .as_ref()
                .is_some_and(|i| i.received_at < now.saturating_sub(config.history.days * 86400))
            {
                let id = session.persisted.inbox.as_ref().unwrap().id.clone();
                session.clear_inbox(&id)?;
            }
            cleanup = Instant::now() + Duration::from_secs(60);
        }
        if paused && !blocked {
            control.set_status("paused");
        } else if !paused && !blocked && Instant::now() >= next {
            let observed = seen;
            let operation = (|| {
                session.check()?;
                if control.stop.load(Ordering::SeqCst) {
                    return Ok(Outcome::Current);
                }
                let uploaded = session.flush(observed)?;
                control.refresh(config, &session, Some(&uploaded));
                let downloaded = if config.downloads() && !control.stop.load(Ordering::SeqCst) {
                    session.pull(observed, Instant::now())?
                } else {
                    Outcome::Current
                };
                control.refresh(config, &session, Some(&downloaded));
                Ok(downloaded)
            })();
            match operation {
                Ok(_) => {
                    retry = 1;
                    control.set_status("connected");
                    next = Instant::now() + Duration::from_secs(config.interval_sec);
                    if worker.is_none() && config.downloads() {
                        worker = Some(Worker::start(
                            session.remote.clone(),
                            session.persisted.notification_identity(),
                            0,
                            control.wake.clone(),
                        ));
                    }
                    if !config.downloads() {
                        drop(worker.take());
                    }
                    if let Some(worker) = &worker {
                        worker.update(&session.persisted);
                    }
                    if session.has_pending() {
                        next = Instant::now() + Duration::from_millis(250);
                    }
                }
                Err(error) => {
                    let state = classify(&error);
                    control.set_status(state);
                    if state == "storage_error" {
                        return Err(error);
                    }
                    if matches!(state, "conflict" | "expired" | "re_pair" | "credentials") {
                        blocked = true;
                    }
                    if matches!(error, SyncError::Clipboard(_)) {
                        drop(worker.take());
                        session =
                            open_live(dir, config, session.clipboard, control.generation.clone())?;
                    }
                    next = Instant::now() + Duration::from_secs(retry);
                    retry = (retry * 2).min(30);
                }
            }
        }
        control.refresh(config, &session, None);
        let timeout = if paused || blocked {
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
fn save_config(dir: &Path, config: &Config) -> Result<(), SyncError> {
    storage::atomic_write(
        &dir.join("config.json"),
        &serde_json::to_vec_pretty(config).map_err(io::Error::other)?,
    )?;
    Ok(())
}
fn command(
    cmd: Command,
    dir: &Path,
    config: &mut Config,
    s: &mut LiveSession,
    control: &Control,
) -> Result<Value, SyncError> {
    let observed = control.generation.load(Ordering::SeqCst);
    let invalid = || SyncError::Remote(Error::InvalidProfile);
    match cmd {
        Command::SetPaused { paused } => {
            s.record_clear()?;
            let mut updated = config.clone();
            updated.paused = paused;
            save_config(dir, &updated)?;
            *config = updated;
            control.paused.store(paused, Ordering::SeqCst);
            if let Ok(mut v) = control.snapshot.lock() {
                v.revision += 1;
            }
            if !paused {
                control.set_status("connecting");
            }
            Ok(json!({}))
        }
        Command::UpdateSettings {
            config: mut updated,
            expected_revision,
        } => {
            updated.validate().map_err(|_| invalid())?;
            if updated.identity() != config.identity() {
                return Err(invalid());
            }
            if control.snapshot.lock().map_err(|_| invalid())?.revision != expected_revision {
                return Err(SyncError::Remote(Error::Status(412)));
            }
            updated.paused = config.paused;
            if updated.direction != config.direction
                || updated.allow_sensitive != config.allow_sensitive
                || serde_json::to_value(&updated.filter).ok()
                    != serde_json::to_value(&config.filter).ok()
            {
                s.record_clear()?;
            }
            save_config(dir, &updated)?;
            s.update_policy(updated.filter.compile()?, updated.allow_sensitive);
            s.history.configure(updated.history.clone());
            *config = updated;
            if let Ok(mut v) = control.snapshot.lock() {
                v.revision += 1;
            }
            Ok(json!({}))
        }
        Command::PushNow {} | Command::PullNow {} => {
            let outcome = if matches!(cmd, Command::PushNow {}) {
                s.push_now(observed)?
            } else {
                s.record_clear()?;
                s.flush(observed)?;
                s.pull(observed, Instant::now())?
            };
            control.set_status(if config.paused { "paused" } else { "connected" });
            control.refresh(config, s, Some(&outcome));
            Ok(json!({"outcome":format!("{outcome:?}").to_lowercase()}))
        }
        Command::ListHistory {
            offset,
            query,
            favorites,
        } => {
            s.history
                .change(|data, cfg| {
                    data.prune(cfg, unix_time());
                    Ok(())
                })
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            let data = s
                .history
                .data()
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            let query = query.to_lowercase();
            let entries: Vec<_> = data
                .entries
                .iter()
                .filter(|e| {
                    (!favorites || e.favorite)
                        && (query.is_empty() || e.text.to_lowercase().contains(&query))
                })
                .collect();
            let items:Vec<_>=entries.iter().skip(offset).take(20).map(|e|json!({"id":e.id,"preview":e.text.chars().take(240).collect::<String>(),"timestamp":e.timestamp,"direction":e.direction,"favorite":e.favorite})).collect();
            Ok(json!({"total":entries.len(),"items":items}))
        }
        Command::CopyHistory { id } => {
            let data = s
                .history
                .data()
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            let item = data
                .entries
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(invalid)?;
            s.clipboard
                .copy_saved(&item.text, &rustclipsync_core::state::random_uuid()?, false)
                .map_err(SyncError::Clipboard)?;
            Ok(json!({}))
        }
        Command::DeleteHistory { id } => {
            s.history
                .change(|d, _| {
                    d.entries.retain(|e| e.id != id);
                    Ok(())
                })
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            Ok(json!({}))
        }
        Command::SetFavorite { id, favorite } => {
            s.history
                .change(|d, _| d.favorite(&id, favorite))
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            Ok(json!({}))
        }
        Command::ClearHistory {} => {
            s.history
                .clear()
                .map_err(|_| SyncError::Remote(Error::Status(422)))?;
            Ok(json!({}))
        }
        Command::CopyInbox { id } => {
            let item = s
                .persisted
                .inbox
                .as_ref()
                .filter(|i| i.id == id)
                .ok_or_else(invalid)?;
            s.clipboard
                .copy_saved(&item.text, &item.id, true)
                .map_err(SyncError::Clipboard)?;
            Ok(json!({}))
        }
        Command::ClearInbox { id } => Ok(json!({"cleared":s.clear_inbox(&id)?})),
        _ => Err(invalid()),
    }
}
pub fn classify(error: &SyncError) -> &'static str {
    match error {
        SyncError::Storage(_) => "storage_error",
        SyncError::Credentials | SyncError::Remote(Error::Status(401 | 403)) => "credentials",
        SyncError::Conflict | SyncError::Remote(Error::Status(409)) => "conflict",
        SyncError::Expired => "expired",
        SyncError::Epoch => "re_pair",
        SyncError::Clipboard(_) => "clipboard_error",
        SyncError::Remote(Error::InvalidProfile) => "invalid_request",
        SyncError::Remote(Error::Status(412)) => "settings_changed",
        SyncError::Remote(Error::Status(422)) => "history_error",
        _ => "offline",
    }
}
fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
