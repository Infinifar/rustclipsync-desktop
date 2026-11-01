//! Optional, bounded history. Sensitive content stays in the separate inbox.
use crate::config::HistorySettings;
use ring::{
    aead, digest,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::io;
pub const MAX_HISTORY: usize = 24 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_TOTAL: usize = 16 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub direction: String,
    pub timestamp: u64,
    pub favorite: bool,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoryData {
    pub entries: Vec<Entry>,
}
impl HistoryData {
    pub fn needs_prune(&self, cfg: &HistorySettings, now: u64) -> bool {
        let cutoff = now.saturating_sub(cfg.days.saturating_mul(86400));
        let mut previous = u64::MAX;
        let mut count = 0;
        let mut bytes: usize = self
            .entries
            .iter()
            .filter(|e| e.favorite)
            .map(|e| e.text.len())
            .sum();
        for entry in &self.entries {
            if entry.timestamp > previous {
                return true;
            }
            previous = entry.timestamp;
            if !entry.favorite {
                if entry.timestamp < cutoff
                    || count >= cfg.limit
                    || bytes + entry.text.len() > MAX_TOTAL
                {
                    return true;
                }
                count += 1;
                bytes += entry.text.len();
            }
        }
        false
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.entries.len() > 1050
            || self.entries.iter().filter(|e| e.favorite).count() > 50
            || self.entries.iter().map(|e| e.text.len()).sum::<usize>() > MAX_TOTAL
            || self.entries.iter().any(|v| {
                v.text.len() > MAX_TEXT
                    || v.id.len() != 36
                    || !matches!(v.direction.as_str(), "upload" | "download")
            })
        {
            return Err(io::Error::other("invalid history"));
        }
        Ok(())
    }
    pub fn prune(&mut self, cfg: &HistorySettings, now: u64) {
        self.entries
            .retain(|e| e.favorite || e.timestamp >= now.saturating_sub(cfg.days * 86400));
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        let mut count = 0;
        let mut bytes: usize = self
            .entries
            .iter()
            .filter(|e| e.favorite)
            .map(|e| e.text.len())
            .sum();
        self.entries.retain(|e| {
            if e.favorite {
                return true;
            }
            if count >= cfg.limit || bytes + e.text.len() > MAX_TOTAL {
                return false;
            }
            count += 1;
            bytes += e.text.len();
            true
        });
    }
    pub fn record(
        &mut self,
        cfg: &HistorySettings,
        text: &str,
        direction: &str,
        now: u64,
    ) -> io::Result<()> {
        if !cfg.enabled {
            return Ok(());
        }
        if text.len() > MAX_TEXT {
            return Err(io::Error::other("history item limit"));
        }
        let previous = self
            .entries
            .iter()
            .position(|e| e.text == text)
            .map(|i| self.entries.remove(i));
        let (id, favorite) = previous
            .map(|e| (e.id, e.favorite))
            .unwrap_or((rustclipsync_core::state::random_uuid()?, false));
        self.entries.insert(
            0,
            Entry {
                id,
                text: text.into(),
                direction: direction.into(),
                timestamp: now,
                favorite,
            },
        );
        self.prune(cfg, now);
        Ok(())
    }
    pub fn favorite(&mut self, id: &str, value: bool) -> io::Result<()> {
        if value
            && self.entries.iter().filter(|e| e.favorite).count() >= 50
            && self.entries.iter().any(|e| e.id == id && !e.favorite)
        {
            return Err(io::Error::other("favorite limit"));
        }
        if value {
            let pinned: usize = self
                .entries
                .iter()
                .filter(|e| e.favorite || e.id == id)
                .map(|e| e.text.len())
                .sum();
            if pinned > MAX_TOTAL {
                return Err(io::Error::other("favorite capacity"));
            }
        }
        self.entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| io::Error::other("item not found"))?
            .favorite = value;
        Ok(())
    }
}
fn key(key: &[u8; 32]) -> io::Result<aead::LessSafeKey> {
    let mut material = zeroize::Zeroizing::new(Vec::from(key.as_slice()));
    material.extend_from_slice(b"RustClipSync history");
    let derived = digest::digest(&digest::SHA256, &material);
    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, derived.as_ref())
            .map_err(|_| io::Error::other("history key"))?,
    ))
}
pub fn seal(secret: &[u8; 32], history: &HistoryData) -> io::Result<Vec<u8>> {
    history.validate()?;
    let mut bytes = serde_json::to_vec(history)?;
    if bytes.len() > MAX_HISTORY - 32 {
        return Err(io::Error::other("history size limit"));
    }
    let mut nonce = [0; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| io::Error::other("random unavailable"))?;
    key(secret)?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"RCH1"),
            &mut bytes,
        )
        .map_err(|_| io::Error::other("history encryption"))?;
    let mut result = b"RCH1".to_vec();
    result.extend(nonce);
    result.extend(bytes);
    Ok(result)
}
pub fn open(secret: &[u8; 32], mut bytes: Vec<u8>) -> io::Result<HistoryData> {
    if bytes.len() < 32 || bytes.len() > MAX_HISTORY || !bytes.starts_with(b"RCH1") {
        return Err(io::Error::other("invalid history"));
    }
    let nonce = bytes[4..16].try_into().map_err(io::Error::other)?;
    let raw = key(secret)?
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"RCH1"),
            &mut bytes[16..],
        )
        .map_err(|_| io::Error::other("history authentication"))?;
    let data: HistoryData = serde_json::from_slice(raw)?;
    data.validate()?;
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_encrypted_authenticated_bounded_and_retains_explicit_favorites() {
        let key = [7; 32];
        let cfg = HistorySettings {
            enabled: true,
            limit: 1,
            days: 7,
        };
        let mut h = HistoryData::default();
        h.record(&cfg, "fixture first", "upload", 100).unwrap();
        let id = h.entries[0].id.clone();
        h.favorite(&id, true).unwrap();
        h.record(&cfg, "fixture second", "download", 900000)
            .unwrap();
        assert_eq!(h.entries.len(), 2);
        let raw = seal(&key, &h).unwrap();
        assert!(!raw.windows(7).any(|s| s == b"fixture"));
        assert_eq!(open(&key, raw.clone()).unwrap().entries.len(), 2);
        assert!(open(&[8; 32], raw.clone()).is_err());
        let mut bad = raw;
        bad[17] ^= 1;
        assert!(open(&key, bad).is_err());
        h.favorite(&id, false).unwrap();
        h.prune(&cfg, 900000);
        assert_eq!(h.entries.len(), 1);
    }
    #[test]
    fn disabled_history_retains_nothing_new() {
        let mut h = HistoryData::default();
        h.record(&HistorySettings::default(), "fixture", "upload", 0)
            .unwrap();
        assert!(h.entries.is_empty());
    }
}
