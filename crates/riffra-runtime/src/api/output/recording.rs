//! Recording capture results.

use super::{ArrangementProjectionOutcome, AudioStatus};
use riffra_core::{AssetId, CanonicalState};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Result returned after a recording capture has been stopped.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStopResult {
    pub canonical: CanonicalState,
    pub audio: AudioStatus,
    pub projection: ArrangementProjectionOutcome,
    pub finalization: RecordingFinalizationOutcome,
}

/// Describes the immediate outcome of stopping a recording capture.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RecordingFinalizationOutcome {
    NotRequired,
    Processing,
    RecoveryRequired { message: String },
}

/// UI read model assembled from the capture manifest and canonical Assets.
///
/// This type is never used as the persistent recording domain. The path fields
/// are resolved/display-oriented data for Recovery; completed captures use
/// their Asset IDs as the authoritative identity.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub state: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub raw_file: Option<String>,
    pub processed_file: Option<String>,
    pub raw_path: Option<String>,
    pub processed_path: Option<String>,
    pub raw_asset_id: Option<AssetId>,
    pub processed_asset_id: Option<AssetId>,
    pub midi_asset_id: Option<AssetId>,
    pub capture: Option<RecordingCapture>,
    pub midi_file: Option<String>,
    pub sample_rate: Option<u32>,
    pub samples_written: u64,
    pub dropped_midi_events: u64,
    pub dropped_blocks: u64,
    pub missing_samples: u64,
    pub dropout_start_sample: Option<u64>,
    pub dropout_end_sample: Option<u64>,
    pub raw_attempted_samples: u64,
    pub processed_attempted_samples: u64,
    pub raw_dropped_blocks: u64,
    pub processed_dropped_blocks: u64,
    pub raw_missing_samples: u64,
    pub processed_missing_samples: u64,
    pub raw_dropout_start_sample: Option<u64>,
    pub raw_dropout_end_sample: Option<u64>,
    pub processed_dropout_start_sample: Option<u64>,
    pub processed_dropout_end_sample: Option<u64>,
    pub recovery_status: String,
}

/// The status of a [`RecordingCapture`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RecordingCaptureStatus {
    Recording,
    Completing,
    Completed,
    Recoverable,
    Failed,
}

/// Dropout/drop diagnostics captured during recording.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DropoutInformation {
    #[serde(default)]
    pub samples_written: u64,
    #[serde(default)]
    pub dropped_midi_events: u64,
    #[serde(default)]
    pub dropped_blocks: u64,
    #[serde(default)]
    pub missing_samples: u64,
    #[serde(default)]
    pub dropout_start_sample: Option<u64>,
    #[serde(default)]
    pub dropout_end_sample: Option<u64>,
    #[serde(default)]
    pub raw_attempted_samples: u64,
    #[serde(default)]
    pub processed_attempted_samples: u64,
    #[serde(default)]
    pub raw_dropped_blocks: u64,
    #[serde(default)]
    pub processed_dropped_blocks: u64,
    #[serde(default)]
    pub raw_missing_samples: u64,
    #[serde(default)]
    pub processed_missing_samples: u64,
    #[serde(default)]
    pub raw_dropout_start_sample: Option<u64>,
    #[serde(default)]
    pub raw_dropout_end_sample: Option<u64>,
    #[serde(default)]
    pub processed_dropout_start_sample: Option<u64>,
    #[serde(default)]
    pub processed_dropout_end_sample: Option<u64>,
}

/// One recording event and the assets it produced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingCapture {
    pub capture_id: String,
    pub session_id: String,
    pub status: RecordingCaptureStatus,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input_device: Option<String>,
    pub audio_driver: Option<String>,
    pub input_channel: Option<u32>,
    pub input_channel_name: Option<String>,
    pub buffer_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub master_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_in_beats: Option<u8>,
    #[serde(default)]
    #[ts(type = "number")]
    pub timeline_start_tick: u64,
    #[serde(default)]
    pub armed_track_ids: Vec<String>,
    #[serde(default)]
    pub loop_recording: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub raw_audio_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub processed_audio_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub midi_asset_id: Option<AssetId>,
    #[serde(default)]
    pub dropout_information: DropoutInformation,
}

impl RecordingCaptureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Completing => "completing",
            Self::Completed => "completed",
            Self::Recoverable => "recoverable",
            Self::Failed => "failed",
        }
    }
}
