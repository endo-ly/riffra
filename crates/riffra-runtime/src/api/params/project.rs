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

/// Permanently deletes a Project that is not the active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectDeleteParams {
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

/// Topic of the Score YAML language specification to return.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScoreSpecTopic {
    /// Minimal working template and topic index.
    Overview,
    /// Top-level structure, meta, form, and scale_definitions.
    Structure,
    /// Degrees, octaves, relative_to, register, c-tokens, and tonic override.
    Pitch,
    /// Common time axis, pattern grammar, melodies, drums, articulations, swing, and timing_offset_steps.
    Pattern,
    /// Chord notation, modifiers, anticipation (<), voicing, comp, arps, and counters.
    Chords,
    /// Melodies inheritance (from/replace/use/pickup_steps), sections.from, and drums.overrides.
    Materials,
    /// Section lines, placements, audio_clips, time_changes, and partial bars.
    Sections,
    /// Instruments (built-in, VST3 source, kit, audio, effects), lanes, and kits.
    Instruments,
    /// Events, macros, priority rules, and VST3 articulation/CC control.
    Events,
    /// Mix routing (tracks, buses, sends, sidechain, master) and mix.automation.
    Mix,
    /// Syntax rules, identifier rules, diagnostics, unimplemented extensions, and field contracts.
    Diagnostics,
}

/// Returns the Score YAML language specification by topic.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct ScoreSpecParams {
    /// Topic to read; omit (or pass `overview`) for a minimal template and topic index.
    pub topic: Option<ScoreSpecTopic>,
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

/// Writes the active Project as a Score song folder.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectExportScoreParams {
    /// Song folder to create; it must not exist.
    pub path: PathBuf,
    /// Links the Project to the exported Score.
    #[serde(default)]
    pub link: bool,
}

/// Restores one saved generation of the active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectRestoreParams {
    pub file_name: String,
}
