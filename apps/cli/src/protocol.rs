use crate::args::CommandRequest;
use serde::Deserialize;
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    #[serde(default)]
    pub protocol_version: u8,
    #[serde(default)]
    pub request_id: String,
    pub command: String,
    #[serde(default = "empty_params")]
    pub params: Value,
}

fn empty_params() -> Value {
    json!({})
}

impl Request {
    pub fn into_command(self) -> Result<CommandRequest, String> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported protocolVersion {}; expected {}",
                self.protocol_version, PROTOCOL_VERSION
            ));
        }
        if self.request_id.is_empty() {
            return Err("requestId must not be empty".into());
        }
        if self.command.trim().is_empty() {
            return Err("command must not be empty".into());
        }
        Ok(CommandRequest {
            command: self.command,
            params: self.params,
        })
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub protocol_version: u8,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CommandResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Debug, serde::Serialize)]
pub struct CommandResult {
    #[serde(rename = "type")]
    pub result_type: &'static str,
    pub value: Value,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
}

impl Response {
    pub fn success(request_id: String, sequence: u64, result: CommandResult) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: true,
            sequence: Some(sequence),
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(request_id: String, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: false,
            sequence: None,
            result: None,
            error: Some(ProtocolError {
                code,
                message: message.into(),
            }),
        }
    }
}
