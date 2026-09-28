//! Shared live Host runtime used by the Desktop shell and the headless CLI.
//!
//! This crate deliberately contains no Tauri, WebView, or command-line
//! parser dependency. A composition root supplies executable paths and an
//! event sink, then embeds the same [`DawHost`] in either shell.

pub mod analysis;
pub mod asset;
mod audio;
mod binaries;
mod control;
mod dispatcher;
mod execution;
mod host;
pub mod instrument;
pub mod jobs;
pub mod library;
pub mod missing;
mod model;
pub mod plugins;
mod preferences;
pub mod projects;
pub mod recording;
pub mod render;
mod runtime;
pub mod session;
mod startup;
#[cfg(test)]
pub(crate) mod test_support;

pub use audio::{
    AudioDeviceReopenOutcome, AudioSupervisor, NativeAudioError, NativeAudioResult,
    RuntimeRestartHandler,
};
pub use binaries::RuntimeBinaries;
pub use dispatcher::{DispatchError, DispatchResult, Dispatcher, command_requires_project_id};
pub use execution::ProjectionDiagnostics;
pub use host::{
    DawHost, HostBootstrap, HostConfig, HostError, HostEvent, HostEventHub, HostEventSink,
    HostEventSubscription, NoopHostEventSink, RecordingHostEventSink, SharedHostEventSink,
};
pub use instrument::{
    BuiltInInstrumentCatalog, BuiltInInstrumentDefinition, BuiltInInstrumentSummary,
    InstrumentPreviewDefinition, InstrumentPreviewNote, InstrumentPreviewTimeSignature,
    InstrumentRecommendedRange,
};
pub use library::instruments::{InstrumentCollection, InstrumentLibraryItem, InstrumentOrigin};
pub use model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioAccessMode, AudioChannelInfo,
    AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe, AudioDiagnostics,
    AudioDiagnosticsDevice, AudioDiagnosticsMute, AudioDiagnosticsOutput, AudioDiagnosticsRealtime,
    AudioDiagnosticsReport, AudioDriverInfo, AudioInstrumentFault, AudioMeterFrame, AudioState,
    AudioStatus, DeviceCapabilities, DeviceChannels, DeviceInspection, DeviceParameterInfo,
    MidiDeviceInfo, PluginPresetInfo, PluginStateSnapshot, ProjectActivationResult,
    ProjectRecoveryState, ProjectState, ProjectSummary, RecordingFinalizationOutcome,
    RecordingFinalized, RecordingPhase, RecordingStatus, RecordingStopResult, RecoveryCandidate,
    RuntimeProjectionState, RuntimeProjectionStatus, RuntimeRestarted, RuntimeStartupFinished,
    TrackAudioMeter, TrackDeviceSummary, TrackInstrumentSummary, TrackInstrumentSummarySource,
    TrackPluginParameterChanged, TrackPluginStateChanged, TrackRackSummary, TrackSummary,
    TransportState, TransportStatus,
};
pub use preferences::{
    AudioDriverConfig, AudioPreferences, AudioPreferencesStore, access_mode_for_driver,
    active_device_matches_preferences, load_or_default,
};
pub use runtime::RuntimeError;
pub(crate) use runtime::{RuntimeDriver, RuntimeReconciler};
