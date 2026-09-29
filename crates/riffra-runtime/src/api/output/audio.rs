//! Audio device, runtime projection, and diagnostic results.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    #[serde(default)]
    pub processing: bool,
    pub cancelled: bool,
    pub directory: Option<String>,
    pub sample_rate: Option<u32>,
    pub samples_written: u64,
    #[serde(default)]
    pub dropped_midi_events: u64,
    pub dropped_blocks: u64,
    pub missing_samples: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
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
    /// Physical input indices currently enabled in the native device setup.
    /// This is routing state; `input_channels` remains the full capability list.
    pub active_input_channels: Vec<u32>,
    pub output_device: Option<String>,
    pub output_channels: Vec<AudioChannelInfo>,
    /// Physical output indices currently enabled in the native device setup.
    pub active_output_channels: Vec<u32>,
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
    /// Whether the prepared instrument preview is active.
    pub instrument_previewing: bool,
    /// Bitmask owned by the Native safety callback. Each bit identifies the
    /// owner that currently keeps the output muted.
    pub mute_reasons: u32,
    pub diagnostics: AudioDiagnostics,
    pub message: String,
}

/// Realtime counters exposed for diagnosing an unsafe or overloaded callback.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnostics {
    pub callback_count: u64,
    pub average_callback_duration_us: u64,
    pub maximum_callback_duration_us: u64,
    pub callback_overruns: u64,
    pub pre_limiter_peak: f64,
    pub limiter_gain_reduction_db: f64,
    pub hard_clip_samples: u64,
    pub live_midi_drops: u64,
    pub graph_revision: u64,
    pub graph_publish_count: u64,
    pub track_count: u64,
    pub instrument_runtime_count: u64,
    pub plugin_count: u64,
    pub maximum_latency_samples: u64,
    pub projection_duration_ms: u64,
    pub audio_environment_revision: u64,
    /// Sidecar output lines that did not match the protocol.
    pub protocol_errors: u64,
    #[serde(default)]
    pub instrument_faults: Vec<AudioInstrumentFault>,
}

/// Fault counters for one instrument runtime in the active graph.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioInstrumentFault {
    pub track_id: String,
    pub instrument_type: String,
    pub fault_code: u32,
    pub dropped_midi_events: u64,
}

/// Read-only diagnostic snapshot assembled from the live Host and Native
/// audio status without changing runtime state or resetting counters.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsReport {
    pub device: AudioDiagnosticsDevice,
    pub mute: AudioDiagnosticsMute,
    pub realtime: AudioDiagnosticsRealtime,
    pub output: AudioDiagnosticsOutput,
    pub instrument_faults: Vec<AudioInstrumentFault>,
}

/// Device values included in an audio diagnostic snapshot.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsDevice {
    pub state: AudioState,
    pub driver: Option<String>,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
    pub round_trip_ms: Option<f64>,
    pub active_input_channels: Vec<u32>,
    pub active_output_channels: Vec<u32>,
}

/// Native mute ownership expanded into stable named flags for diagnosis.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsMute {
    pub state: AudioState,
    pub raw_reasons: u32,
    pub user_emergency: bool,
    pub engine_transition: bool,
    pub device_fault: bool,
    pub feedback_protection: bool,
}

/// Internal projection values used while diagnosing audio lifecycle failures.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDiagnosticsProjection {
    pub state: RuntimeProjectionState,
    pub target_sequence: Option<u64>,
    pub active_sequence: Option<u64>,
    pub session_revision: Option<u64>,
    pub audio_environment_revision: u64,
    pub generation: u64,
    pub last_error: Option<String>,
    pub last_projection_duration_ms: u64,
}

/// Realtime callback values included in a stable audio diagnostic snapshot.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsRealtime {
    pub callback_count: u64,
    pub average_callback_duration_us: u64,
    pub maximum_callback_duration_us: u64,
    pub callback_overruns: u64,
}

/// Output safety values included in a stable audio diagnostic snapshot.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsOutput {
    pub pre_limiter_peak: f64,
    pub limiter_gain_reduction_db: f64,
    pub hard_clip_samples: u64,
    pub output_peak: f64,
    pub invalid_samples: u64,
}

/// Internal graph and MIDI values used while diagnosing projection failures.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDiagnosticsTimeline {
    pub track_count: u64,
    pub instrument_runtime_count: u64,
    pub plugin_count: u64,
    pub maximum_latency_samples: u64,
    pub graph_revision: u64,
    pub graph_publish_count: u64,
    pub live_midi_drops: u64,
}

/// Internal diagnostic details that are intentionally outside the stable
/// `audio.diagnostics` contract.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDiagnosticsDebug {
    pub(crate) projection: AudioDiagnosticsProjection,
    pub(crate) timeline: AudioDiagnosticsTimeline,
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
    pub audio_environment_revision: u64,
    pub target_audio_environment_revision: Option<u64>,
    pub prepared_audio_environment_revision: Option<u64>,
    pub active_audio_environment_revision: Option<u64>,
    pub active_diagnostics: Option<ProjectionDiagnostics>,
    pub queued_at_ms: Option<u64>,
    pub started_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub last_native_response_at_ms: Option<u64>,
    pub discarded_preparation_count: u64,
    pub last_error: Option<String>,
    pub last_error_code: Option<String>,
}

/// Resources missing from an executable projection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDiagnostics {
    pub unavailable_clip_ids: Vec<String>,
    pub missing_device_ids: Vec<String>,
}
