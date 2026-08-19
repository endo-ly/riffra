use serde::{Deserialize, Serialize};
use ts_rs::TS;

// Shared production types live in feature modules; this module owns the
// application-level audio and runtime status types.

/// A paired session and audio status returned by operations that update both
/// the Audio Runtime and the persisted `CreativeSession`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAudioPair {
    pub session: riffra_core::CreativeSession,
    pub audio: AudioStatus,
}

/// Result of stopping a recording, including the session visible after stop,
/// audio status, and separate finalization/projection outcomes. A recovery
/// result keeps stopped files visible even when no canonical commit occurred.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStopResult {
    pub session: riffra_core::CreativeSession,
    pub audio: AudioStatus,
    pub projection: ArrangementProjectionOutcome,
    pub finalization: RecordingFinalizationOutcome,
}

/// Describes whether stopped recording outputs were committed to the
/// Arrangement or remain available for Inbox recovery.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RecordingFinalizationOutcome {
    NotRequired,
    Completed,
    RecoveryRequired { message: String },
}

/// Result of a canonical Arrangement mutation and its best-effort Audio
/// Runtime projection. The canonical Session is committed before projection,
/// so a failed projection is reported alongside the committed Session.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementMutationResult {
    pub session: riffra_core::CreativeSession,
    pub projection: ArrangementProjectionOutcome,
}

/// Outcome of projecting a committed Arrangement mutation into the Audio
/// Runtime. This is deliberately discriminated so callers never infer state
/// from a human-readable error message.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ArrangementProjectionOutcome {
    NotRequired,
    Queued {
        status: RuntimeProjectionStatus,
    },
    Failed {
        status: RuntimeProjectionStatus,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeProjectionState {
    #[default]
    Idle,
    Queued,
    Preparing,
    Active,
    Failed,
}

/// Describes how far the latest persisted arrangement has been reflected in
/// the isolated Audio Runtime. A target can remain queued while an older VST
/// operation is still returning; the active revision is the graph currently
/// visible to the audio callback.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProjectionStatus {
    pub state: RuntimeProjectionState,
    pub operation_id: u64,
    pub running_operation_id: Option<u64>,
    pub target_projection_sequence: Option<u64>,
    pub target_session_revision: Option<u64>,
    pub prepared_session_revision: Option<u64>,
    pub active_projection_sequence: Option<u64>,
    pub active_session_revision: Option<u64>,
    pub runtime_generation: u64,
    pub queued_at_ms: Option<u64>,
    pub started_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub last_native_response_at_ms: Option<u64>,
    pub discarded_preparation_count: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub updated_at_ms: u64,
    pub session_id: String,
    pub project_name: Option<String>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub session: riffra_core::CreativeSession,
    pub plugin_catalog: Vec<crate::plugins::PluginEntry>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
    pub native_available: bool,
    pub recovery_candidates: Vec<RecoveryCandidate>,
    pub data_root: String,
    pub vst3_root: String,
}

/// Reports the result of a Session audio-graph restoration attempt to the UI.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeStartupFinishedEvent {
    pub succeeded: bool,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub active: bool,
    pub cancelled: bool,
    pub directory: Option<String>,
    pub sample_rate: Option<u32>,
    pub raw_channels: Option<u32>,
    pub processed_channels: Option<u32>,
    pub samples_written: u64,
    #[serde(default)]
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioChannelInfo {
    pub index: u32,
    pub name: String,
}

/// An audio device and the channels exposed by the device probe.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub channels: Vec<AudioChannelInfo>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub state: AudioState,
    pub driver: Option<String>,
    pub input_device: Option<String>,
    pub input_channel: Option<u32>,
    pub input_channels: Vec<AudioChannelInfo>,
    pub output_device: Option<String>,
    pub output_channels: Vec<AudioChannelInfo>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
    pub round_trip_ms: Option<f64>,
    #[serde(default)]
    pub timeline_tick: Option<u64>,
    pub recording: RecordingStatus,
    pub midi_inputs: Vec<MidiDeviceInfo>,
    pub midi_outputs: Vec<MidiDeviceInfo>,
    pub midi_input_active: bool,
    pub midi_messages: u64,
    pub last_midi_note: Option<u8>,
    pub input_peak: f64,
    pub output_peak: f64,
    pub invalid_samples: u64,
    pub feedback_suspected: bool,
    pub previewing: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiDeviceInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverInfo {
    pub name: String,
    pub access_mode: AudioAccessMode,
    pub device_pairing: AudioDevicePairing,
    pub inputs: Vec<AudioDeviceInfo>,
    pub outputs: Vec<AudioDeviceInfo>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AudioDevicePairing {
    #[default]
    Independent,
    SameDevice,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AudioAccessMode {
    Shared,
    Exclusive,
    #[default]
    DriverManaged,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceProbe {
    pub drivers: Vec<AudioDriverInfo>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

/// Channel names resolved lazily for a single selected device from Audio
/// Settings. Startup discovery stays passive (no device open); this detail is
/// fetched only when the user configures a specific device, opening it once.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceChannels {
    pub driver: String,
    pub input_device: String,
    pub input_channels: Vec<AudioChannelInfo>,
    pub output_device: String,
    pub output_channels: Vec<AudioChannelInfo>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AudioState {
    Offline,
    Starting,
    Ready,
    Muted,
    Faulted,
}
