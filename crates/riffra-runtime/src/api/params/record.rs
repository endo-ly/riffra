//! Params of recording commands.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Starts a recording; an existing recording session records another take.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RecordStartParams {
    pub recording_session_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RecordListParams {
    pub query: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RecordRenameParams {
    pub id: String,
    pub new_name: String,
}
