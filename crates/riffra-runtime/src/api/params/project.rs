//! Params of Project container commands.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct ProjectCreateParams {
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectOpenParams {
    pub project_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectRenameParams {
    pub name: String,
}

/// Imports a `.riffra` package manifest as a new Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectImportParams {
    pub path: PathBuf,
}

/// Imports a Score YAML file as a new active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectImportScoreParams {
    /// Score YAML file.
    pub path: PathBuf,
    /// Sonalloy definition JSON for each `external: true` instrument ID.
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, PathBuf>,
}

/// Reads a Score YAML file without changing any Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScoreDocumentParams {
    /// Score YAML file.
    pub path: PathBuf,
    /// Sonalloy definition JSON for each `external: true` instrument ID.
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, PathBuf>,
}

/// Expands a Score YAML file into its resolved structure.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScoreInspectParams {
    /// Score YAML file.
    pub path: PathBuf,
    /// Sonalloy definition JSON for each `external: true` instrument ID.
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, PathBuf>,
    /// Include every expanded event; the default reports only counts per lane.
    #[serde(default)]
    pub events: bool,
}

/// Synchronizes the active Project with its linked Score YAML file.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct ProjectRebuildScoreParams {
    /// Score YAML file; omit to read the Project's linked file.
    pub path: Option<PathBuf>,
    /// External instrument binding added or replaced as ID=DEFINITION.
    #[serde(default)]
    pub bindings: std::collections::BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectExportParams {
    pub output: PathBuf,
}

/// Restores one saved generation of the active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectRestoreParams {
    pub file_name: String,
}
