//! Params of session-wide commands.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Params of `session.apply`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
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
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BatchOperation {
    pub command: String,
    #[ts(type = "unknown")]
    pub params: Value,
    /// Names exactly one device created by this operation for later references.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bind"
    )]
    #[ts(as = "Option<String>", optional)]
    pub bind: Option<Value>,
}

fn deserialize_bind<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
