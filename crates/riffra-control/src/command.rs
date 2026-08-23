use serde_json::Value;

/// A protocol command independent of a CLI argument syntax.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlCommand {
    /// Stable command name understood by a backend.
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
