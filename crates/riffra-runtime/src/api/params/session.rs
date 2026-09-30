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
}
