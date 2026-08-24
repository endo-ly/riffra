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
pub mod plugin_catalog;
pub mod plugin_validation;
pub mod plugins;
mod preferences;
pub mod projects;
pub mod recording;
pub mod render;
mod runtime;
pub mod runtime_snapshot;
pub mod session;
mod startup;

use serde_json::Value;

pub use audio::{
    AudioDeviceReopenOutcome, AudioSupervisor, MuteCause, NativeAudioError, NativeAudioResult,
    RuntimeRestartHandler,
};
pub use binaries::RuntimeBinaries;
pub use dispatcher::{DispatchError, DispatchResult, Dispatcher};
pub use host::{DawHost, HostConfig, HostError};
pub use model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioAccessMode, AudioChannelInfo,
    AudioDeviceInfo, AudioDevicePairing, AudioDeviceProbe, AudioDriverInfo, AudioState,
    AudioStatus, DeviceChannels, MidiDeviceInfo, RecordingFinalizationOutcome, RecordingStatus,
    RecordingStopResult, RuntimeProjectionState, RuntimeProjectionStatus, SessionAudioPair,
};
pub use preferences::{
    AudioDriverConfig, AudioPreferences, AudioPreferencesStore, access_mode_for_driver,
    active_device_matches_preferences, load_or_default,
};
pub use runtime::{
    ProjectionDriver, ProjectionStatusHook, RuntimeDriver, RuntimeError, RuntimeReconciler,
    RuntimeRecovery, TIMELINE_PREPARE_TIMEOUT, TransportDriver,
};

use riffra_core::CanonicalState;

/// Typed notifications emitted by a live Host.
#[derive(Clone, Debug)]
pub enum HostEvent {
    /// The canonical state changed after a successful Core commit.
    CanonicalStateChanged(CanonicalState),
    /// Startup completed, with the result of the runtime handshake.
    RuntimeStartupFinished { succeeded: bool },
    /// The latest canonical arrangement projection state.
    RuntimeProjectionStatus(RuntimeProjectionStatus),
    /// Current audio device and safety state.
    AudioStatus(AudioStatus),
    /// Raw native meters retained until a stable DTO is justified.
    AudioMeters(Value),
    /// Raw transport status from the native engine.
    TransportStatus(Value),
    /// A native runtime generation was replaced.
    RuntimeRestarted { generation: u64 },
    /// Raw plugin state event from the native engine.
    TrackPluginStateChanged(Value),
    /// Raw plugin parameter event from the native engine.
    TrackPluginParameterChanged(Value),
}

/// Boundary for turning Host events into a shell-specific event system.
pub trait HostEventSink: Send + Sync + 'static {
    /// Delivers one event. Implementations must not panic when a shell is
    /// already shutting down.
    fn emit(&self, event: HostEvent);
}

/// Event sink for foreground hosts that do not have an event UI.
#[derive(Debug, Default)]
pub struct NoopHostEventSink;

impl HostEventSink for NoopHostEventSink {
    fn emit(&self, _event: HostEvent) {}
}

/// Convenient shared ownership for event sinks supplied to a Host.
pub type SharedHostEventSink = std::sync::Arc<dyn HostEventSink>;

/// A small event sink useful to tests and embedding applications.
#[derive(Debug, Default)]
pub struct RecordingHostEventSink {
    events: std::sync::Mutex<Vec<HostEvent>>,
}

impl RecordingHostEventSink {
    /// Returns a snapshot of events observed so far.
    pub fn events(&self) -> Vec<HostEvent> {
        self.events
            .lock()
            .expect("host event sink lock poisoned")
            .clone()
    }
}

impl HostEventSink for RecordingHostEventSink {
    fn emit(&self, event: HostEvent) {
        self.events
            .lock()
            .expect("host event sink lock poisoned")
            .push(event);
    }
}
