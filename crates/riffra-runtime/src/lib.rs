//! Shared live Host runtime used by the Desktop shell and the headless CLI.
//!
//! This crate deliberately contains no Tauri, WebView, or command-line
//! parser dependency. A composition root supplies executable paths and an
//! event sink, then embeds the same [`DawHost`] in either shell.

mod analysis;
pub mod api;
mod asset;
mod audio;
mod binaries;
mod control;
mod dispatcher;
mod execution;
mod host;
mod instrument;
mod jobs;
mod library;
mod missing;
mod model;
mod plugins;
mod preferences;
mod process;
mod projects;
mod recording;
mod render;
mod runtime;
mod score;
mod score_yaml_import;
mod session;
mod startup;
#[cfg(test)]
pub(crate) mod test_support;

pub use api::output::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioAccessMode, AudioChannelInfo,
    AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe, AudioDiagnostics,
    AudioDiagnosticsDevice, AudioDiagnosticsMute, AudioDiagnosticsOutput, AudioDiagnosticsRealtime,
    AudioDiagnosticsReport, AudioDriverInfo, AudioInstrumentFault, AudioState, AudioStatus,
    DeviceCapabilities, DeviceChannels, DeviceInspection, DeviceParameterChoice,
    DeviceParameterInfo, DeviceSidechainCapability, HostBootstrap, InstrumentCollection,
    InstrumentLibraryItem, InstrumentOrigin, InstrumentPreviewDefinition, InstrumentPreviewNote,
    InstrumentPreviewTimeSignature, InstrumentRecommendedRange, MidiDeviceInfo, PluginPresetInfo,
    PluginStateSnapshot, ProjectActivationResult, ProjectRecoveryState, ProjectState,
    ProjectSummary, ProjectionDiagnostics, RecordingFinalizationOutcome, RecordingStatus,
    RecordingStopResult, RecoveryCandidate, RuntimeProjectionState, RuntimeProjectionStatus,
    TrackEffectSummary, TrackInstrumentSummary, TrackInstrumentSummarySource, TrackSummary,
};
pub use api::params::AudioDriverConfig;
pub use audio::{
    AudioDeviceReopenOutcome, AudioSupervisor, NativeAudioError, NativeAudioResult,
    RuntimeRestartHandler,
};
pub use binaries::RuntimeBinaries;
pub use dispatcher::{DispatchError, DispatchResult, Dispatcher};
pub use host::{
    DawHost, HostConfig, HostError, HostEvent, HostEventHub, HostEventSink, HostEventSubscription,
    NoopHostEventSink, RecordingHostEventSink, SharedHostEventSink,
};
pub use model::{
    AudioMeterFrame, PluginParameterChanged, PluginStateChanged, RecordingFinalized,
    RecordingPhase, RuntimeRestarted, RuntimeStartupFinished, TrackAudioMeter, TransportState,
    TransportStatus,
};
pub use preferences::{
    AudioPreferences, AudioPreferencesStore, access_mode_for_driver,
    active_device_matches_preferences, load_or_default,
};
pub use runtime::RuntimeError;
pub(crate) use runtime::{RuntimeDriver, RuntimeReconciler};
