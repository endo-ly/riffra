//! Score document results.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One structured diagnostic produced while reading a Score document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDiagnostic {
    pub severity: ScoreDiagnosticSeverity,
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

/// Severity of a Score diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScoreDiagnosticSeverity {
    Error,
    Warning,
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

/// One Score instrument and the source it resolves to.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreInstrumentInspection {
    pub id: String,
    pub name: String,
    pub kind: ScoreInstrumentKind,
    /// Absolute definition path for a Sonalloy source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub definition: Option<String>,
    /// Whether the definition is a bundled preset.
    pub built_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kit: Option<String>,
}

/// Execution source used by a Score instrument.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScoreInstrumentKind {
    Builtin,
    Vst3,
    Audio,
    KitMember,
}

/// One Score-defined mixing bus.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBusInspection {
    pub id: String,
    pub kind: String,
    pub gain_db: f64,
    pub pan: f64,
}

/// One parallel send in the Score mix.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSendInspection {
    pub from: String,
    pub bus: String,
    pub level_db: f64,
    pub position: String,
    pub enabled: bool,
}

/// One sidechain route in the Score mix.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSidechainInspection {
    pub from: String,
    pub owner_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_id: Option<String>,
    pub effect: String,
    pub level_db: f64,
    pub position: String,
    pub enabled: bool,
}

/// One audio clip specification and its expanded musical positions.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreAudioClipInspection {
    pub id: String,
    pub section: String,
    pub instrument: String,
    pub file: String,
    pub start_ticks: Vec<u64>,
    pub offset_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub duration_seconds: Option<f64>,
    pub gain_db: f64,
    pub pan: f64,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
}

/// One Track generated from a Score instrument, kit, or audio declaration.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreTrackInspection {
    /// Logical Score instrument or kit id.
    pub id: String,
    /// `instrument`, `kit`, or `audio`.
    pub kind: String,
    pub track_id: String,
    pub name: String,
    pub gain_db: f64,
    pub pan: f64,
    /// Score bus id, or `None` for the master output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_identifier: Option<String>,
    pub effect_ids: Vec<String>,
    pub note_count: usize,
}

/// One Kit Track aggregated from member trigger notes.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreKitInspection {
    pub id: String,
    pub track_id: String,
    pub device_id: String,
    pub plugin_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_identifier: Option<String>,
    pub members: Vec<String>,
    pub effect_ids: Vec<String>,
    pub note_count: usize,
}

/// One effect device generated from a Score effect chain.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreEffectInspection {
    /// `instrument`, `kit`, `audio`, `bus`, or `master`.
    pub owner_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub owner_id: Option<String>,
    /// Score effect id inside its owner chain.
    pub id: String,
    pub plugin_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin_identifier: Option<String>,
    pub plugin_path: String,
    pub bypassed: bool,
}

/// One automation lane generated from the Score mix.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreAutomationInspection {
    pub target: String,
    pub interpolation: String,
    pub points: usize,
}

/// Detailed structure expanded from one Score document.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreInspection {
    pub tracks: Vec<ScoreTrackInspection>,
    pub kits: Vec<ScoreKitInspection>,
    pub effects: Vec<ScoreEffectInspection>,
    pub automation: Vec<ScoreAutomationInspection>,
    pub summary: ScoreSummary,
    pub sections: Vec<ScoreSectionInspection>,
    pub instruments: Vec<ScoreInstrumentInspection>,
    pub lanes: Vec<ScoreLaneInspection>,
    pub diagnostics: Vec<ScoreDiagnostic>,
    pub buses: Vec<ScoreBusInspection>,
    pub sends: Vec<ScoreSendInspection>,
    pub sidechains: Vec<ScoreSidechainInspection>,
    pub audio_clips: Vec<ScoreAudioClipInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub master_gain_db: Option<f64>,
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
    /// Whether the linked file path or external instrument bindings changed.
    pub link_changed: bool,
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
