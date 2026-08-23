use serde::Serialize;
use ts_rs::TS;

pub use riffra_runtime::RuntimeProjectionStatus;
pub use riffra_runtime::{
    AudioAccessMode, AudioChannelInfo, AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe,
    AudioDriverInfo, AudioState, AudioStatus, DeviceChannels, MidiDeviceInfo, RecordingStatus,
};

// Shared production types live in feature modules; this module owns the
// application-level audio and runtime status types.

/// A canonical state and audio status returned by operations that update both
/// the Audio Runtime and the persisted `CreativeSession`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAudioPair {
    pub canonical: riffra_core::CanonicalState,
    pub audio: AudioStatus,
}

/// Result of stopping a recording, including the canonical state visible after
/// stop, audio status, and separate finalization/projection outcomes. A
/// recovery result keeps stopped files visible even when no canonical commit
/// occurred.
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

/// Result of a canonical Arrangement mutation and its best-effort Audio
/// Runtime projection. The canonical state is committed before projection, so
/// a failed projection is reported alongside the committed canonical state.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementMutationResult {
    pub canonical: riffra_core::CanonicalState,
    pub projection: ArrangementProjectionOutcome,
}

/// Outcome of projecting a committed Arrangement mutation into the Audio
/// Runtime. This is deliberately discriminated so callers never infer state
/// from a human-readable error message.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ArrangementProjectionOutcome {
    NotRequired,
    Queued,
    Failed { message: String },
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
    pub canonical: riffra_core::CanonicalState,
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
