use serde::Serialize;

// Shared production types live in feature modules; nothing is re-exported here
// because this module no longer aggregates the removed mirror types.

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub updated_at_ms: u64,
    pub session_id: String,
    pub project_name: Option<String>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Debug, Default, Serialize)]
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginParameter {
    pub index: u32,
    pub name: String,
    pub value: f32,
    pub default_value: f32,
    pub automatable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub loaded: bool,
    pub bypassed: bool,
    pub path: Option<String>,
    pub name: Option<String>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
    pub bypassed_blocks: u64,
    pub parameters: Vec<PluginParameter>,
    pub state_data: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub state: AudioState,
    pub driver: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
    pub round_trip_ms: Option<f64>,
    pub recording: RecordingStatus,
    pub plugin: Option<PluginStatus>,
    pub midi_inputs: Vec<String>,
    pub midi_outputs: Vec<String>,
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

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiProbe {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDriverInfo {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceProbe {
    pub drivers: Vec<AudioDriverInfo>,
    pub midi_inputs: Vec<String>,
    pub midi_outputs: Vec<String>,
    pub refreshed_at_ms: u64,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioState {
    Offline,
    Starting,
    Ready,
    Muted,
    Faulted,
}
