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
mod host;
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
pub mod runtime_snapshot;
pub mod session;
mod startup;

pub use audio::{
    AudioDeviceReopenOutcome, AudioSupervisor, MuteCause, NativeAudioError, NativeAudioResult,
    RuntimeRestartHandler,
};
pub use binaries::RuntimeBinaries;
pub use dispatcher::{DispatchError, DispatchResult, Dispatcher, command_requires_project_id};
pub use host::{
    DawHost, HostBootstrap, HostConfig, HostError, HostEvent, HostEventHub, HostEventSink,
    HostEventSubscription, NoopHostEventSink, RecordingHostEventSink, SharedHostEventSink,
};
pub use model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioAccessMode, AudioChannelInfo,
    AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe, AudioDriverInfo, AudioState,
    AudioStatus, DeviceChannels, MidiDeviceInfo, ProjectActivationResult, ProjectRecoveryState,
    ProjectState, ProjectSummary, RecordingFinalizationOutcome, RecordingStatus,
    RecordingStopResult, RecoveryCandidate, RuntimeProjectionState, RuntimeProjectionStatus,
    SessionAudioPair, TrackDeviceSummary, TrackRackSummary, TrackSummary,
};
pub use preferences::{
    AudioDriverConfig, AudioPreferences, AudioPreferencesStore, access_mode_for_driver,
    active_device_matches_preferences, load_or_default,
};
pub use runtime::{
    ProjectionDriver, ProjectionStatusHook, RuntimeDriver, RuntimeError, RuntimeReconciler,
    RuntimeRecovery, TIMELINE_PREPARE_TIMEOUT, TransportDriver,
};
