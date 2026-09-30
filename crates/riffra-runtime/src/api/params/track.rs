//! Params of Track, marker, range, and automation commands.

use riffra_core::{
    AutomationParameter, AutomationPoint, MonitoringState, MusicalPosition, TrackKind,
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

/// Params of `loop-range.set` and `punch-range.set`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RangeParams {
    pub enabled: bool,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationSetParams {
    pub track_id: String,
    pub parameter: AutomationParameter,
    pub points: Vec<AutomationPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationClearParams {
    pub track_id: String,
    pub parameter: AutomationParameter,
}
