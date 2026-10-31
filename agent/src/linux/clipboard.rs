use crate::runtime::Control;
use rustclipsync_core::{
    adapters::{Clipboard, ClipboardError},
    MAX_TEXT_BYTES,
};
use std::{
    cell::Cell,
    collections::VecDeque,
    io,
    sync::{atomic::Ordering, Arc, Mutex},
    thread::JoinHandle,
};

pub const ORIGIN: &str = "application/x-rustclipsync-v1-";
pub const SENSITIVE: &str = "x-kde-passwordManagerHint";
pub const TEXT_TYPES: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
#[derive(Default)]
pub struct Observation {
    pub value: Option<(String, bool)>,
    pub marker: Option<String>,
    pub unchanged: bool,
}
#[derive(Default)]
pub struct Cache {
    pub pending: bool,
    pub observation: Observation,
    pub error: bool,
}
pub type SharedCache = Arc<Mutex<Cache>>;
pub struct PlatformClipboard {
    cache: SharedCache,
    writer: Writer,
    written: Mutex<VecDeque<String>>,
    auto: Cell<bool>,
}
enum Writer {
    Wayland,
    X11(super::x11::Writer),
}
pub struct Monitor {
    cancel: Option<wl_clipboard_rs::watch::CancelHandle>,
    thread: Option<JoinHandle<()>>,
    control: Arc<Control>,
}
impl Monitor {
    pub(super) fn new(
        control: Arc<Control>,
        cancel: Option<wl_clipboard_rs::watch::CancelHandle>,
        thread: JoinHandle<()>,
    ) -> Self {
        Self {
            control,
            cancel,
            thread: Some(thread),
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.control.stop.store(true, Ordering::SeqCst);
        self.control.notify();
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl PlatformClipboard {
    pub fn start(control: Arc<Control>) -> io::Result<(Self, Monitor)> {
        let cache = Arc::new(Mutex::new(Cache::default()));
        let (writer, monitor) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            (
                Writer::Wayland,
                super::wayland::start(cache.clone(), control)?,
            )
        } else if std::env::var_os("DISPLAY").is_some() {
            let (writer, monitor) = super::x11::start(cache.clone(), control)?;
            (Writer::X11(writer), monitor)
        } else {
            return Err(io::Error::other("no Wayland or X11 desktop session"));
        };
        Ok((
            Self {
                cache,
                writer,
                written: Mutex::new(VecDeque::new()),
                auto: Cell::new(false),
            },
            monitor,
        ))
    }
    pub fn set_auto_apply_sensitive(&self, enabled: bool) {
        self.auto.set(enabled);
    }
    pub fn observe(&self, allow_sensitive: bool) -> Result<Observation, ClipboardError> {
        let mut cache = self.cache.lock().map_err(|_| ClipboardError::Unavailable)?;
        if cache.pending {
            return Err(ClipboardError::Unavailable);
        }
        if cache.error {
            return Err(ClipboardError::Platform);
        }
        let mut observation = Observation {
            value: cache.observation.value.clone(),
            marker: cache.observation.marker.clone(),
            unchanged: cache.observation.unchanged,
        };
        cache.observation.unchanged = true;
        if let Some(id) = &observation.marker {
            if self
                .written
                .lock()
                .map_err(|_| ClipboardError::Unavailable)?
                .contains(id)
            {
                observation.unchanged = true;
            }
        }
        if !allow_sensitive
            && observation
                .value
                .as_ref()
                .is_some_and(|(_, sensitive)| *sensitive)
        {
            return Err(ClipboardError::Sensitive);
        }
        Ok(observation)
    }
    pub fn copy_saved(&self, text: &str, id: &str, sensitive: bool) -> Result<(), ClipboardError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        if id.len() != 36 || !id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') {
            return Err(ClipboardError::Platform);
        }
        {
            let mut written = self
                .written
                .lock()
                .map_err(|_| ClipboardError::Unavailable)?;
            written.push_back(id.into());
            while written.len() > 64 {
                written.pop_front();
            }
        }
        match &self.writer {
            Writer::Wayland => super::wayland::write(text, id, sensitive),
            Writer::X11(writer) => writer.write(text, id, sensitive),
        }
    }
}
impl Clipboard for PlatformClipboard {
    fn snapshot(&self, allow_sensitive: bool) -> Result<Option<(String, bool)>, ClipboardError> {
        let cache = self.cache.lock().map_err(|_| ClipboardError::Unavailable)?;
        if cache.pending || cache.error {
            return Err(ClipboardError::Unavailable);
        }
        if !allow_sensitive
            && cache
                .observation
                .value
                .as_ref()
                .is_some_and(|(_, sensitive)| *sensitive)
        {
            return Err(ClipboardError::Sensitive);
        }
        Ok(cache.observation.value.clone())
    }
    fn write(&self, text: &str, id: &str, sensitive: bool) -> Result<(), ClipboardError> {
        if sensitive && !self.auto.get() {
            return Err(ClipboardError::Sensitive);
        }
        self.copy_saved(text, id, sensitive)
    }
}
pub(super) fn begin(cache: &SharedCache, control: &Control, initial: bool) {
    if let Ok(mut cache) = cache.lock() {
        cache.pending = true;
    }
    if !initial {
        control.generation.fetch_add(1, Ordering::SeqCst);
    }
}
pub(super) fn finish(
    cache: &SharedCache,
    control: &Control,
    result: Result<Observation, ClipboardError>,
    initial: bool,
) {
    if let Ok(mut cache) = cache.lock() {
        cache.pending = false;
        match result {
            Ok(o) => {
                cache.observation = o;
                cache.error = false;
            }
            Err(_) => {
                cache.observation = Observation::default();
                cache.error = true;
            }
        }
    }
    if !initial {
        control.notify();
    }
}
pub(super) fn marker(types: &[String]) -> Option<String> {
    types
        .iter()
        .find_map(|t| t.strip_prefix(ORIGIN))
        .filter(|id| id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'))
        .map(str::to_owned)
}
