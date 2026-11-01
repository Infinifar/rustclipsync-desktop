use crate::platform::storage;
use crate::{
    config::HistorySettings,
    history::{self, HistoryData},
};
use rustclipsync_core::adapters::{Direction, HistorySink};
use serde_json::{json, Value};
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
    revision: u64,
    epoch: String,
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
            revision: 1,
            // This is a cache identity, not an encryption key. An empty value
            // disables conditional replies if the random source is unavailable.
            epoch: rustclipsync_core::state::random_uuid().unwrap_or_default(),
        })))
    }
    pub fn revision(&self) -> u64 {
        self.0.lock().map(|i| i.revision).unwrap_or(0)
    }
    pub fn failed(&self) -> bool {
        self.0.lock().map(|i| i.failed).unwrap_or(true)
    }
    pub fn epoch(&self) -> String {
        self.0.lock().map(|i| i.epoch.clone()).unwrap_or_default()
    }
    pub fn configure(&self, config: HistorySettings) {
        if let Ok(mut i) = self.0.lock() {
            if i.config.enabled == config.enabled
                && i.config.days == config.days
                && i.config.limit == config.limit
            {
                return;
            }
            i.config = config;
            i.revision = i.revision.wrapping_add(1);
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
        self.change_when(|_, _| true, action)
    }
    fn change_when(
        &self,
        needed: impl FnOnce(&HistoryData, &HistorySettings) -> bool,
        action: impl FnOnce(&mut HistoryData, &HistorySettings) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        if i.failed {
            return Err(io::Error::other("history unavailable"));
        }
        if !needed(&i.data, &i.config) {
            return Ok(());
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
                i.revision = i.revision.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                i.failed = true;
                Err(e)
            }
        }
    }
    pub fn prune(&self, now: u64) -> io::Result<()> {
        self.change_when(
            |data, cfg| data.needs_prune(cfg, now),
            |data, cfg| {
                data.prune(cfg, now);
                Ok(())
            },
        )
    }
    pub fn page(
        &self,
        offset: usize,
        query: &str,
        favorites: bool,
        known_epoch: Option<&str>,
        known_revision: Option<u64>,
        now: u64,
    ) -> io::Result<Value> {
        self.prune(now)?;
        let i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        if i.failed {
            return Err(io::Error::other("history unavailable"));
        }
        if !i.epoch.is_empty()
            && known_epoch == Some(i.epoch.as_str())
            && known_revision == Some(i.revision)
        {
            return Ok(
                json!({"unchanged":true,"historyEpoch":i.epoch,"historyRevision":i.revision,"enabled":i.config.enabled}),
            );
        }
        let query = query.to_lowercase();
        let matches: Vec<_> = i
            .data
            .entries
            .iter()
            .filter(|e| {
                (!favorites || e.favorite)
                    && (query.is_empty() || e.text.to_lowercase().contains(&query))
            })
            .collect();
        let offset = offset.min(matches.len().saturating_sub(1) / 20 * 20);
        let items: Vec<_> = matches.iter().skip(offset).take(20).map(|e| json!({"id":e.id,"preview":e.text.chars().take(240).collect::<String>(),"timestamp":e.timestamp,"direction":e.direction,"favorite":e.favorite})).collect();
        Ok(
            json!({"unchanged":false,"historyEpoch":i.epoch,"historyRevision":i.revision,"enabled":i.config.enabled,"offset":offset,"total":matches.len(),"items":items}),
        )
    }
    pub fn text(&self, id: &str) -> io::Result<Option<String>> {
        let i = self
            .0
            .lock()
            .map_err(|_| io::Error::other("history unavailable"))?;
        if i.failed {
            return Err(io::Error::other("history unavailable"));
        }
        Ok(i.data
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.text.clone()))
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
        i.revision = i.revision.wrapping_add(1);
        Ok(())
    }
}
impl HistorySink for LocalHistory {
    type Error = io::Error;
    fn record(&self, text: &str, direction: Direction, timestamp: u64) -> io::Result<()> {
        self.change_when(
            |_, config| config.enabled,
            |data, config| {
                data.record(
                    config,
                    text,
                    match direction {
                        Direction::Upload => "upload",
                        Direction::Download => "download",
                    },
                    timestamp,
                )
            },
        )
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
