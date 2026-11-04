use crate::{
    config::{Config, Shortcuts},
    ipc::Command,
};
use crate::{
    local_history::LocalHistory,
    platform::{
        self,
        clipboard::PlatformClipboard,
        storage::{self, FileStateStore},
    },
};
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
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
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
    history_revision: u64,
    history_epoch: String,
    inbox: Option<Value>,
}
pub struct Control {
    pub stop: AtomicBool,
    pub paused: AtomicBool,
    pub generation: Arc<AtomicU64>,
    pub signals: platform::Signals,
    pub status: Mutex<&'static str>,
    pub wake: SyncSender<(u64, Event)>,
    pub shortcut_error: AtomicBool,
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
            signals: platform::Signals::default(),
            status: Mutex::new("connecting"),
            wake,
            shortcut_error: AtomicBool::new(false),
            snapshot: Mutex::new(Snapshot {
                config: config.clone(),
                revision: 1,
                device: String::new(),
                cursor: 0,
                pending: false,
                last_sync: 0,
                history_error: false,
                history_revision: 0,
                history_epoch: String::new(),
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
        platform::notify(&self.signals, platform::Notification::StatusChanged);
    }
    fn status_value(&self) -> Result<Value, &'static str> {
        let s = self.snapshot.lock().map_err(|_| "unavailable")?;
        Ok(
            json!({"state":*self.status.lock().map_err(|_|"unavailable")?,"paused":self.paused.load(Ordering::SeqCst),"config":s.config,"configRevision":s.revision,"deviceId":s.device,"cursor":s.cursor,"pending":s.pending,"lastSync":s.last_sync,"historyError":s.history_error,"historyRevision":s.history_revision,"historyEpoch":s.history_epoch,"inbox":s.inbox,"shortcutError":self.shortcut_error.load(Ordering::SeqCst),"version":env!("CARGO_PKG_VERSION")}),
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
                    json!({"version":env!("CARGO_PKG_VERSION"),"coreVersion":"0.4.0","state":*self.status.lock().map_err(|_|"unavailable")?,"paused":self.paused.load(Ordering::SeqCst),"pending":s.pending,"historyError":s.history_error}),
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
    pub fn shortcuts(&self) -> Shortcuts {
        self.snapshot
            .lock()
            .map(|s| s.config.shortcuts.clone())
            .unwrap_or_default()
    }
    fn bind_shortcuts(&self, settings: &Shortcuts) -> Result<(), SyncError> {
        platform::bind_shortcuts(&self.signals, settings)
    }
    fn refresh(&self, config: &Config, session: &LiveSession, outcome: Option<&Outcome>) {
        if matches!(outcome, Some(Outcome::Stored)) {
            platform::inbox_received(&self.signals);
        }
        if let Ok(mut s) = self.snapshot.lock() {
            s.config = config.clone();
            s.device = session.persisted.state.device_id.clone();
            s.cursor = session.persisted.state.cursor;
            s.pending = session.has_pending();
            s.history_error = session.history_error || session.history.failed();
            s.history_revision = session.history.revision();
            s.history_epoch = session.history.epoch();
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
type LiveSession = Session<PlatformClipboard, FileStateStore, LocalHistory>;
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
    if reset {
        revoke_previous_pairing(dir)?;
    }
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
        session.re_pair_with_direction(config.server_direction(), config.paused)
    } else {
        session.set_sync_direction(config.server_direction(), config.paused)?;
        session.check()
    }
}
/// Explicit migration escape hatch: forget only this local pairing, never claim remote revocation.
/// The CLI requires a verbose confirmation flag and an exclusive agent-instance lock.
pub fn forget_local_pairing(dir: &Path) -> io::Result<()> {
    let config_path = dir.join("config.json");
    let Some(bytes) = storage::read_private(&config_path, 64 * 1024)? else {
        return Ok(());
    };
    let mut config: Config = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    config.validate()?;
    let state = dir.join(format!("{}.state", config.identity()));
    match std::fs::remove_file(state) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::remove_file(config_path)
}
// Explicit account replacement must finish revocation before discarding the old identity.
fn revoke_previous_pairing(dir: &Path) -> Result<(), SyncError> {
    let Some(bytes) = storage::read_private(&dir.join("config.json"), 64 * 1024)? else {
        return Ok(());
    };
    let mut previous: Config = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    previous.validate()?;
    let identity = previous.identity();
    let mut session = Session::open(
        SessionConfig {
            server: previous.server.clone(),
            encryption_key: storage::load_key(dir, &identity)?,
            filter: Filter::new(FilterMode::Off, &[], false).expect("empty filter"),
            allow_sensitive: false,
        },
        Arc::new(Client::new(
            &previous.server,
            previous.user.clone(),
            String::new(),
            false,
        )?),
        FileStateStore(dir.join(format!("{identity}.state"))),
        PairingClipboard,
        NoHistory,
        Arc::new(AtomicU64::new(0)),
    )?;
    session.revoke_sync_direction().map_err(|error| {
        if matches!(classify(&error), "protocol_unsupported" | "storage_error") {
            error
        } else {
            SyncError::Remote(Error::Status(424))
        }
    })
}
fn open_live(
    dir: &Path,
    config: &Config,
    clipboard: PlatformClipboard,
    control: &Control,
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
        control.generation.clone(),
    )?;
    receive_policy(&mut session, config);
    if !session.persisted.paired {
        return Err(SyncError::Credentials);
    }
    if !config.uploads() || config.paused {
        session.record_clear()?;
    }
    // Every newly constructed session needs its local policy even if its report
    // cannot reach the server yet. Core retries the report before data access.
    if let Err(error) = session.set_sync_direction(config.server_direction(), config.paused) {
        if matches!(error, SyncError::Storage(_)) {
            return Err(error);
        }
        control.set_status(direction_report_status(&error));
    }
    Ok(session)
}
fn receive_policy(session: &mut LiveSession, config: &Config) {
    let apply = config.applies_sensitive();
    session.clipboard.set_auto_apply_sensitive(apply);
    session.use_sensitive_inbox(!apply);
}
pub fn run(
    dir: PathBuf,
    mut config: Config,
    clipboard: PlatformClipboard,
    control: Arc<Control>,
    events: Receiver<(u64, Event)>,
) {
    struct Completion(Arc<Control>);
    impl Drop for Completion {
        fn drop(&mut self) {
            platform::notify(&self.0.signals, platform::Notification::WorkerStopped);
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
    clipboard: PlatformClipboard,
    control: &Control,
    events: Receiver<(u64, Event)>,
) -> Result<(), SyncError> {
    let mut session = open_live(dir, config, clipboard, control)?;
    control.refresh(config, &session, None);
    let mut worker = None;
    let mut seen = 0;
    let mut next = Instant::now();
    let mut retry = 1;
    let mut blocked = control.status.lock().is_ok_and(|state| {
        matches!(
            *state,
            "protocol_unsupported" | "direction_superseded" | "credentials"
        )
    });
    let mut cleanup = Instant::now();
    while !control.stop.load(Ordering::SeqCst) {
        let paused = control.paused.load(Ordering::SeqCst);
        if config.paused != paused {
            drop(worker.take());
            control.generation.fetch_add(1, Ordering::SeqCst);
            session.record_clear()?;
            seen = control.generation.load(Ordering::SeqCst);
            config.paused = paused;
            save_config(dir, config)?;
            if let Ok(mut s) = control.snapshot.lock() {
                s.revision += 1;
            }
            if let Err(error) = session.set_sync_direction(config.server_direction(), paused) {
                control.set_status(direction_report_status(&error));
                blocked = matches!(
                    classify(&error),
                    "protocol_unsupported" | "direction_superseded" | "credentials"
                );
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
            let direction_change = match &job.command {
                Command::SetPaused { .. } => true,
                Command::UpdateSettings {
                    config: updated, ..
                } => updated.direction != config.direction,
                _ => false,
            };
            if direction_change {
                drop(worker.take());
                // Policy changes invalidate in-flight work, not synthesize a clipboard copy.
                seen = control.generation.fetch_add(1, Ordering::SeqCst) + 1;
            }
            let result = command(job.command, dir, config, &mut session, control);
            match &result {
                Ok(_) => {
                    if direction_change {
                        blocked = false;
                        control.set_status(if config.paused {
                            "paused"
                        } else {
                            "connecting"
                        });
                    }
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
                    if matches!(
                        state,
                        "conflict"
                            | "expired"
                            | "re_pair"
                            | "credentials"
                            | "protocol_unsupported"
                            | "direction_superseded"
                    ) {
                        blocked = true;
                        control.set_status(state);
                    }
                }
            }
            control.refresh(config, &session, None);
            let fatal = matches!(result, Err(SyncError::Storage(_)));
            let _ = job.reply.send(result.map_err(|e| {
                if direction_change
                    && !matches!(
                        classify(&e),
                        "protocol_unsupported"
                            | "direction_superseded"
                            | "storage_error"
                            | "invalid_request"
                            | "settings_changed"
                            | "shortcut_conflict"
                    )
                {
                    "direction_unconfirmed"
                } else {
                    classify(&e)
                }
            }));
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
            let _ = session.history.prune(now);
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
            if Instant::now() >= next {
                let result = session
                    .set_sync_direction(config.server_direction(), true)
                    .and_then(|_| session.flush(seen).map(|_| ()));
                match result {
                    Ok(()) => control.set_status("paused"),
                    Err(error) => {
                        control.set_status(direction_report_status(&error));
                        blocked = matches!(
                            classify(&error),
                            "protocol_unsupported" | "direction_superseded" | "credentials"
                        );
                    }
                }
                next = Instant::now() + Duration::from_secs(config.interval_sec);
            }
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
                    if matches!(
                        state,
                        "conflict"
                            | "expired"
                            | "re_pair"
                            | "credentials"
                            | "protocol_unsupported"
                            | "direction_superseded"
                    ) {
                        blocked = true;
                    }
                    if matches!(error, SyncError::Clipboard(_)) {
                        // Core has already journaled uncertain writes. Retain its
                        // direction, pending work and echo state while retrying.
                        drop(worker.take());
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
            s.set_sync_direction(config.server_direction(), paused)?;
            Ok(json!({}))
        }
        Command::UpdateSettings {
            config: mut updated,
            expected_revision,
            rebind_shortcuts,
        } => {
            updated.validate().map_err(|_| invalid())?;
            if updated.identity() != config.identity() {
                return Err(invalid());
            }
            if control.snapshot.lock().map_err(|_| invalid())?.revision != expected_revision {
                return Err(SyncError::Remote(Error::Status(412)));
            }
            updated.paused = config.paused;
            let direction_changed = updated.direction != config.direction;
            if updated.direction != config.direction
                || updated.allow_sensitive != config.allow_sensitive
                || updated.auto_apply_sensitive != config.auto_apply_sensitive
                || serde_json::to_value(&updated.filter).ok()
                    != serde_json::to_value(&config.filter).ok()
            {
                s.record_clear()?;
            }
            let changed_shortcuts = updated.shortcuts != config.shortcuts || rebind_shortcuts;
            if changed_shortcuts {
                control.bind_shortcuts(&updated.shortcuts)?;
            }
            if let Err(error) = save_config(dir, &updated) {
                if changed_shortcuts {
                    control.bind_shortcuts(&config.shortcuts)?;
                }
                return Err(error);
            }
            s.update_policy(updated.filter.compile()?, updated.allow_sensitive);
            receive_policy(s, &updated);
            s.history.configure(updated.history.clone());
            *config = updated;
            if let Ok(mut v) = control.snapshot.lock() {
                v.revision += 1;
            }
            if direction_changed {
                s.set_sync_direction(config.server_direction(), config.paused)?;
            }
            Ok(json!({}))
        }
        Command::PushNow {} | Command::PullNow {} => {
            if !manual_allowed(config, matches!(cmd, Command::PushNow {})) {
                return Err(SyncError::Remote(Error::Status(405)));
            }
            s.check()?;
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
            if_epoch,
            if_revision,
        } => s
            .history
            .page(
                offset,
                &query,
                favorites,
                if_epoch.as_deref(),
                if_revision,
                unix_time(),
            )
            .map_err(|_| SyncError::Remote(Error::Status(422))),
        Command::CopyHistory { id } => {
            let text = s
                .history
                .text(&id)
                .map_err(|_| SyncError::Remote(Error::Status(422)))?
                .ok_or_else(invalid)?;
            s.clipboard
                .copy_saved(&text, &rustclipsync_core::state::random_uuid()?, false)
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
        Command::ReadInbox { id, offset } => {
            let item = s.persisted.inbox.as_ref().ok_or_else(invalid)?;
            crate::inbox::read_chunk(item, &id, offset).map_err(|_| invalid())
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
fn manual_allowed(config: &Config, upload: bool) -> bool {
    !config.paused
        && if upload {
            config.uploads()
        } else {
            config.downloads()
        }
}
fn direction_report_status(error: &SyncError) -> &'static str {
    match classify(error) {
        "offline" | "conflict" => "direction_unconfirmed",
        state => state,
    }
}
pub fn classify(error: &SyncError) -> &'static str {
    match error {
        SyncError::Remote(Error::DirectionRequired) => "direction_unconfirmed",
        SyncError::Remote(Error::Protocol(error)) => {
            use rustclipsync_core::direction::DirectionErrorCode;
            match error.code {
                DirectionErrorCode::ProtocolUnsupported => "protocol_unsupported",
                DirectionErrorCode::Unauthenticated | DirectionErrorCode::DeviceMismatch => {
                    "credentials"
                }
                DirectionErrorCode::VersionStale | DirectionErrorCode::SessionRevoked => {
                    "direction_superseded"
                }
                DirectionErrorCode::DirectionForbidden => "direction_blocked",
                DirectionErrorCode::InvalidDirection => "invalid_request",
                DirectionErrorCode::RateLimited | DirectionErrorCode::SessionOffline => "offline",
            }
        }
        SyncError::Storage(_) => "storage_error",
        SyncError::Credentials | SyncError::Remote(Error::Status(401 | 403)) => "credentials",
        SyncError::Conflict | SyncError::Remote(Error::Status(409)) => "conflict",
        SyncError::Expired => "expired",
        SyncError::Epoch => "re_pair",
        SyncError::Clipboard(_) => "clipboard_error",
        SyncError::Remote(Error::InvalidProfile) => "invalid_request",
        SyncError::Remote(Error::Status(424)) => "revocation_unconfirmed",
        SyncError::Remote(Error::Status(426)) => "protocol_unsupported",
        SyncError::Remote(Error::Status(405)) => "direction_blocked",
        SyncError::Remote(Error::Status(412)) => "settings_changed",
        SyncError::Remote(Error::Status(422)) => "history_error",
        SyncError::Remote(Error::Status(423)) => "shortcut_conflict",
        _ => "offline",
    }
}
fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod direction_tests {
    use super::*;

    #[test]
    fn explicit_local_forget_preserves_history_and_other_profile_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::new("https://example.invalid/".into(), "fixture".into());
        config.validate().unwrap();
        storage::atomic_write(
            &dir.path().join("config.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let state = dir.path().join(format!("{}.state", config.identity()));
        std::fs::write(&state, b"fixture-state").unwrap();
        let history = dir.path().join(format!("{}.history", config.identity()));
        std::fs::write(&history, b"fixture-encrypted-history").unwrap();
        let other = dir.path().join("other.state");
        std::fs::write(&other, b"other-profile").unwrap();
        forget_local_pairing(dir.path()).unwrap();
        assert!(!dir.path().join("config.json").exists());
        assert!(!state.exists());
        assert_eq!(
            std::fs::read(history).unwrap(),
            b"fixture-encrypted-history"
        );
        assert_eq!(std::fs::read(other).unwrap(), b"other-profile");
        forget_local_pairing(dir.path()).unwrap();
    }
    #[test]
    fn manual_transfer_respects_the_selected_direction_and_pause() {
        use crate::config::Direction;
        let mut config = Config::new("https://example.invalid/".into(), "fixture".into());
        for (direction, upload, download) in [
            (Direction::Upload, true, false),
            (Direction::Download, false, true),
            (Direction::Both, true, true),
        ] {
            config.direction = direction;
            config.paused = false;
            assert_eq!(manual_allowed(&config, true), upload);
            assert_eq!(manual_allowed(&config, false), download);
            config.paused = true;
            assert!(!manual_allowed(&config, true));
            assert!(!manual_allowed(&config, false));
        }
    }
    #[test]
    fn protocol_and_unconfirmed_revocation_have_actionable_distinct_states() {
        assert_eq!(
            classify(&SyncError::Remote(Error::Status(426))),
            "protocol_unsupported"
        );
        assert_eq!(
            classify(&SyncError::Remote(Error::Status(424))),
            "revocation_unconfirmed"
        );
        assert_eq!(
            classify(&SyncError::Remote(Error::Status(405))),
            "direction_blocked"
        );
        assert_eq!(
            classify(&SyncError::Remote(Error::Status(401))),
            "credentials"
        );
        use rustclipsync_core::direction::{DirectionErrorCode, SyncDirectionError};
        for code in [
            DirectionErrorCode::VersionStale,
            DirectionErrorCode::SessionRevoked,
        ] {
            let error = SyncError::Remote(Error::Protocol(SyncDirectionError {
                code,
                version: Some(7),
                minimum_protocol_version: None,
            }));
            assert_eq!(classify(&error), "direction_superseded");
            assert_eq!(direction_report_status(&error), "direction_superseded");
        }
    }
}

#[cfg(all(test, any(windows, target_os = "linux", feature = "check-windows")))]
mod recovery_tests {
    use super::*;
    use rustclipsync_core::state::{self, InboxItem, Persisted, State};

    #[test]
    #[ignore = "Uses native key storage; run only in a disposable native-test session"]
    fn every_live_session_restores_direction_when_reporting_is_offline() {
        #[cfg(any(windows, feature = "check-windows"))]
        assert_eq!(std::env::var("RCS_WINDOWS_DISPOSABLE").as_deref(), Ok("1"));
        #[cfg(all(target_os = "linux", not(feature = "check-windows")))]
        assert_eq!(std::env::var("RCS_LINUX_DISPOSABLE").as_deref(), Ok("1"));
        for direction in [
            crate::config::Direction::Both,
            crate::config::Direction::Upload,
            crate::config::Direction::Download,
        ] {
            for paused in [false, true] {
                // Reserve then close a loopback port: no deployed server or credentials.
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                drop(listener);
                let mut config = Config::new(format!("https://{address}/"), "fixture".into());
                config.direction = direction;
                config.paused = paused;
                config.allow_sensitive = true;
                config.validate().unwrap();
                let temp = tempfile::tempdir().unwrap();
                let dir = temp.path().join("profile");
                storage::ensure_private_dir(&dir).unwrap();
                let identity = config.identity();
                let key = storage::load_key(&dir, &identity).unwrap();
                let saved = Persisted {
                    server: config.server.clone(),
                    token: state::random_token().unwrap(),
                    paired: true,
                    state: State::new(
                        state::random_uuid().unwrap(),
                        state::random_uuid().unwrap(),
                        42,
                    ),
                    inbox: Some(InboxItem {
                        id: state::random_uuid().unwrap(),
                        text: "disposable private fixture".into(),
                        received_at: 4_000_000_000,
                    }),
                };
                storage::atomic_write(
                    &dir.join(format!("{identity}.state")),
                    &state::seal(&key, &saved).unwrap(),
                )
                .unwrap();
                let (tx, _rx) = mpsc::sync_channel(32);
                let control = Arc::new(Control::new(&config, tx));
                #[cfg(all(target_os = "linux", not(feature = "check-windows")))]
                let (clipboard, _monitor) = PlatformClipboard::start(control.clone()).unwrap();
                #[cfg(any(windows, feature = "check-windows"))]
                let clipboard =
                    PlatformClipboard::new(0, platform::clipboard::Formats::register().unwrap());
                let mut session = open_live(&dir, &config, clipboard, &control).unwrap();
                for construction in 0..2 {
                    let error = session.check().unwrap_err();
                    assert!(
                        !matches!(error, SyncError::Remote(Error::DirectionRequired)),
                        "missing local direction: {direction:?}, paused={paused}, construction={construction}"
                    );
                    assert_eq!(
                        serde_json::to_value(&session.persisted).unwrap(),
                        serde_json::to_value(&saved).unwrap()
                    );
                    if !config.uploads() || paused {
                        assert_eq!(session.push_now(0).unwrap(), Outcome::Paused);
                    }
                    if !config.downloads() || paused {
                        assert_eq!(session.pull(0, Instant::now()).unwrap(), Outcome::Paused);
                    }
                    assert_eq!(*control.status.lock().unwrap(), "direction_unconfirmed");
                    if construction == 0 {
                        session = open_live(&dir, &config, session.clipboard, &control).unwrap();
                    }
                }
            }
        }
    }
}
