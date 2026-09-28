//! Messages written by `riffra-audio` and the response of `riffra-render`.
//!
//! Every struct denies unknown keys and every `Option` key is mandatory, so a
//! change in the native encoder fails decoding instead of degrading silently.

use super::commands::ExpectedResponse;
use super::nullable;
use serde::Deserialize;
use serde_json::Value;

/// One line written by the realtime sidecar.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum SidecarMessage {
    /// The successful completion of one request.
    Response {
        request_id: u64,
        response: SidecarResponse,
    },
    /// The failed completion of one request.
    Error {
        request_id: u64,
        error: SidecarError,
    },
    /// A notification that belongs to no request.
    Event { event: SidecarEvent },
}

/// Successful completion payload of one request.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum SidecarResponse {
    AudioStatus(Box<WireAudioStatus>),
    TransportStatus(WireTransportStatus),
    Ack {},
    TrackDeviceStatus(WireTrackDeviceStatus),
    TrackDeviceParameters(WireTrackDeviceParameters),
    TrackDevicePrograms(WireTrackDevicePrograms),
    TrackPluginState(WireTrackPluginState),
}

impl SidecarResponse {
    /// Returns the response type used to match the pending request.
    pub(crate) fn kind(&self) -> ExpectedResponse {
        match self {
            Self::AudioStatus(_) => ExpectedResponse::AudioStatus,
            Self::TransportStatus(_) => ExpectedResponse::TransportStatus,
            Self::Ack {} => ExpectedResponse::Ack,
            Self::TrackDeviceStatus(_) => ExpectedResponse::TrackDeviceStatus,
            Self::TrackDeviceParameters(_) => ExpectedResponse::TrackDeviceParameters,
            Self::TrackDevicePrograms(_) => ExpectedResponse::TrackDevicePrograms,
            Self::TrackPluginState(_) => ExpectedResponse::TrackPluginState,
        }
    }
}

/// A structured sidecar failure.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SidecarError {
    pub(crate) kind: String,
    pub(crate) message: String,
    pub(crate) operation: String,
    /// Kind-specific diagnostic values.
    #[serde(deserialize_with = "nullable")]
    pub(crate) details: Option<Value>,
}

/// A notification written independently of any request.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum SidecarEvent {
    /// The first message of a sidecar generation.
    Ready {
        protocol_version: u32,
        status: Box<WireAudioStatus>,
    },
    AudioStatus(Box<WireAudioStatus>),
    AudioMeters(WireAudioMeters),
    TransportStatus(WireTransportStatus),
    RecordingComplete(WireRecordingComplete),
    TrackPluginStateChanged(WireTrackPluginStateChanged),
    TrackPluginParameterChanged(WireTrackPluginParameterChanged),
    /// A failure that is not tied to a request, such as a lost device.
    Fault(SidecarError),
}

