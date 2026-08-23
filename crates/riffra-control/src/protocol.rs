use crate::ControlCommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u16 = 2;

/// Machine-readable failure classes exposed by the local control boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    InvalidRequest,
    CommandFailed,
    Conflict,
    HostUnavailable,
    RuntimeUnavailable,
}

impl ErrorCode {
    /// Returns the JSON spelling used by the protocol.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalidRequest",
            Self::CommandFailed => "commandFailed",
            Self::Conflict => "conflict",
            Self::HostUnavailable => "hostUnavailable",
            Self::RuntimeUnavailable => "runtimeUnavailable",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Structured error returned by a control server.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ProtocolError {
    /// Creates an error without machine-readable details.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Creates a conflict with the revisions used for the failed precondition.
    pub fn conflict(expected_sequence: u64, current_sequence: u64) -> Self {
        Self {
            code: ErrorCode::Conflict,
            message: "canonical state changed".into(),
            details: Some(serde_json::json!({
                "expectedSequence": expected_sequence,
                "currentSequence": current_sequence,
            })),
        }
    }
}

/// One command request sent over JSON Lines or a framed local transport.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlRequest {
    pub protocol_version: u16,
    pub request_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_sequence: Option<u64>,
    pub params: Value,
}

impl ControlRequest {
    /// Creates a request for one backend command.
    pub fn new(
        request_id: impl Into<String>,
        command: ControlCommand,
        expected_sequence: Option<u64>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            command: command.name,
            expected_sequence,
            params: command.params,
        }
    }

    /// Validates envelope fields before a backend is allowed to execute it.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                format!("unsupported protocol version: {}", self.protocol_version),
            ));
        }
        if self.request_id.trim().is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "requestId must not be empty",
            ));
        }
        if self.command.trim().is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "command must not be empty",
            ));
        }
        Ok(())
    }

    /// Returns the command portion without its request envelope.
    pub fn control_command(&self) -> ControlCommand {
        ControlCommand::new(self.command.clone(), self.params.clone())
    }
}

/// Result payload returned after a command succeeds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandResult {
    #[serde(rename = "type")]
    pub result_type: String,
    pub value: Value,
}

/// One response to one control request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CommandResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl ControlResponse {
    /// Creates a successful response.
    pub fn success(request_id: impl Into<String>, sequence: u64, result: CommandResult) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: true,
            sequence: Some(sequence),
            result: Some(result),
            error: None,
        }
    }

    /// Creates a failed response.
    pub fn failure(
        request_id: impl Into<String>,
        sequence: Option<u64>,
        error: ProtocolError,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: false,
            sequence,
            result: None,
            error: Some(error),
        }
    }
}

/// First message sent by a control client after opening a pipe.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloRequest {
    #[serde(rename = "type")]
    pub message_type: String,
    pub protocol_version: u16,
}

impl HelloRequest {
    /// Creates the protocol handshake request.
    pub fn new() -> Self {
        Self {
            message_type: "hello".into(),
            protocol_version: PROTOCOL_VERSION,
        }
    }
}

impl Default for HelloRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Handshake response identifying the live Desktop instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResponse {
    #[serde(rename = "type")]
    pub message_type: String,
    pub protocol_version: u16,
    pub instance_id: String,
    pub pid: u32,
}

impl HelloResponse {
    /// Creates the server side handshake response.
    pub fn new(instance_id: impl Into<String>, pid: u32) -> Self {
        Self {
            message_type: "hello".into(),
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id.into(),
            pid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_v2_round_trips_expected_sequence() {
        let request = ControlRequest::new(
            "42",
            ControlCommand::new("track.add", serde_json::json!({"name": "Bass"})),
            Some(18),
        );

        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ControlRequest = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, request);
        assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn invalid_envelope_fields_are_rejected() {
        let request = ControlRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: String::new(),
            command: String::new(),
            expected_sequence: None,
            params: Value::Null,
        };

        assert_eq!(
            request.validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn conflict_contains_machine_readable_revisions() {
        let error = ProtocolError::conflict(18, 20);
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.details.unwrap()["currentSequence"], 20);
    }
}
