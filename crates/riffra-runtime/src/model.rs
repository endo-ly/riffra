use crate::api::output::AudioInstrumentFault;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Transport state of the native timeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransportState {
    Stopped,
    Starting,
    Playing,
    Faulted,
}

/// Arrange-recording phase of the native timeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RecordingPhase {
    Idle,
    CountingIn,
    Recording,
    Stopping,
}

/// Live transport snapshot published by the native timeline.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TransportStatus {
    pub state: TransportState,
    /// Revision of the active graph; `None` when no graph could be read.
    pub revision: Option<u64>,
    pub timeline_tick: u64,
    /// Timeline position in output samples, including a pending seek.
    pub timeline_sample: i64,
    /// Samples processed by the audio device since the clock generation began.
    pub audio_clock_sample: u64,
    /// Output sample rate of the active graph; `None` when no graph could be read.
    pub sample_rate: Option<f64>,
    /// Sequence of the last transport or recording command applied by the audio thread.
    pub applied_command_sequence: u64,
    pub recording_phase: RecordingPhase,
    pub recording_start_tick: u64,
    pub recording_pass_ordinal: u32,
    pub armed_track_ids: Vec<String>,
    pub instrument_faults: Vec<AudioInstrumentFault>,
    pub clock_generation: u64,
    pub discontinuity: u64,
}

/// Post-fader stereo meter of one Track.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackAudioMeter {
    pub track_id: String,
    pub peak_left: f64,
    pub peak_right: f64,
    pub rms_left: f64,
    pub rms_right: f64,
}

/// Meter frame of the Project that owns the active graph.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeterFrame {
    pub project_id: String,
    pub input_peak: f64,
    pub output_peak: f64,
    pub output_peak_left: f64,
    pub output_peak_right: f64,
    pub pre_limiter_peak: f64,
    pub limiter_gain_reduction_db: f64,
    pub hard_clip_samples: u64,
    pub invalid_samples: u64,
    pub feedback_suspected: bool,
    pub track_meters: Vec<TrackAudioMeter>,
}

/// Plugin state captured from an open native plugin editor.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackPluginStateChanged {
    pub project_id: String,
    pub track_id: String,
    pub device_id: String,
    pub parameter_values: Vec<f32>,
    pub state_data: Option<String>,
    pub bypassed: bool,
}

/// One parameter changed in an open native plugin editor.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackPluginParameterChanged {
    pub project_id: String,
    pub track_id: String,
    pub device_id: String,
    pub parameter_index: u32,
    pub value: f32,
}

/// Result of the Host runtime startup handshake.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStartupFinished {
    pub succeeded: bool,
}

/// Canonical recording finalization after native offline processing.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingFinalized {
    pub directory: String,
    pub succeeded: bool,
    pub message: Option<String>,
}

/// A replaced native runtime generation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRestarted {
    pub generation: u64,
}
