mod audio;
mod control;
mod events;
mod lifecycle;
pub(crate) mod open_project;
mod persistence;
mod plugin_scan;
mod project;
mod state;

pub use events::{
    HostEvent, HostEventHub, HostEventSink, HostEventSubscription, NoopHostEventSink,
    RecordingHostEventSink, SharedHostEventSink,
};
pub use state::HostBootstrap;

pub(crate) use state::HostState;

use crate::api::output::AudioStatus;
use crate::api::output::{BackgroundJobStatus, JobKind};
use crate::api::params::RenderOptions;
use crate::asset::application::{AssetPreviewContext, AssetPreviewOptions};
use crate::audio::AudioSupervisor;
use crate::binaries::RuntimeBinaries;
use crate::control::ControlServer;
use crate::dispatcher::HostDispatcher;
use crate::jobs::{self, JobRegistry};
use crate::recording::{self, RecordingContext};
use crate::render;
use crate::runtime::RuntimeError;
use crate::session::{adapter as session_adapter, commit, context::SessionContext};
use crate::startup;
use crate::{
    AudioDeviceReopenOutcome, AudioDriverConfig, AudioPreferences, AudioPreferencesStore,
    active_device_matches_preferences, load_or_default,
};
use crate::{analysis, library, missing, plugins};
use riffra_control::{ControlRequest, ControlResponse, ErrorCode, HostIdentity, ProtocolError};
use riffra_core::{AppCore, CanonicalState};
use riffra_host::{DataRootLease, ProjectStore, now_ms};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;

/// Composition configuration for one live Host.
#[derive(Clone, Debug)]
pub struct HostConfig {
    /// Data Root owned by the Host.
    pub data_root: PathBuf,
    /// Resource root containing the shipped built-in instrument presets.
    pub built_in_instruments_root: PathBuf,
    /// Whether external audio, MIDI, and plugin processes remain offline.
    pub safe_mode: bool,
    /// Explicit native executable paths.
    pub binaries: RuntimeBinaries,
}

impl HostConfig {
    /// Creates a normal-mode configuration using executables beside `riffra`.
    pub fn new(data_root: PathBuf, built_in_instruments_root: PathBuf) -> Result<Self, String> {
        Ok(Self {
            data_root,
            built_in_instruments_root,
            safe_mode: false,
            binaries: RuntimeBinaries::beside_current_executable()?,
        })
    }
}

/// Errors raised while opening or shutting down a Host.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("data root is already owned by another Riffra Host")]
    DataRootInUse,
    #[error("data root could not be opened: {0}")]
    DataRoot(String),
    #[error("session could not be loaded: {0}")]
    Session(String),
    #[error("control server could not start: {0}")]
    Control(String),
    #[error("host state could not be read: {0}")]
    State(String),
}

/// One live canonical Host and its shared runtime services.
pub struct DawHost {
    state: Arc<HostState>,
    identity: HostIdentity,
    control: Mutex<Option<ControlServer>>,
    startup: Mutex<Option<std::thread::JoinHandle<()>>>,
    plugin_persistence: Mutex<Option<persistence::PluginStatePersistenceCoordinator>>,
}

impl DawHost {
    /// Returns the current canonical state without opening the Data Root.
    pub fn canonical_state(&self) -> Result<CanonicalState, HostError> {
        self.state.canonical()
    }

    /// Returns the Host-owned bootstrap snapshot used by Desktop shells.
    pub fn bootstrap(&self) -> Result<HostBootstrap, HostError> {
        self.state.bootstrap()
    }

    /// Dispatches one shared Control request through the in-process Host.
    pub fn dispatch_control(&self, request: ControlRequest) -> ControlResponse {
        self.state.dispatch_request(request)
    }

    /// Returns the identity allocated for this Host process.
    pub fn identity(&self) -> &HostIdentity {
        &self.identity
    }

    /// Returns whether a connected client requested graceful process shutdown.
    pub fn shutdown_requested(&self) -> bool {
        self.state.shutdown_requested.load(Ordering::Acquire)
    }

    /// Reports whether the native audio engine is currently capturing input.
    pub fn recording_active(&self) -> bool {
        self.state
            .audio
            .status()
            .map(|status| status.recording.active || status.recording.processing)
            .unwrap_or(false)
    }

    /// Returns the Data Root owned by the Host.
    pub fn data_root(&self) -> &std::path::Path {
        &self.state.data_root
    }
}
