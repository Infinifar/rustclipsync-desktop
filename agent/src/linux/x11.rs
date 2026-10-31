//! Event-driven XFixes regular clipboard; bounded ICCCM INCR transfers.
use super::clipboard::{self, Monitor, Observation, SharedCache};
use crate::runtime::Control;
use rustclipsync_core::{adapters::ClipboardError, MAX_TEXT_BYTES};
use std::{
    collections::HashMap,
    io,
    sync::{
        atomic::Ordering,
        mpsc::{self, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xfixes::{ConnectionExt as _, SelectionEventMask},
        xproto::{ConnectionExt as _, *},
        Event,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE,
};
x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD, TARGETS, UTF8_STRING, INCR, TIMESTAMP,
        property: b"RUSTCLIPSYNC_TRANSFER",
        text: b"text/plain;charset=utf-8",
        plain: b"text/plain",
        origin: b"application/x-rustclipsync-message-id",
        sensitive: b"x-kde-passwordManagerHint",
    }
}
struct WriteRequest {
    text: String,
    id: String,
    sensitive: bool,
    reply: SyncSender<Result<(), ClipboardError>>,
}
pub struct Writer(SyncSender<WriteRequest>);
impl Writer {
    pub fn write(&self, text: &str, id: &str, sensitive: bool) -> Result<(), ClipboardError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.0
            .try_send(WriteRequest {
                text: text.into(),
                id: id.into(),
                sensitive,
                reply: tx,
            })
            .map_err(|_| ClipboardError::Unavailable)?;
        rx.recv_timeout(Duration::from_secs(7))
            .map_err(|_| ClipboardError::Timeout)?
    }
}
struct Context {
    connection: RustConnection,
    window: Window,
    atoms: Atoms,
}
impl Context {
    fn new() -> io::Result<Self> {
        let (c, screen) = x11rb::connect(None).map_err(io::Error::other)?;
        let window = c.generate_id().map_err(io::Error::other)?;
        let root = &c.setup().roots[screen];
        c.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            root.root_visual,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(io::Error::other)?
        .check()
        .map_err(io::Error::other)?;
        let atoms = Atoms::new(&c)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok(Self {
            connection: c,
            window,
            atoms,
        })
    }
    fn owner(&self) -> io::Result<Window> {
        Ok(self
            .connection
            .get_selection_owner(self.atoms.CLIPBOARD)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?
            .owner)
    }
    /// Read on a separate connection: transfer events never consume watcher events.
    fn load(&self, target: Atom, limit: usize) -> Result<Vec<u8>, ClipboardError> {
        let c = &self.connection;
        c.delete_property(self.window, self.atoms.property)
            .map_err(|_| ClipboardError::Platform)?;
        c.convert_selection(
            self.window,
            self.atoms.CLIPBOARD,
            target,
            self.atoms.property,
            CURRENT_TIME,
        )
        .map_err(|_| ClipboardError::Platform)?;
        c.flush().map_err(|_| ClipboardError::Platform)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut incremental = false;
        let mut bytes = Vec::new();
        loop {
            if Instant::now() >= deadline {
                return Err(ClipboardError::Timeout);
            }
            let event = c.poll_for_event().map_err(|_| ClipboardError::Platform)?;
            let read = match event {
                Some(Event::SelectionNotify(event)) if event.selection == self.atoms.CLIPBOARD => {
                    if event.property == NONE {
                        return Err(ClipboardError::Unavailable);
                    }
                    true
                }
                Some(Event::PropertyNotify(event))
                    if incremental
                        && event.atom == self.atoms.property
                        && event.state == Property::NEW_VALUE =>
                {
                    true
                }
                None => {
                    std::thread::sleep(Duration::from_millis(5));
                    false
                }
                _ => false,
            };
            if !read {
                continue;
            }
            let remaining = limit.saturating_sub(bytes.len());
            let reply = c
                .get_property(
                    true,
                    self.window,
                    self.atoms.property,
                    AtomEnum::ANY,
                    0,
                    (remaining.div_ceil(4) + 1) as u32,
                )
                .map_err(|_| ClipboardError::Platform)?
                .reply()
                .map_err(|_| ClipboardError::Platform)?;
            if reply.type_ == self.atoms.INCR {
                if reply
                    .value32()
                    .and_then(|mut v| v.next())
                    .is_some_and(|size| size as usize > limit)
                {
                    return Err(ClipboardError::TooLarge);
                }
                incremental = true;
                c.delete_property(self.window, self.atoms.property)
                    .map_err(|_| ClipboardError::Platform)?;
                c.flush().map_err(|_| ClipboardError::Platform)?;
                continue;
            }
            if reply.bytes_after != 0 || reply.value.len() > remaining {
                return Err(ClipboardError::TooLarge);
            }
            if reply.type_ != target
                && !(target == self.atoms.TARGETS && reply.type_ == u32::from(AtomEnum::ATOM))
            {
                return Err(ClipboardError::Unavailable);
            }
            let empty = reply.value.is_empty();
            bytes.extend_from_slice(&reply.value);
            c.flush().map_err(|_| ClipboardError::Platform)?;
            if !incremental || empty {
                return Ok(bytes);
            }
        }
    }
    fn read(&self, expected_owner: Window) -> Result<Observation, ClipboardError> {
        if expected_owner == NONE {
            return Ok(Observation::default());
        }
        let targets = self.load(self.atoms.TARGETS, 64 * 1024)?;
        let targets: Vec<u32> = targets
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_ne_bytes(*b))
            .collect();
        let target = [self.atoms.UTF8_STRING, self.atoms.text, self.atoms.plain]
            .into_iter()
            .find(|a| targets.contains(a));
        let Some(target) = target else {
            return Ok(Observation::default());
        };
        let sensitive = targets.contains(&self.atoms.sensitive);
        let marker = if targets.contains(&self.atoms.origin) {
            String::from_utf8(self.load(self.atoms.origin, 36)?)
                .ok()
                .filter(|id| {
                    id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
                })
        } else {
            None
        };
        let text = String::from_utf8(self.load(target, MAX_TEXT_BYTES)?)
            .map_err(|_| ClipboardError::InvalidUtf8)?;
        if self.owner().map_err(ClipboardError::Io)? != expected_owner {
            return Err(ClipboardError::Unavailable);
        }
        Ok(Observation {
            value: Some((text, sensitive)),
            marker,
            unchanged: false,
        })
    }
}
struct Selection {
    text: Arc<Vec<u8>>,
    id: String,
    sensitive: bool,
}
struct Transfer {
    target: Atom,
    bytes: Arc<Vec<u8>>,
    offset: usize,
    expires: Instant,
}
pub fn start(cache: SharedCache, control: Arc<Control>) -> io::Result<(Writer, Monitor)> {
    let context = Context::new()?;
    let reader = Context::new()?;
    context
        .connection
        .xfixes_query_version(5, 0)
        .map_err(io::Error::other)?
        .reply()
        .map_err(io::Error::other)?;
    context
        .connection
        .xfixes_select_selection_input(
            context.window,
            context.atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(io::Error::other)?
        .check()
        .map_err(io::Error::other)?;
    context.connection.flush().map_err(io::Error::other)?;
    let initial_owner = context.owner()?;
    let (tx, rx) = mpsc::sync_channel::<WriteRequest>(4);
    let shared = control.clone();
    let thread = std::thread::spawn(move || {
        clipboard::begin(&cache, &shared, true);
        clipboard::finish(&cache, &shared, reader.read(initial_owner), true);
        let mut selection: Option<Selection> = None;
        let mut transfers: HashMap<(Window, Atom), Transfer> = HashMap::new();
        while !shared.stop.load(Ordering::SeqCst) {
            while let Ok(request) = rx.try_recv() {
                let result = context
                    .connection
                    .set_selection_owner(context.window, context.atoms.CLIPBOARD, CURRENT_TIME)
                    .map_err(|_| ClipboardError::Platform)
                    .and_then(|cookie| cookie.check().map_err(|_| ClipboardError::Platform));
                let result = result.and_then(|_| {
                    if context.owner().ok() == Some(context.window) {
                        Ok(())
                    } else {
                        Err(ClipboardError::Unavailable)
                    }
                });
                if result.is_ok() {
                    selection = Some(Selection {
                        text: Arc::new(request.text.into_bytes()),
                        id: request.id,
                        sensitive: request.sensitive,
                    });
                }
                let _ = request.reply.send(result);
            }
            transfers.retain(|_, t| t.expires > Instant::now());
            let event = match context.connection.poll_for_event() {
                Ok(Some(event)) => event,
                Ok(None) => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => {
                    clipboard::begin(&cache, &shared, false);
                    clipboard::finish(&cache, &shared, Err(ClipboardError::Unavailable), false);
                    shared.set_status("clipboard_error");
                    break;
                }
            };
            match event {
                Event::XfixesSelectionNotify(e) if e.selection == context.atoms.CLIPBOARD => {
                    clipboard::begin(&cache, &shared, false);
                    let result = if e.owner == context.window {
                        selection
                            .as_ref()
                            .map(|s| Observation {
                                value: Some((
                                    String::from_utf8_lossy(&s.text).into_owned(),
                                    s.sensitive,
                                )),
                                marker: Some(s.id.clone()),
                                unchanged: false,
                            })
                            .ok_or(ClipboardError::Unavailable)
                    } else {
                        reader.read(e.owner)
                    };
                    clipboard::finish(&cache, &shared, result, false);
                }
                Event::SelectionClear(_) => {
                    selection = None;
                }
                Event::SelectionRequest(request)
                    if request.selection == context.atoms.CLIPBOARD =>
                {
                    if let Some(selection) = &selection {
                        let _ = serve(&context, selection, request, &mut transfers);
                    }
                }
                Event::PropertyNotify(event) if event.state == Property::DELETE => {
                    let key = (event.window, event.atom);
                    if let Some(transfer) = transfers.get_mut(&key) {
                        let end = (transfer.offset + 64 * 1024).min(transfer.bytes.len());
                        let _ = context.connection.change_property8(
                            PropMode::REPLACE,
                            event.window,
                            event.atom,
                            transfer.target,
                            &transfer.bytes[transfer.offset..end],
                        );
                        let _ = context.connection.flush();
                        if transfer.offset == end {
                            transfers.remove(&key);
                        } else {
                            transfer.offset = end;
                        }
                    }
                }
                _ => {}
            }
        }
    });
    Ok((Writer(tx), Monitor::new(control, None, thread)))
}
fn serve(
    c: &Context,
    selection: &Selection,
    request: SelectionRequestEvent,
    transfers: &mut HashMap<(Window, Atom), Transfer>,
) -> io::Result<()> {
    let a = &c.atoms;
    let property = if request.property == NONE {
        request.target
    } else {
        request.property
    };
    let mut accepted = true;
    if request.target == a.TARGETS {
        let mut targets = vec![a.TARGETS, a.UTF8_STRING, a.text, a.plain, a.origin];
        if selection.sensitive {
            targets.push(a.sensitive);
        }
        c.connection
            .change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::ATOM,
                &targets,
            )
            .map_err(io::Error::other)?;
    } else {
        let bytes = if [a.UTF8_STRING, a.text, a.plain].contains(&request.target) {
            Some(selection.text.clone())
        } else if request.target == a.origin {
            Some(Arc::new(selection.id.as_bytes().to_vec()))
        } else if request.target == a.sensitive && selection.sensitive {
            Some(Arc::new(b"secret".to_vec()))
        } else {
            None
        };
        if let Some(bytes) = bytes {
            if bytes.len() <= 64 * 1024 {
                c.connection
                    .change_property8(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        request.target,
                        &bytes,
                    )
                    .map_err(io::Error::other)?;
            } else if transfers.len() < 8 {
                c.connection
                    .change_window_attributes(
                        request.requestor,
                        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                    )
                    .map_err(io::Error::other)?;
                c.connection
                    .change_property32(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        a.INCR,
                        &[bytes.len() as u32],
                    )
                    .map_err(io::Error::other)?;
                transfers.insert(
                    (request.requestor, property),
                    Transfer {
                        target: request.target,
                        bytes,
                        offset: 0,
                        expires: Instant::now() + Duration::from_secs(30),
                    },
                );
            } else {
                accepted = false;
            }
        } else {
            accepted = false;
        }
    }
    c.connection
        .send_event(
            false,
            request.requestor,
            EventMask::NO_EVENT,
            SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: request.time,
                requestor: request.requestor,
                selection: request.selection,
                target: request.target,
                property: if accepted { property } else { NONE },
            },
        )
        .map_err(io::Error::other)?;
    c.connection.flush().map_err(io::Error::other)
}
