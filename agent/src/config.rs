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
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub server: String,
    pub user: String,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub paused: bool,
}
impl Config {
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
}
pub fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equivalent_urls_have_one_identity_and_user_accounts_remain_distinct() {
        let mut a = Config {
            server: "https://EXAMPLE.test/base".into(),
            user: "Cassius".into(),
            direction: Direction::Both,
            paused: false,
        };
        a.validate().unwrap();
        let mut b = a.clone();
        b.server = "https://example.test/base/".into();
        b.validate().unwrap();
        assert_eq!(a.identity(), b.identity());
        b.user = "cassius".into();
        assert_ne!(a.identity(), b.identity());
    }
    #[test]
    fn rejects_embedded_credentials_and_secret_config_fields() {
        for server in [
            "http://localhost",
            "https://user@host",
            "https://host/?password=x",
            "https://host/#x",
        ] {
            let mut c = Config {
                server: server.into(),
                user: "user".into(),
                direction: Direction::Both,
                paused: false,
            };
            assert!(c.validate().is_err());
        }
        assert!(serde_json::from_str::<Config>(
            r#"{"server":"https://host","user":"u","password":"secret"}"#
        )
        .is_err());
    }
}
