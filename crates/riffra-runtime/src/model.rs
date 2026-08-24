use riffra_core::CanonicalState;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Canonical session and audio status returned by a coordinated operation.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAudioPair {
    pub canonical: CanonicalState,
    pub audio: AudioStatus,
}

/// Coarse state of the native audio runtime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AudioState {
    /// No native process is available.
    #[default]
    Offline,
    /// The process exists but has not completed its handshake.
    Starting,
    /// The process is ready to accept commands.
    Ready,
    /// The process is connected but deliberately muted.
    Muted,
    /// The process or selected device faulted.
    Faulted,
}

/// Native recording status retained by the Host boundary.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
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

/// A channel exposed by an audio device probe.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioChannelInfo {
    pub index: u32,
    pub name: String,
}

/// An audio device and the channels exposed by its probe.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub channels: Vec<AudioChannelInfo>,
}

/// An external MIDI device reported by the audio runtime.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiDeviceInfo {
    pub id: String,
    pub name: String,
}

/// A native audio driver and its available devices.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverInfo {
    pub name: String,
    pub access_mode: AudioAccessMode,
    pub device_pairing: AudioDevicePairing,
    pub inputs: Vec<AudioDeviceInfo>,
    pub outputs: Vec<AudioDeviceInfo>,
}

/// Whether input and output devices are selected independently or as a pair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AudioDevicePairing {
    #[default]
    Independent,
    SameDevice,
}

/// Access mode exposed by a native audio driver.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AudioAccessMode {
    Shared,
    Exclusive,
    #[default]
    DriverManaged,
}

/// Result of an audio-device probe.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceProbe {
    pub drivers: Vec<AudioDriverInfo>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

/// Channel names resolved for a selected device.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceChannels {
    pub driver: String,
    pub input_device: String,
    pub input_channels: Vec<AudioChannelInfo>,
    pub output_device: String,
    pub output_channels: Vec<AudioChannelInfo>,
}

/// A native audio status snapshot.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
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

/// Latest-wins state of canonical arrangement projection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeProjectionState {
    #[default]
    Idle,
    Queued,
    Preparing,
    Active,
    Failed,
}

/// Observable projection state shared by GUI and headless Hosts.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
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

/// Result of a canonical Arrangement mutation and its best-effort runtime
/// projection.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementMutationResult {
    pub canonical: riffra_core::CanonicalState,
    pub projection: ArrangementProjectionOutcome,
}

/// Outcome of projecting a committed Arrangement mutation into the native
/// runtime.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ArrangementProjectionOutcome {
    NotRequired,
    Queued,
    Failed { message: String },
}

/// Result returned after a recording capture has been stopped.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStopResult {
    pub canonical: riffra_core::CanonicalState,
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
