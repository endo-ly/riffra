//! Canonical instrument control editing parameters.

use riffra_core::{InstrumentControlEvent, InstrumentControlEventKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Identifies the MIDI Clip whose instrument controls are inspected or edited.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentEventClipParams {
    pub clip_id: String,
}

/// Adds a native precision control at a clip-relative tick.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentEventAddParams {
    pub clip_id: String,
    pub tick: u64,
    pub kind: InstrumentControlEventKind,
}

/// Replaces one control, retaining its canonical identity.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentEventUpdateParams {
    pub clip_id: String,
    pub event: InstrumentControlEvent,
}

/// Replaces the complete ordered control array in one history operation.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentEventSetParams {
    pub clip_id: String,
    pub events: Vec<InstrumentControlEvent>,
}

/// Removes one control by its canonical identity.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentEventRemoveParams {
    pub clip_id: String,
    pub event_id: String,
}

/// Connects one instrument Track to another Track's processed audio.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackExternalAudioInputSetParams {
    pub track_id: String,
    pub source_track_id: String,
}
