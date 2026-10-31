//! Bounded local control contract. No clipboard text or credentials in status.
use crate::config::{Config, FilterSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const MAX_FRAME: usize = 256 * 1024;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub ipc_version: u32,
    pub method: String,
    pub params: Value,
}
#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    GetStatus {},
    SetPaused {
        paused: bool,
    },
    UpdateSettings {
        config: Config,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
    },
    PushNow {},
    PullNow {},
    ListHistory {
        #[serde(default)]
        offset: usize,
        #[serde(default)]
        query: String,
        #[serde(default)]
        favorites: bool,
    },
    CopyHistory {
        id: String,
    },
    DeleteHistory {
        id: String,
    },
    SetFavorite {
        id: String,
        favorite: bool,
    },
    ClearHistory {},
    CopyInbox {
        id: String,
    },
    ReadInbox {
        id: String,
        #[serde(default)]
        offset: usize,
    },
    ClearInbox {
        id: String,
    },
    TestFilter {
        filter: FilterSettings,
        text: String,
    },
    ExportDiagnostics {},
    Quit {},
}
impl Request {
    pub fn command(&self) -> Result<Command, &'static str> {
        if self.ipc_version != 1 || self.id.is_empty() || self.id.len() > 64 {
            return Err("invalid_request");
        }
        let command: Command =
            serde_json::from_value(serde_json::json!({"method":self.method,"params":self.params}))
                .map_err(|_| "invalid_request")?;
        match &command {
            Command::ListHistory { offset, query, .. } if *offset > 1050 || query.len() > 1024 => {
                Err("invalid_request")
            }
            Command::CopyHistory { id }
            | Command::DeleteHistory { id }
            | Command::SetFavorite { id, .. }
            | Command::CopyInbox { id }
            | Command::ReadInbox { id, .. }
            | Command::ClearInbox { id }
                if id.len() != 36 =>
            {
                Err("invalid_request")
            }
            Command::TestFilter { text, .. } if text.len() > 65536 => Err("invalid_request"),
            Command::ReadInbox { offset, .. } if *offset > rustclipsync_core::MAX_TEXT_BYTES => {
                Err("invalid_request")
            }
            _ => Ok(command),
        }
    }
}
#[derive(Serialize)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}
impl Response {
    pub fn new(id: String, result: Result<Value, &'static str>) -> Self {
        match result {
            Ok(v) => Self {
                id,
                ok: true,
                result: Some(v),
                error: None,
            },
            Err(e) => Self {
                id,
                ok: false,
                result: None,
                error: Some(e),
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract_rejects_unknown_versions_methods_and_oversized_inputs() {
        for input in [
            r#"{"id":"a","ipcVersion":2,"method":"getStatus","params":{}}"#,
            r#"{"id":"a","ipcVersion":1,"method":"pair","params":{"password":"secret"}}"#,
        ] {
            let r: Request = serde_json::from_str(input).unwrap();
            assert!(r.command().is_err());
        }
        let r: Request = serde_json::from_str(
            r#"{"id":"a","ipcVersion":1,"method":"setPaused","params":{"paused":true}}"#,
        )
        .unwrap();
        assert!(matches!(
            r.command(),
            Ok(Command::SetPaused { paused: true })
        ));
    }
}
