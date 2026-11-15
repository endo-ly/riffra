//! Params of session-wide commands.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Params of `session.apply`.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionApplyParams {
    pub operations: Vec<BatchOperation>,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub include_created_ids: bool,
}

/// One undecoded `session.apply` operation.
///
/// Operations may reference Tracks and Clips by `trackName` / `clipName`,
/// which the batch resolves to IDs before decoding the command.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BatchOperation {
    pub command: String,
    #[ts(type = "unknown")]
    pub params: Value,
    /// Names exactly one device created by this operation for later references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::BatchOperation;
    use serde_json::json;

    #[test]
    fn bind_is_an_optional_string() {
        let operation = |bind| json!({"command": "effect.add", "params": {}, "bind": bind});
        assert_eq!(
            serde_json::from_value::<BatchOperation>(operation(json!(null)))
                .unwrap()
                .bind,
            None
        );
        assert_eq!(
            serde_json::from_value::<BatchOperation>(operation(json!("synth")))
                .unwrap()
                .bind
                .as_deref(),
            Some("synth")
        );
        for invalid in [json!(42), json!({})] {
            assert!(serde_json::from_value::<BatchOperation>(operation(invalid)).is_err());
        }
    }
}