/// Output state computed by the native safety chain.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WireAudioState {
    Ready,
    Muted,
    Faulted,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireAudioStatus {
    pub(crate) state: WireAudioState,
    pub(crate) message: String,
    #[serde(deserialize_with = "nullable")]
    pub(crate) driver: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) input_device: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) input_channel: Option<u32>,
    pub(crate) input_channels: Vec<WireAudioChannel>,
    pub(crate) active_input_channels: Vec<u32>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) output_device: Option<String>,
    pub(crate) output_channels: Vec<WireAudioChannel>,
    pub(crate) active_output_channels: Vec<u32>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) sample_rate: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) buffer_size: Option<u32>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) round_trip_ms: Option<f64>,
    /// Transport position of the active graph; `null` before a graph exists.
    #[serde(deserialize_with = "nullable")]
    pub(crate) timeline_tick: Option<u64>,
    pub(crate) recording: WireRecordingStatus,
    pub(crate) midi_inputs: Vec<WireMidiDevice>,
    pub(crate) midi_outputs: Vec<WireMidiDevice>,
    pub(crate) midi_input_active: bool,
    pub(crate) midi_messages: u64,
    #[serde(deserialize_with = "nullable")]
    pub(crate) last_midi_note: Option<u8>,
    pub(crate) input_peak: f64,
    pub(crate) output_peak: f64,
    pub(crate) invalid_samples: u64,
    pub(crate) feedback_suspected: bool,
    pub(crate) previewing: bool,
    pub(crate) instrument_previewing: bool,
    pub(crate) mute_reasons: u32,
    pub(crate) diagnostics: WireAudioDiagnostics,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireAudioChannel {
    pub(crate) index: u32,
    pub(crate) name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireMidiDevice {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Whether a capture lost any audio block or MIDI event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WireRecoveryStatus {
    Clean,
    Partial,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireRecordingStatus {
    pub(crate) active: bool,
    pub(crate) processing: bool,
    pub(crate) cancelled: bool,
    #[serde(deserialize_with = "nullable")]
    pub(crate) directory: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) sample_rate: Option<f64>,
    pub(crate) samples_written: u64,
    pub(crate) dropped_midi_events: u64,
    pub(crate) dropped_blocks: u64,
    pub(crate) missing_samples: u64,
    pub(crate) raw_attempted_samples: u64,
    pub(crate) processed_attempted_samples: u64,
    pub(crate) raw_dropped_blocks: u64,
    pub(crate) processed_dropped_blocks: u64,
    pub(crate) raw_missing_samples: u64,
    pub(crate) processed_missing_samples: u64,
    #[serde(deserialize_with = "nullable")]
    pub(crate) raw_dropout_start_sample: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) raw_dropout_end_sample: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) processed_dropout_start_sample: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) processed_dropout_end_sample: Option<u64>,
    pub(crate) recovery_status: WireRecoveryStatus,
    #[serde(deserialize_with = "nullable")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireAudioDiagnostics {
    pub(crate) callback_count: u64,
    pub(crate) average_callback_duration_us: u64,
    pub(crate) maximum_callback_duration_us: u64,
    pub(crate) callback_overruns: u64,
    pub(crate) pre_limiter_peak: f64,
    pub(crate) limiter_gain_reduction_db: f64,
    pub(crate) hard_clip_samples: u64,
    pub(crate) live_midi_drops: u64,
    pub(crate) graph_revision: u64,
    pub(crate) graph_publish_count: u64,
    pub(crate) track_count: u64,
    pub(crate) instrument_runtime_count: u64,
    pub(crate) plugin_count: u64,
    pub(crate) maximum_latency_samples: u64,
    pub(crate) instrument_faults: Vec<WireInstrumentFault>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireInstrumentFault {
    pub(crate) track_id: String,
    pub(crate) instrument_type: String,
    pub(crate) fault_code: u32,
    pub(crate) dropped_midi_events: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireAudioMeters {
    /// Project that owns the metered graph; `null` before a graph exists.
    #[serde(deserialize_with = "nullable")]
    pub(crate) project_id: Option<String>,
    pub(crate) input_peak: f64,
    pub(crate) output_peak: f64,
    pub(crate) output_peak_left: f64,
    pub(crate) output_peak_right: f64,
    pub(crate) invalid_samples: u64,
    pub(crate) pre_limiter_peak: f64,
    pub(crate) limiter_gain_reduction_db: f64,
    pub(crate) hard_clip_samples: u64,
    pub(crate) mute_reasons: u32,
    pub(crate) feedback_suspected: bool,
    pub(crate) previewing: bool,
    pub(crate) instrument_previewing: bool,
    pub(crate) track_meters: Vec<WireTrackMeter>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackMeter {
    pub(crate) track_id: String,
    pub(crate) peak_left: f64,
    pub(crate) peak_right: f64,
    pub(crate) rms_left: f64,
    pub(crate) rms_right: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WireTransportState {
    Stopped,
    Starting,
    Playing,
    Faulted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WireRecordingPhase {
    Idle,
    CountingIn,
    Recording,
    Stopping,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTransportStatus {
    pub(crate) state: WireTransportState,
    /// Revision of the active graph; `null` before a graph exists.
    #[serde(deserialize_with = "nullable")]
    pub(crate) revision: Option<u64>,
    pub(crate) timeline_tick: u64,
    pub(crate) sequence: u64,
    pub(crate) recording_phase: WireRecordingPhase,
    pub(crate) recording_start_tick: u64,
    pub(crate) recording_pass_ordinal: u32,
    pub(crate) armed_track_ids: Vec<String>,
    pub(crate) clock_generation: u64,
    pub(crate) discontinuity: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireRecordingComplete {
    pub(crate) directory: String,
    pub(crate) success: bool,
    #[serde(deserialize_with = "nullable")]
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WirePluginState {
    pub(crate) parameter_values: Vec<f32>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) state_data: Option<String>,
    pub(crate) bypassed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackPluginState {
    pub(crate) state: WirePluginState,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDeviceCapabilities {
    pub(crate) parameters: bool,
    pub(crate) state: bool,
    pub(crate) presets: bool,
    pub(crate) editor: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDeviceStatus {
    pub(crate) name: String,
    pub(crate) bypassed: bool,
    pub(crate) parameter_count: u32,
    pub(crate) capabilities: WireTrackDeviceCapabilities,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDeviceParameter {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) value: f32,
    pub(crate) default_value: f32,
    pub(crate) automatable: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDeviceParameters {
    pub(crate) parameters: Vec<WireTrackDeviceParameter>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDeviceProgram {
    pub(crate) index: u32,
    pub(crate) name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackDevicePrograms {
    /// Selected program; `null` when the plugin reports none.
    #[serde(deserialize_with = "nullable")]
    pub(crate) current_index: Option<u32>,
    pub(crate) programs: Vec<WireTrackDeviceProgram>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackPluginStateChanged {
    pub(crate) project_id: String,
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) state: WirePluginState,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireTrackPluginParameterChanged {
    pub(crate) project_id: String,
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) parameter_index: u32,
    pub(crate) value: f32,
}

/// The one line written by `riffra-render`.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum RenderMessage {
    OfflineRenderComplete { frames: u64, sample_rate: u32 },
    Error(SidecarError),
}
