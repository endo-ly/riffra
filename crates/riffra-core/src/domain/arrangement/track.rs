use crate::domain::rack::{RackDevice, RackInstance};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The production source hosted by a timeline track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Instrument,
}

/// A physical input channel routed to one Audio Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputRoute {
    pub channel_index: u32,
}

/// A MIDI source and channel filter routed to one Instrument Track.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiInputRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<u8>,
}

/// A timeline track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub monitoring: MonitoringState,
    /// Presentation color as `#rrggbb`. `None` delegates automatic coloring
    /// to the presentation layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio_input: Option<AudioInputRoute>,
    #[serde(default)]
    pub midi_input: MidiInputRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instrument: Option<RackDevice>,
    pub rack: RackInstance,
}

/// A partial update for a timeline Track.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitoring: Option<MonitoringState>,
    /// Sets the presentation color; an empty string clears it and returns
    /// the track to automatic coloring.
    pub color: Option<String>,
}

/// Audio Track input monitoring state. `Auto` monitors only while the track is
/// armed; `On` always monitors; `Off` never monitors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringState {
    #[default]
    Off,
    Auto,
    On,
}

fn empty_track_rack() -> RackInstance {
    RackInstance {
        devices: Vec::new(),
        macros: Vec::new(),
    }
}

impl Track {
    /// Creates a neutral audio track.
    pub fn audio(id: String, name: String) -> Self {
        Self {
            id,
            name,
            kind: TrackKind::Audio,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: MonitoringState::Off,
            color: None,
            audio_input: None,
            midi_input: MidiInputRoute::default(),
            instrument: None,
            rack: empty_track_rack(),
        }
    }

    /// Creates a neutral Instrument Track with no assigned instrument.
    pub fn instrument(id: String, name: String) -> Self {
        Self {
            kind: TrackKind::Instrument,
            ..Self::audio(id, name)
        }
    }
}
