use rustclipsync_core::{Filter, FilterMode};
use serde::{Deserialize, Serialize};
use std::io;
use url::Url;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Both,
    Upload,
    Download,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilterSettings {
    #[serde(default)]
    pub mode: FilterChoice,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub downloads: bool,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterChoice {
    #[default]
    Off,
    Blacklist,
    Whitelist,
}
impl FilterSettings {
    pub fn compile(&self) -> io::Result<Filter> {
        if self.patterns.len() > 32 || self.patterns.iter().any(|s| s.len() > 512) {
            return Err(invalid("filter limit exceeded"));
        }
        Filter::new(
            match self.mode {
                FilterChoice::Off => FilterMode::Off,
                FilterChoice::Blacklist => FilterMode::Blacklist,
                FilterChoice::Whitelist => FilterMode::Whitelist,
            },
            &self.patterns,
            self.downloads,
        )
        .map_err(|_| invalid("invalid regular expression"))
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistorySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "history_limit")]
    pub limit: usize,
    #[serde(default = "history_days")]
    pub days: u64,
}
fn history_limit() -> usize {
    50
}
fn history_days() -> u64 {
    7
}
fn interval() -> u64 {
    60
}
impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            limit: 50,
            days: 7,
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub server: String,
    pub user: String,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub allow_sensitive: bool,
    #[serde(default)]
    pub auto_apply_sensitive: bool,
    #[serde(default)]
    pub filter: FilterSettings,
    #[serde(default)]
    pub history: HistorySettings,
    #[serde(default = "interval")]
    pub interval_sec: u64,
}
impl Config {
    pub fn new(server: String, user: String) -> Self {
        Self {
            server,
            user,
            direction: Direction::Both,
            paused: false,
            allow_sensitive: false,
            auto_apply_sensitive: false,
            filter: FilterSettings::default(),
            history: HistorySettings::default(),
            interval_sec: 60,
        }
    }
    pub fn validate(&mut self) -> io::Result<()> {
        let mut url = Url::parse(&self.server).map_err(|_| invalid("invalid server URL"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.user.is_empty()
            || self.user.len() > 256
            || self.user.contains([':', '\r', '\n', '\0'])
        {
            return Err(invalid("HTTPS URL and a valid account name are required"));
        }
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        if !(30..=300).contains(&self.interval_sec)
            || !(1..=1000).contains(&self.history.limit)
            || !(1..=365).contains(&self.history.days)
        {
            return Err(invalid("settings outside allowed range"));
        }
        self.filter.compile()?;
        self.server = url.to_string();
        Ok(())
    }
    pub fn identity(&self) -> String {
        rustclipsync_core::hash(&format!("{}\0{}", self.server, self.user))
    }
    pub fn uploads(&self) -> bool {
        self.direction != Direction::Download
    }
    pub fn downloads(&self) -> bool {
        self.direction != Direction::Upload
    }
    pub fn applies_sensitive(&self) -> bool {
        self.allow_sensitive && self.auto_apply_sensitive
    }
}
pub fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equivalent_urls_have_one_identity_and_accounts_remain_distinct() {
        let mut a = Config::new("https://EXAMPLE.test/base".into(), "Cassius".into());
        a.validate().unwrap();
        let mut b = a.clone();
        b.server = "https://example.test/base/".into();
        b.validate().unwrap();
        assert_eq!(a.identity(), b.identity());
        b.user = "cassius".into();
        assert_ne!(a.identity(), b.identity());
    }
    #[test]
    fn migration_keeps_existing_pairing_and_safe_defaults() {
        let mut c: Config = serde_json::from_str(
            r#"{"server":"https://example.test","user":"user","direction":"both","paused":true}"#,
        )
        .unwrap();
        c.validate().unwrap();
        assert!(c.paused);
        assert!(!c.allow_sensitive);
        assert!(!c.auto_apply_sensitive);
        assert!(!c.history.enabled);
        assert_eq!(c.history.days, 7);
        let c: Config = serde_json::from_str(
            r#"{"server":"https://example.test/","user":"user","allowSensitive":true}"#,
        )
        .unwrap();
        assert!(c.allow_sensitive);
        assert!(!c.auto_apply_sensitive);
        assert!(!c.applies_sensitive());
    }
    #[test]
    fn automatic_sensitive_clipboard_requires_both_opt_ins() {
        let mut c = Config::new("https://example.test".into(), "user".into());
        for allow in [false, true] {
            for auto in [false, true] {
                c.allow_sensitive = allow;
                c.auto_apply_sensitive = auto;
                let restored: Config =
                    serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
                assert_eq!(restored.applies_sensitive(), allow && auto);
            }
        }
    }
    #[test]
    fn rejects_credentials_unsafe_ranges_and_bad_regex() {
        for server in [
            "http://localhost",
            "https://user@host",
            "https://host/?password=x",
            "https://host/#x",
        ] {
            assert!(Config::new(server.into(), "user".into())
                .validate()
                .is_err());
        }
        assert!(serde_json::from_str::<Config>(
            r#"{"server":"https://host","user":"u","password":"secret"}"#
        )
        .is_err());
        let mut c = Config::new("https://host".into(), "user".into());
        c.filter.patterns = vec!["[".into()];
        assert!(c.validate().is_err());
        c.filter.patterns.clear();
        c.history.days = 0;
        assert!(c.validate().is_err());
    }
}
