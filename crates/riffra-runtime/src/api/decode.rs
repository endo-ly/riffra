//! Decoding of wire requests into typed Control Commands.

use riffra_control::{ErrorCode, ProtocolError};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use thiserror::Error;

/// A request that does not name a known command or carries invalid params.
#[derive(Debug, Error)]
pub enum CommandDecodeError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    /// `details` holds the JSON pointer `path` of the rejected value and, when
    /// small enough, the rejected `value` and innermost array `index`.
    #[error("invalid command parameters: {message}")]
    InvalidParams { message: String, details: Value },
}

impl CommandDecodeError {
    /// Returns the machine-readable details for this decoding failure.
    pub fn details(&self) -> Value {
        match self {
            Self::UnknownCommand(_) => serde_json::json!({"path": "/command"}),
            Self::InvalidParams { details, .. } => details.clone(),
        }
    }
}

impl From<CommandDecodeError> for ProtocolError {
    fn from(error: CommandDecodeError) -> Self {
        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
            .with_details(error.details())
    }
}

/// Decodes params, or a params fragment, reporting a rejected value by its JSON
/// pointer as [`ControlCommand::decode`](super::ControlCommand::decode) does.
///
/// # Errors
///
/// Returns [`CommandDecodeError::InvalidParams`] when `params` does not match `T`.
pub fn decode_params<T: DeserializeOwned>(params: Value) -> Result<T, CommandDecodeError> {
    let input = serde_json::to_vec(&params).expect("JSON values must serialize");
    let mut deserializer = serde_json::Deserializer::from_slice(&input);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let (path, index) = json_path_to_pointer(error.path());
        let mut details = Map::new();
        details.insert("path".into(), Value::String(path));
        if let Some(index) = index {
            details.insert("index".into(), index.into());
        }
        attach_input_value(&mut details, &params);
        CommandDecodeError::InvalidParams {
            message: error.inner().to_string(),
            details: Value::Object(details),
        }
    })
}

/// Adds the rejected input value at `details.path` when it is small enough to
/// echo back to the caller.
pub(crate) fn attach_input_value(details: &mut Map<String, Value>, params: &Value) {
    if details.contains_key("value") {
        return;
    }
    if let Some(value) = details
        .get("path")
        .and_then(Value::as_str)
        .and_then(|path| params.pointer(path))
        .filter(|value| should_include_error_value(value))
    {
        let value = value.clone();
        details.insert("value".into(), value);
    }
}

/// Escapes one JSON pointer reference token.
pub(crate) fn json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn json_path_to_pointer(path: &serde_path_to_error::Path) -> (String, Option<usize>) {
    let mut pointer = String::new();
    let mut innermost_index = None;
    for segment in path.iter() {
        match segment {
            serde_path_to_error::Segment::Map { key } => {
                pointer.push('/');
                pointer.push_str(&json_pointer_segment(key));
            }
            serde_path_to_error::Segment::Seq { index } => {
                pointer.push('/');
                pointer.push_str(&index.to_string());
                innermost_index = Some(*index);
            }
            serde_path_to_error::Segment::Enum { variant } => {
                pointer.push('/');
                pointer.push_str(&json_pointer_segment(variant));
            }
            serde_path_to_error::Segment::Unknown => {}
        }
    }
    (pointer, innermost_index)
}

fn should_include_error_value(value: &Value) -> bool {
    let small_shape = match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(values) => values.len() <= 4,
        Value::Object(values) => values.len() <= 8,
    };
    small_shape && serde_json::to_vec(value).is_ok_and(|value| value.len() <= 512)
}
