//! Desktop notifications contain neither clipboard text nor account/device IDs.
use super::Signals;
use dbus::{
    arg::{PropMap, Variant},
    blocking::Connection,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub fn inbox_received(signals: &Signals) {
    let Ok(mut last) = signals.last_notification.lock() else {
        return;
    };
    if last.is_some_and(|at| at.elapsed() < Duration::from_secs(3)) {
        return;
    }
    *last = Some(Instant::now());
    // Notification failure never fails an already durable sync operation.
    // Coalescing also bounds the number of concurrent notification workers.
    std::thread::spawn(|| {
        let Ok(connection) = Connection::new_session() else {
            return;
        };
        let proxy = connection.with_proxy(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            Duration::from_secs(2),
        );
        let hints: PropMap = HashMap::from([(
            "desktop-entry".into(),
            Variant(Box::new("io.github.infinifar.rustclipsync".to_owned())
                as Box<dyn dbus::arg::RefArg>),
        )]);
        let _: Result<(u32,), _> = proxy.method_call(
            "org.freedesktop.Notifications",
            "Notify",
            (
                "RustClipSync",
                0u32,
                "io.github.infinifar.rustclipsync",
                "RustClipSync",
                "已收到敏感文本，可在敏感收件区查看。",
                Vec::<String>::new(),
                hints,
                5000i32,
            ),
        );
    });
}
