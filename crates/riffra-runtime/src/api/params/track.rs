//! Params of Track, marker, range, and automation commands.

use riffra_core::{
    AutomationInterpolation, AutomationTarget, EffectOwner, MonitoringState, MusicalPosition,
    TrackKind,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackAddParams {
    pub name: String,
    pub kind: TrackKind,
}

/// Params of `track.update`; absent fields keep their current value.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct TrackUpdateParams {
    pub track_id: String,
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitoring: Option<MonitoringState>,
    /// An empty string clears the color.
    pub color: Option<String>,
    pub pan_law: Option<riffra_core::PanLaw>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackReorderParams {
    pub track_id: String,
    pub target_index: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioInputParams {
    pub track_id: String,
    pub channel_index: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MidiInputParams {
    pub track_id: String,
    pub device_id: Option<String>,
    pub channel: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MarkerAddParams {
    pub name: String,
    pub position: MusicalPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MarkerUpdateParams {
    pub marker_id: String,
    pub name: Option<String>,
    pub position: Option<MusicalPosition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MarkerIdParams {
    pub marker_id: String,
}

/// Params of `timebase.update`; at least one field is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct TimebaseUpdateParams {
    pub bpm: Option<f64>,
    pub time_signature_numerator: Option<u8>,
    pub time_signature_denominator: Option<u8>,
}

/// Replaces all tempo and meter changes while preserving the current project PPQ.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TimebaseSetMapParams {
    pub tempo_changes: Vec<riffra_core::TempoChange>,
    pub time_signature_changes: Vec<riffra_core::TimeSignatureChange>,
}

/// Params of `loop-range.set` and `punch-range.set`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RangeParams {
    pub enabled: bool,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
}

/// Replaces one canonical lane; an empty point array clears it.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationSetParams {
    pub target: AutomationTarget,
    pub interpolation: AutomationInterpolation,
    pub points: Vec<AutomationPointInput>,
}

/// One point supplied in canonical units or plugin-provided display text.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationPointInput {
    pub id: String,
    pub tick: u64,
    pub value: AutomationPointValue,
}

/// Display strings are converted through the live plugin; canonical state stores numbers.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(untagged)]
pub enum AutomationPointValue {
    Number(f64),
    Display(String),
}

/// Reads the lane for a single target.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationGetParams {
    pub target: AutomationTarget,
}

/// Discovers lanes across the project or a canonical channel owner.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct AutomationListParams {
    pub owner: Option<EffectOwner>,
}

/// Removes the lane for a single target.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationClearParams {
    pub target: AutomationTarget,
}
