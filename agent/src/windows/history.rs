use super::storage;
use crate::{
    config::HistorySettings,
    history::{self, HistoryData},
};
use rustclipsync_core::adapters::{Direction, HistorySink};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct LocalHistory(Arc<Mutex<Inner>>);
struct Inner {
    path: PathBuf,
    key: [u8; 32],
    config: HistorySettings,
    data: HistoryData,
    failed: bool,
}
impl LocalHistory {
    pub fn open(path: PathBuf, key: [u8; 32], config: HistorySettings) -> Self {
        let loaded = storage::read_private(&path, history::MAX_HISTORY as u64)
            .and_then(|raw| raw.map(|b| history::open(&key, b)).transpose());
        let (data, failed) = match loaded {
            Ok(v) => (v.unwrap_or_default(), false),
            Err(_) => (HistoryData::default(), true),
        };
        Self(Arc::new(Mutex::new(Inner {
            path,
            key,
            config,
            data,
            failed,
        })))
    }
    pub fn failed(&self) -> bool {
        self.0.lock().map(|i| i.failed).unwrap_or(true)
    }
    pub fn configure(&self, config: HistorySettings) {
        if let Ok(mut i) = self.0.lock() {
            i.config = config;
        }
    }
    pub fn data(&self) -> io::Result<HistoryData> {
        let i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        if i.failed {
            return Err(io::Error::other("history unavailable"));
        }
        Ok(i.data.clone())
    }
    pub fn change(
        &self,
        action: impl FnOnce(&mut HistoryData, &HistorySettings) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        if i.failed {
            return Err(io::Error::other("history unavailable"));
        }
        let mut data = i.data.clone();
        action(&mut data, &i.config)?;
        if data == i.data {
            return Ok(());
        }
        match history::seal(&i.key, &data).and_then(|bytes| storage::atomic_write(&i.path, &bytes))
        {
            Ok(()) => {
                i.data = data;
                Ok(())
            }
            Err(e) => {
                i.failed = true;
                Err(e)
            }
        }
    }
    pub fn clear(&self) -> io::Result<()> {
        let mut i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        storage::reject_reparse(&i.path)?;
        match std::fs::remove_file(&i.path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        i.data = HistoryData::default();
        i.failed = false;
        Ok(())
    }
}
impl HistorySink for LocalHistory {
    type Error = io::Error;
    fn record(&self, text: &str, direction: Direction, timestamp: u64) -> io::Result<()> {
        self.change(|data, config| {
            data.record(
                config,
                text,
                match direction {
                    Direction::Upload => "upload",
                    Direction::Download => "download",
                },
                timestamp,
            )
        })
    }
    fn record_marked(
        &self,
        text: &str,
        direction: Direction,
        timestamp: u64,
        sensitive: bool,
    ) -> io::Result<()> {
        if sensitive {
            return Ok(());
        }
        self.record(text, direction, timestamp)
    }
}
