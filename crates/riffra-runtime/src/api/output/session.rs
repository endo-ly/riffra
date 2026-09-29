//! Canonical session results.

use riffra_core::{
    AudioInputRoute, CanonicalState, DeviceKind, MidiInputRoute, MonitoringState, MusicalDuration,
    MusicalPitch, MusicalPosition, RackDevice, RackMacro, Track, TrackInstrument,
    TrackInstrumentSource, TrackKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// Result of a canonical Arrangement mutation and its best-effort runtime
/// projection.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementMutationResult {
    pub canonical: CanonicalState,
    pub projection: ArrangementProjectionOutcome,
    pub created_entity_ids: BTreeMap<String, Vec<String>>,
}

/// Outcome of projecting a committed Arrangement mutation into the native
/// runtime.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ArrangementProjectionOutcome {
    NotRequired,
    Queued,
    Failed { message: String },
}

/// Lightweight Track projection used by `track.list`.
///
/// Device parameter arrays are intentionally omitted. The complete device
/// state remains available through the canonical `session.get` response.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    pub gain_db: f64,
    pub pan: f64,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitoring: MonitoringState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio_input: Option<AudioInputRoute>,
    pub midi_input: MidiInputRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instrument: Option<TrackInstrumentSummary>,
    pub rack: TrackRackSummary,
}

/// Device metadata included in a lightweight Track projection.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackDeviceSummary {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    pub bypassed: bool,
    pub gain_db: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
    pub disabled_placeholder: bool,
}

/// Instrument metadata included in a lightweight Track projection.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackInstrumentSummary {
    pub id: String,
    pub name: String,
    pub bypassed: bool,
    pub source: TrackInstrumentSummarySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preset_id: Option<String>,
}

/// Instrument implementation family included in a Track summary.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TrackInstrumentSummarySource {
    Internal,
    Vst3,
}

/// Rack metadata included in a lightweight Track projection.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackRackSummary {
    pub devices: Vec<TrackDeviceSummary>,
    pub macros: Vec<RackMacro>,
}

impl TrackSummary {
    pub(crate) fn from_track(track: &Track) -> Self {
        Self {
            id: track.id.clone(),
            name: track.name.clone(),
            kind: track.kind,
            gain_db: track.gain_db,
            pan: track.pan,
            muted: track.muted,
            solo: track.solo,
            armed: track.armed,
            monitoring: track.monitoring,
            color: track.color.clone(),
            audio_input: track.audio_input,
            midi_input: track.midi_input.clone(),
            instrument: track
                .instrument
                .as_ref()
                .map(TrackInstrumentSummary::from_instrument),
            rack: TrackRackSummary {
                devices: track
                    .rack
                    .devices
                    .iter()
                    .map(TrackDeviceSummary::from_device)
                    .collect(),
                macros: track.rack.macros.clone(),
            },
        }
    }
}

impl TrackDeviceSummary {
    fn from_device(device: &RackDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            kind: device.kind,
            path: device.path.clone(),
            bypassed: device.bypassed,
            gain_db: device.gain_db,
            state_data: device.state_data.clone(),
            disabled_placeholder: device.disabled_placeholder,
        }
    }
}

impl TrackInstrumentSummary {
    fn from_instrument(instrument: &TrackInstrument) -> Self {
        Self {
            id: instrument.id.clone(),
            name: instrument.name.clone(),
            bypassed: instrument.bypassed,
            source: match &instrument.source {
                TrackInstrumentSource::Internal { .. } => TrackInstrumentSummarySource::Internal,
                TrackInstrumentSource::Vst3 { .. } => TrackInstrumentSummarySource::Vst3,
            },
            preset_id: instrument.built_in_preset_id().map(str::to_owned),
        }
    }
}

/// Result of an atomic `session.apply` batch.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BatchMutationResult {
    pub applied_commands: usize,
    pub created_entity_counts: BTreeMap<String, usize>,
    /// Present only when the batch requested `includeCreatedIds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_entity_ids: Option<BTreeMap<String, Vec<String>>>,
    /// Present only when a live Host projected the committed batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub projection: Option<ArrangementProjectionOutcome>,
}

/// Notes a phrase would insert, resolved without changing canonical state.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PhrasePreview {
    pub note_count: usize,
    pub placement_count: usize,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
    /// Present only when the preview requested `includeNotes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notes: Option<Vec<PhrasePreviewNote>>,
}

/// One note of a [`PhrasePreview`].
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PhrasePreviewNote {
    pub pitch: MusicalPitch,
    pub position: MusicalPosition,
    pub duration: MusicalDuration,
    pub velocity: u8,
    pub channel: u8,
}
