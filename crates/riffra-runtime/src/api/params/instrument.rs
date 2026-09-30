//! Params of instrument assignment and instrument library commands.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Saves a Sonalloy definition as a user instrument.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct InstrumentSaveParams {
    pub definition_path: PathBuf,
    /// Existing user instrument to overwrite.
    pub instrument_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentExportParams {
    pub instrument_id: String,
    pub output: PathBuf,
}

/// Assigns a `builtin:` or `user:` instrument to a Track.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentApplyParams {
    pub track_id: String,
    pub instrument_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentIdParams {
    pub instrument_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentFavoriteParams {
    pub instrument_id: String,
    pub favorite: bool,
}

/// Overrides the category; `null` restores the default category.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct InstrumentCategoryParams {
    pub instrument_id: String,
    pub category: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentTagsParams {
    pub instrument_id: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentCollectionIdParams {
    pub id: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentCollectionCreateParams {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentCollectionRenameParams {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentCollectionMembershipParams {
    pub collection_id: i64,
    pub instrument_id: String,
    pub included: bool,
}
