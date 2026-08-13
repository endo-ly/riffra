use riffra_core::application::SessionSettingsPatch;
use riffra_core::{CreativeSession, Track, TrackKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    GetSession,
    ListTracks,
    AddTrack {
        name: String,
        kind: TrackKind,
    },
    RemoveTrack {
        track_id: String,
    },
    UpdateSessionSettings {
        #[serde(flatten)]
        patch: SessionSettingsPatch,
    },
    Undo,
    Redo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub request_id: String,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum CommandResult {
    Session(Box<CreativeSession>),
    Tracks(Vec<Track>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CommandResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
}

impl Response {
    pub fn success(request_id: String, result: CommandResult) -> Self {
        Self {
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(request_id: String, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            request_id,
            ok: false,
            result: None,
            error: Some(ProtocolError {
                code,
                message: message.into(),
            }),
        }
    }
}
