use serde::{Deserialize, Serialize};
use ts_rs::TS;

// Shared production types live in feature modules; nothing is re-exported here
// because this module no longer aggregates the removed mirror types.

/// A paired session and audio status returned by Application Operations that
/// change the Audio Runtime and the persisted `CreativeSession` in one atomic
/// step. The caller applies both fields directly instead of re-deriving either
/// side, so the runtime and the persisted session never diverge.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAudioPair {
    pub session: crate::session::CreativeSession,
    pub audio: AudioStatus,
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
    pub session: crate::session::CreativeSession,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
    pub native_available: bool,
    pub recovery_candidates: Vec<RecoveryCandidate>,
    pub data_root: String,
    pub vst3_root: String,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub active: bool,
    pub directory: Option<String>,
    pub sample_rate: Option<u32>,
    pub raw_channels: Option<u32>,
    pub processed_channels: Option<u32>,
    pub samples_written: u64,
    pub dropped_blocks: u64,
    pub missing_samples: u64,
    pub dropout_start_sample: Option<u64>,
    pub dropout_end_sample: Option<u64>,
    pub recovery_status: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginParameter {
    pub index: u32,
    pub name: String,
    pub value: f32,
    pub default_value: f32,
    pub automatable: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub loaded: bool,
    pub bypassed: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
    pub input_channels: u32,
    pub output_channels: u32,
    pub bypassed_blocks: u64,
    pub processed_blocks: u64,
    pub contention_blocks: u64,
    pub transition_blocks: u64,
    pub parameters: Vec<PluginParameter>,
    pub state_data: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioChannelInfo {
    pub index: u32,
    pub name: String,
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
    pub plugin: Option<PluginStatus>,
    pub midi_inputs: Vec<MidiDeviceInfo>,
    pub midi_outputs: Vec<MidiDeviceInfo>,
    pub midi_input_active: bool,
    pub midi_messages: u64,
    pub last_midi_note: Option<u8>,
    pub midi_pad_mappings: u32,
    pub midi_pad_triggers: u64,
    pub input_peak: f64,
    pub output_peak: f64,
    pub invalid_samples: u64,
    pub feedback_suspected: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiProbe {
    pub inputs: Vec<MidiDeviceInfo>,
    pub outputs: Vec<MidiDeviceInfo>,
    pub refreshed_at_ms: u64,
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
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
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
    pub midi_inputs: Vec<MidiDeviceInfo>,
    pub midi_outputs: Vec<MidiDeviceInfo>,
    pub refreshed_at_ms: u64,
    pub message: String,
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
