use serde::Serialize;
use ts_rs::TS;

pub use riffra_runtime::RuntimeProjectionStatus;
pub use riffra_runtime::{
    ArrangementMutationResult, AudioDeviceProbe, AudioStatus, DeviceChannels, ProjectState,
    RecordingStopResult, SessionAudioPair,
};
#[cfg(test)]
pub use riffra_runtime::{
    AudioAccessMode, AudioChannelInfo, AudioDevicePairing, AudioDriverInfo, AudioState,
    MidiDeviceInfo, RecordingFinalizationOutcome, RecordingStatus,
};

// Shared production types live in feature modules; this module owns the
// application-level audio and runtime status types.

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
    pub canonical: riffra_core::CanonicalState,
    pub project_state: ProjectState,
    pub plugin_catalog: Vec<crate::plugins::PluginEntry>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
    pub native_available: bool,
    pub recovery_candidates: Vec<RecoveryCandidate>,
    pub data_root: String,
    pub vst3_root: String,
    pub host_connection: crate::host_connection::HostConnectionState,
}
