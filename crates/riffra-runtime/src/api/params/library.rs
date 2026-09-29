//! Params of Library and Asset commands.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LibrarySearchParams {
    pub query: String,
}

/// Updates the Library tag and note of an Asset or recording take.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct LibraryTagParams {
    pub id: String,
    pub tag: Option<String>,
    pub note: Option<String>,
}

/// Imports a Standard MIDI File as an Asset.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct AssetImportParams {
    pub path: PathBuf,
    pub name: Option<String>,
}
