use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A protocol command independent of a CLI argument syntax.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlCommand {
    /// Stable command name understood by a backend.
    #[serde(rename = "command")]
    pub name: String,
    /// Command-specific JSON parameters.
    pub params: Value,
}

impl ControlCommand {
    /// Creates a command from its shared wire name and parameters.
    pub fn new(name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
        }
    }
}
