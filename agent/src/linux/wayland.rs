//! Native data-control access, independent of any QML shell or focused window.
use super::clipboard::{self, Monitor, Observation, SharedCache, ORIGIN, SENSITIVE, TEXT_TYPES};
use crate::runtime::Control;
use rustclipsync_core::{adapters::ClipboardError, MAX_TEXT_BYTES};
use std::{
    io::{self, Read},
    os::fd::AsRawFd,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use wl_clipboard_rs::{
    copy, paste,
    watch::{ClipboardEvent, ClipboardType, Watcher},
};

pub fn start(cache: SharedCache, control: Arc<Control>) -> io::Result<Monitor> {
    let mut watcher = Watcher::new(ClipboardType::Regular, paste::Seat::Unspecified)
        .map_err(|_| io::Error::other("Wayland data-control is unavailable; this compositor needs a supported clipboard integration"))?;
    let cancel = watcher.cancel_handle();
    let shared = control.clone();
    let thread = std::thread::spawn(move || {
        let mut initial = true;
        loop {
            let event = match watcher.next_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    clipboard::begin(&cache, &shared, false);
                    clipboard::finish(&cache, &shared, Err(ClipboardError::Unavailable), false);
                    shared.set_status("clipboard_error");
                    break;
                }
            };
            if shared.stop.load(Ordering::SeqCst) {
                break;
            }
            clipboard::begin(&cache, &shared, initial);
            let result = match event {
                ClipboardEvent::Cleared { .. } => Ok(Observation::default()),
                ClipboardEvent::Changed {
                    mime_types,
                    mut offer,
                    ..
                } => {
                    let mime = TEXT_TYPES
                        .iter()
                        .find(|m| mime_types.iter().any(|t| t == **m));
                    if let Some(mime) = mime {
                        let sensitive = mime_types.iter().any(|t| t == SENSITIVE);
                        offer
                            .receive(mime)
                            .map_err(|_| ClipboardError::Unavailable)
                            .and_then(|reader| {
                                read_bounded(reader, MAX_TEXT_BYTES, Duration::from_secs(5))
                            })
                            .map(|text| Observation {
                                value: Some((text, sensitive)),
                                marker: clipboard::marker(&mime_types),
                                unchanged: false,
                            })
                    } else {
                        Ok(Observation::default())
                    }
                }
            };
            clipboard::finish(&cache, &shared, result, initial);
            initial = false;
        }
    });
    Ok(Monitor::new(control, Some(cancel), thread))
}
pub fn write(text: &str, id: &str, sensitive: bool) -> Result<(), ClipboardError> {
    let mut sources = vec![
        copy::MimeSource {
            source: copy::Source::Bytes(text.as_bytes().into()),
            mime_type: copy::MimeType::Text,
        },
        copy::MimeSource {
            source: copy::Source::Bytes(Box::new([])),
            mime_type: copy::MimeType::Specific(format!("{ORIGIN}{id}")),
        },
    ];
    if sensitive {
        sources.push(copy::MimeSource {
            source: copy::Source::Bytes(b"secret".to_vec().into()),
            mime_type: copy::MimeType::Specific(SENSITIVE.into()),
        });
    }
    copy::Options::new()
        .copy_multi(sources)
        .map_err(|_| ClipboardError::Unavailable)
}
pub fn read_bounded(
    mut reader: impl Read + AsRawFd,
    limit: usize,
    timeout: Duration,
) -> Result<String, ClipboardError> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ClipboardError::Timeout);
        }
        let mut descriptor = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                left.as_millis().clamp(1, i32::MAX as u128) as i32,
            )
        };
        if result == 0 {
            return Err(ClipboardError::Timeout);
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(ClipboardError::Io(error));
        }
        if descriptor.revents & libc::POLLNVAL != 0 {
            return Err(ClipboardError::Unavailable);
        }
        let count = match reader.read(&mut buffer) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ClipboardError::Io(e)),
        };
        if count == 0 {
            break;
        }
        if bytes.len() + count > limit {
            return Err(ClipboardError::TooLarge);
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|_| ClipboardError::InvalidUtf8)
}
