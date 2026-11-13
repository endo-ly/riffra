//! Score document results.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One structured diagnostic produced while reading a Score document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDiagnostic {
    /// Score field path owning the problem.
    pub path: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// One-based line of the problem, or zero when the source has no position.
    pub line: usize,
    /// One-based column of the problem, or zero when the source has no position.
    pub column: usize,
    /// Additional source metadata for the diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub detail: Option<serde_json::Value>,
}

/// Counts of the musical content a Score document expands to.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSummary {
    pub title: String,
    pub ticks_per_beat: u16,
    pub length_ticks: u64,
    pub section_count: usize,
    pub instrument_count: usize,
    pub lane_count: usize,
    pub note_count: usize,
    pub control_count: usize,
}

/// Result of validating a Score document without creating a Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreValidation {
    /// Whether the document could be resolved and expanded completely.
    pub valid: bool,
    /// Problems and warnings in source order.
    pub diagnostics: Vec<ScoreDiagnostic>,
    /// Expansion counts when the document is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary: Option<ScoreSummary>,
}

/// One expanded Score section occurrence.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSectionInspection {
    pub name: String,
    pub bars: usize,
    pub tonic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scale: Option<String>,
    pub bpm: f64,
    pub meter: String,
    pub start_tick: u64,
    pub end_tick: u64,
    pub chords: Vec<ScoreChordInspection>,
}

/// One resolved chord change in a Score section occurrence.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreChordInspection {
    pub tick: u64,
    pub chord: String,
}

/// One expanded Score lane and its resolved instruments.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreLaneInspection {
    pub id: String,
    pub gain_db: f64,
    pub notes: usize,
    pub controls: usize,
    /// Expanded events in timeline order with their Score source locations;
    /// empty unless the command requests them.
    #[ts(type = "unknown[]")]
    pub events: Vec<serde_json::Value>,
}

/// One Score instrument with the definition it resolves to.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreInstrumentInspection {
    pub id: String,
    pub name: String,
    /// Absolute definition path used for the resolved source.
    pub definition: String,
    /// Whether the definition is a bundled preset.
    pub built_in: bool,
}

/// Detailed structure expanded from one Score document.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreInspection {
    pub summary: ScoreSummary,
    pub sections: Vec<ScoreSectionInspection>,
    pub instruments: Vec<ScoreInstrumentInspection>,
    pub lanes: Vec<ScoreLaneInspection>,
}

/// Result of synchronizing the active Project with its linked Score file.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreRebuild {
    pub project_id: String,
    /// Whether the Score-owned performance changed.
    pub changed: bool,
    /// Whether the stored Score body changed.
    pub yaml_changed: bool,
    /// Canonical sequence after the synchronization.
    pub sequence: u64,
    /// Linked Score file when the Project has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    /// Diagnostics reported while accepting the Score.
    pub diagnostics: Vec<ScoreDiagnostic>,
}

/// Whether the active Project is synchronized with its linked Score file.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScoreSyncState {
    /// The active Project has no linked Score file.
    #[default]
    Unlinked,
    /// The stored Score matches the linked file.
    Synced,
    /// A changed Score file is being read and applied.
    Reading,
    /// The linked file could not be read or applied.
    Failed,
}

/// Synchronization state of the active Project's linked Score file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSyncStatus {
    pub state: ScoreSyncState,
    /// Project whose Score is synchronized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_id: Option<String>,
    /// Absolute path of the linked Score file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    /// Canonical sequence at the last successful acceptance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sequence: Option<u64>,
    /// Session update timestamp at the last successful acceptance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub updated_at_ms: Option<u64>,
    /// Diagnostics of the latest failed synchronization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ScoreDiagnostic>,
}
