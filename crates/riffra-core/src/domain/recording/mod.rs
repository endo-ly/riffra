//! Recording session, pass, take, and source domain models.

use crate::domain::asset::AssetId;
use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Whether a recorded audio take is using its original or processed variant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AudioTakeVariant {
    #[default]
    Raw,
    Processed,
}

/// A persisted recording attempt group. Its takes remain available even when
/// only one take is currently placed on the timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionRecord {
    pub id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[serde(default)]
    pub track_slots: Vec<RecordingSessionTrackSlot>,
    #[serde(default)]
    pub pass_ids: Vec<String>,
}

/// The active take and stable Timeline Clip owned by one recorded Track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionTrackSlot {
    pub track_id: String,
    pub active_take_id: String,
    pub timeline_clip_id: String,
}

/// One pass through the recording range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPassRecord {
    pub id: String,
    pub session_id: String,
    pub ordinal: u32,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    #[serde(default)]
    pub partial_start: bool,
    #[serde(default)]
    pub partial_end: bool,
    #[serde(default)]
    pub track_take_ids: Vec<String>,
}

/// A recorded Track product belonging to one recording pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TakeAudioSource {
    pub asset_id: AssetId,
    #[ts(type = "number")]
    pub source_start_sample: u64,
    #[ts(type = "number")]
    pub source_end_sample: u64,
    #[serde(default)]
    #[ts(type = "number")]
    pub tail_end_sample: u64,
    #[serde(default)]
    pub sample_rate: u32,
}

/// A recorded Track product belonging to one recording pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingTakeRecord {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub pass_id: String,
    pub track_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub duration_ticks: u64,
    #[serde(default)]
    pub source_start_sample: u64,
    #[serde(default)]
    pub source_end_sample: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub raw_audio: Option<TakeAudioSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub processed_audio: Option<TakeAudioSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub midi_asset_id: Option<AssetId>,
}

impl RecordingTakeRecord {
    pub fn audio_source(&self, variant: AudioTakeVariant) -> Option<&TakeAudioSource> {
        match variant {
            AudioTakeVariant::Raw => self.raw_audio.as_ref(),
            AudioTakeVariant::Processed => self.processed_audio.as_ref(),
        }
    }

    pub fn preferred_audio_source(&self, variant: AudioTakeVariant) -> Option<&TakeAudioSource> {
        self.audio_source(variant).or(match variant {
            AudioTakeVariant::Raw => self.processed_audio.as_ref(),
            AudioTakeVariant::Processed => self.raw_audio.as_ref(),
        })
    }
}
