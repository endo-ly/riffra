mod control;
mod events;
mod lifecycle;
mod state;

pub use events::{
    HostEvent, HostEventHub, HostEventSink, HostEventSubscription, NoopHostEventSink,
    RecordingHostEventSink, SharedHostEventSink,
};

use crate::asset::application::{AssetPreviewContext, AssetPreviewOptions};
use crate::audio::AudioSupervisor;
use crate::binaries::RuntimeBinaries;
use crate::control::ControlServer;
use crate::dispatcher::{
    AudioInputParams, DeviceBypassParams, DeviceIdParams, DeviceParameterParams,
    EffectRemoveParams, EffectReorderParams, HostDispatcher, MidiInputParams,
    MissingPluginReplaceParams, MissingRelinkParams, PluginPathParams,
};
use crate::jobs::{self, BackgroundJobStatus, JobKind, JobRegistry};
use crate::model::{AudioStatus, RuntimeProjectionStatus};
use crate::recording::{self, RecordingContext};
use crate::render::{self, RenderOptions, RenderResult};
use crate::runtime::{RuntimeError, RuntimeReconciler};
use crate::session::{
    adapter as session_adapter,
    commit::{self, CanonicalMutationEffect},
    context::SessionContext,
};
use crate::startup;
use crate::{
    AudioDeviceReopenOutcome, AudioDriverConfig, AudioPreferences, AudioPreferencesStore,
    RuntimeRecovery, active_device_matches_preferences, load_or_default,
};
use crate::{analysis, library, missing, plugins};
use riffra_control::{
    CommandResult, ControlCommand, ControlRequest, ControlResponse, ErrorCode, HostIdentity,
    ProtocolError, new_instance_id,
};
use riffra_core::{AppCore, CanonicalState};
use riffra_host::{DataRootLease, SessionStore, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;

/// Composition configuration for one live Host.
#[derive(Clone, Debug)]
pub struct HostConfig {
    /// Data Root owned by the Host.
    pub data_root: PathBuf,
    /// Whether external audio, MIDI, and plugin processes remain offline.
    pub safe_mode: bool,
    /// Explicit native executable paths.
    pub binaries: RuntimeBinaries,
}

impl HostConfig {
    /// Creates a normal-mode configuration using executables beside `riffra`.
    pub fn new(data_root: PathBuf) -> Result<Self, String> {
        Ok(Self {
            data_root,
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

/// Host-owned state required to initialize an embedded or attached Desktop.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBootstrap {
    pub canonical: CanonicalState,
    pub plugin_catalog: Vec<plugins::PluginEntry>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub runtime_projection: RuntimeProjectionStatus,
    pub audio_status: AudioStatus,
    pub recovered_from_generation: bool,
    pub safe_mode: bool,
    pub recovery_candidates: Vec<riffra_host::RecoveryCandidate>,
    pub data_root: PathBuf,
}

pub(crate) struct HostState {
    _lease: DataRootLease,
    identity: HostIdentity,
    pub(crate) data_root: PathBuf,
    core: Arc<AppCore<AudioSupervisor>>,
    storage: SessionStore,
    runtime: Arc<RuntimeReconciler<AudioSupervisor>>,
    events: SharedHostEventSink,
    event_hub: Arc<HostEventHub>,
    binaries: RuntimeBinaries,
    render_worker: render::RenderWorker,
    jobs: JobRegistry,
    audio_preferences: Mutex<AudioPreferences>,
    recording_gate: Mutex<()>,
    _command_gate: Mutex<()>,
    startup_gate: Mutex<()>,
    lifecycle_gate: RwLock<()>,
    shutting_down: AtomicBool,
    shutdown_requested: AtomicBool,
}

/// One live canonical Host and its shared runtime services.
pub struct DawHost {
    state: Arc<HostState>,
    identity: HostIdentity,
    control: Mutex<Option<ControlServer>>,
    startup: Mutex<Option<std::thread::JoinHandle<()>>>,
    plugin_persistence: Mutex<Option<PluginStatePersistenceCoordinator>>,
}

struct PluginStatePersistenceCoordinator {
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStateEvent {
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterEvent {
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
}

#[derive(Debug)]
enum PendingPluginChange {
    State(PluginStateEvent),
    Parameter(PluginParameterEvent),
}

struct QueuedPluginChange {
    order: u64,
    change: PendingPluginChange,
}

#[derive(Hash, Eq, PartialEq)]
enum PluginChangeKey {
    State(String, String),
    Parameter(String, String, i32),
}

impl PluginStatePersistenceCoordinator {
    fn start(
        state: std::sync::Weak<HostState>,
        subscription: Option<HostEventSubscription>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = subscription.and_then(|subscription| {
            std::thread::Builder::new()
                .name("riffra-plugin-state-persistence".into())
                .spawn(move || {
                    let mut pending = HashMap::new();
                    let mut next_order = 0;
                    loop {
                        if worker_stop.load(Ordering::Acquire) {
                            while let Ok(frame) = subscription.try_recv() {
                                collect_plugin_change(&mut pending, frame, &mut next_order);
                            }
                            flush_plugin_changes(&state, &mut pending);
                            break;
                        }
                        match subscription.recv_timeout(std::time::Duration::from_millis(24)) {
                            Ok(frame) => {
                                collect_plugin_change(&mut pending, frame, &mut next_order)
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                flush_plugin_changes(&state, &mut pending);
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                flush_plugin_changes(&state, &mut pending);
                                break;
                            }
                        }
                    }
                })
                .ok()
        });
        Self {
            stop,
            worker: Mutex::new(worker),
        }
    }

    fn shutdown(self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

fn collect_plugin_change(
    pending: &mut HashMap<PluginChangeKey, QueuedPluginChange>,
    frame: riffra_control::HostEventFrame,
    next_order: &mut u64,
) {
    if frame.event == "runtime-restarted" {
        pending.clear();
        return;
    }
    let order = *next_order;
    *next_order = (*next_order).saturating_add(1);
    match frame.event.as_str() {
        "track-plugin-state-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginStateEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::State(change.track_id.clone(), change.device_id.clone()),
                    QueuedPluginChange {
                        order,
                        change: PendingPluginChange::State(change),
                    },
                );
            }
        }
        "track-plugin-parameter-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginParameterEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::Parameter(
                        change.track_id.clone(),
                        change.device_id.clone(),
                        change.parameter_index,
                    ),
                    QueuedPluginChange {
                        order,
                        change: PendingPluginChange::Parameter(change),
                    },
                );
            }
        }
        _ => {}
    }
}

fn flush_plugin_changes(
    state: &std::sync::Weak<HostState>,
    pending: &mut HashMap<PluginChangeKey, QueuedPluginChange>,
) {
    let Some(state) = state.upgrade() else {
        pending.clear();
        return;
    };
    let mut changes = pending
        .drain()
        .map(|(_, change)| change)
        .collect::<Vec<_>>();
    changes.sort_by_key(|change| change.order);
    for queued in changes {
        let (command, params) = match queued.change {
            PendingPluginChange::State(change) => (
                "plugin.state.persist",
                serde_json::json!({
                    "trackId": change.track_id,
                    "deviceId": change.device_id,
                    "parameterValues": change.parameter_values,
                    "stateData": change.state_data,
                    "bypassed": change.bypassed,
                }),
            ),
            PendingPluginChange::Parameter(change) => (
                "plugin.parameter.persist",
                serde_json::json!({
                    "trackId": change.track_id,
                    "deviceId": change.device_id,
                    "parameterIndex": change.parameter_index,
                    "value": change.value,
                }),
            ),
        };
        let response = state.dispatch_persistence_request(ControlRequest::new(
            format!("plugin-persistence-{}", new_instance_id()),
            ControlCommand::new(command, params),
            None,
        ));
        if !response.ok {
            tracing::warn!(
                command,
                error = ?response.error,
                "Host plugin state persistence failed"
            );
        }
    }
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

    /// Returns the canonical history state owned by this Host.
    pub fn history_state(&self) -> Result<riffra_core::HistoryState, HostError> {
        self.state
            .core
            .application(&self.state.storage)
            .history_state()
            .map_err(|error| HostError::State(error.to_string()))
    }

    /// Returns the current projection status.
    pub fn runtime_status(&self) -> Result<RuntimeProjectionStatus, HostError> {
        Ok(self.state.runtime.status())
    }

    /// Locks the Host-wide canonical operation gate.
    pub fn lock_command_gate(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.state
            ._command_gate
            .lock()
            .map_err(|_| "Host command gate was poisoned".to_owned())
    }

    /// Runs a shell operation while the Host remains in its active lifecycle.
    ///
    /// The read barrier makes Desktop invocations obey the same shutdown rule
    /// as attached control clients: shutdown waits for an accepted operation,
    /// while new operations are rejected after shutdown has begun.
    pub fn with_lifecycle<T, F>(&self, operation: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        let _lifecycle = self
            .state
            .lifecycle_gate
            .read()
            .map_err(|_| "Host lifecycle gate was poisoned".to_owned())?;
        if self.state.shutting_down.load(Ordering::Acquire) {
            return Err("Riffra Host has shut down".to_owned());
        }
        operation()
    }

    /// Locks the Host-wide recording operation gate.
    pub fn lock_recording_gate(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.state
            .recording_gate
            .lock()
            .map_err(|_| "Host recording operation gate was poisoned".to_owned())
    }

    /// Returns the event sink owned by this Host.
    pub fn event_sink(&self) -> &dyn crate::HostEventSink {
        self.state.events.as_ref()
    }

    /// Returns whether a connected client requested graceful process shutdown.
    pub fn shutdown_requested(&self) -> bool {
        self.state.shutdown_requested.load(Ordering::Acquire)
    }

    /// Reports whether the native audio engine is currently capturing input.
    pub fn recording_active(&self) -> bool {
        self.state
            .core
            .audio()
            .status()
            .map(|status| status.recording.active)
            .unwrap_or(false)
    }

    /// Queries audio devices through the Host-owned native process adapter.
    pub fn probe_devices(&self) -> Result<crate::AudioDeviceProbe, HostError> {
        if self.state.core.safe_mode() {
            return Ok(crate::AudioDeviceProbe {
                drivers: Vec::new(),
                refreshed_at_ms: now_ms(),
                message: "Safe Mode skipped audio device discovery.".into(),
            });
        }
        self.state
            .core
            .audio()
            .probe_devices(std::time::Duration::from_secs(10))
            .map_err(HostError::State)
    }

    /// Queries the selected device channel layout through the shared audio
    /// process adapter.
    pub fn probe_device_channels(
        &self,
        driver: &str,
        input_device: &str,
        output_device: &str,
    ) -> Result<crate::DeviceChannels, HostError> {
        if self.state.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode skipped audio channel discovery.".into(),
            ));
        }
        self.state
            .core
            .audio()
            .probe_device_channels(
                driver,
                input_device,
                output_device,
                std::time::Duration::from_secs(10),
            )
            .map_err(HostError::State)
    }

    /// Renders the canonical session using the Host-owned render worker.
    pub fn render_timeline(&self, options: RenderOptions) -> Result<RenderResult, HostError> {
        let snapshot = self
            .state
            .core
            .snapshot()
            .map_err(|error| HostError::State(error.to_string()))?;
        render::render_timeline_with_options(
            &self.state.render_worker,
            &self.state.data_root,
            &snapshot.session,
            now_ms(),
            options,
        )
        .map_err(HostError::State)
    }

    /// Returns one Host-owned background job status.
    pub fn background_job(&self, id: &str) -> Result<Option<BackgroundJobStatus>, HostError> {
        self.state
            .jobs
            .status(id)
            .map(jobs::to_background_status)
            .transpose()
            .map_err(HostError::State)
    }

    /// Requests cancellation of one Host-owned background job.
    pub fn cancel_background_job(
        &self,
        id: &str,
    ) -> Result<Option<BackgroundJobStatus>, HostError> {
        self.state
            .jobs
            .cancel(id)
            .map(jobs::to_background_status)
            .transpose()
            .map_err(HostError::State)
    }

    /// Runs a synchronous plugin discovery/validation pass in the Host.
    pub fn scan_plugins(&self, path: Option<PathBuf>) -> Result<plugins::ScanReport, HostError> {
        self.state
            .scan_plugins(path.unwrap_or_else(lifecycle::default_plugin_root))
            .map_err(HostError::State)
    }

    /// Starts a cancellable Host-owned plugin scan job.
    pub fn start_plugin_scan(
        &self,
        path: Option<PathBuf>,
    ) -> Result<BackgroundJobStatus, HostError> {
        self.state
            .start_plugin_scan(path.unwrap_or_else(lifecycle::default_plugin_root))
            .map_err(HostError::State)
    }

    /// Applies and persists a Host-wide audio-device selection.
    pub fn set_audio_driver(&self, config: AudioDriverConfig) -> Result<AudioStatus, HostError> {
        self.state
            .set_audio_driver(config)
            .map_err(|error| HostError::State(error.message))
    }

    /// Returns the canonical Core shared with the Host's control server.
    pub fn core(&self) -> &AppCore<AudioSupervisor> {
        &self.state.core
    }

    /// Returns the Runtime reconciler shared with the Host.
    pub fn runtime(&self) -> &RuntimeReconciler<AudioSupervisor> {
        &self.state.runtime
    }

    /// Returns the Data Root owned by the Host.
    pub fn data_root(&self) -> &std::path::Path {
        &self.state.data_root
    }

    /// Reopens the configured audio device and restores the active graph.
    pub fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        self.state.recover_audio_device()
    }

    /// Retries the initial native graph handshake synchronously.
    pub fn retry_runtime_startup(&self) -> Result<AudioStatus, HostError> {
        self.state.retry_runtime_startup()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{
        ControlCommand, HelloRequest, HelloResponse, LocalHostClient, LocalHostRegistry,
        endpoint_path, new_instance_id, read_endpoint, transport,
    };

    #[test]
    fn plugin_parameter_changes_coalesce_per_parameter_index() {
        let mut pending = HashMap::new();
        let mut next_order = 0;
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.25,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 2,
                    "value": 0.75,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.5,
                }),
            ),
            &mut next_order,
        );

        assert_eq!(pending.len(), 2);
        assert!(matches!(
            pending.get(&PluginChangeKey::Parameter(
                "track:1".into(),
                "device:1".into(),
                1,
            )),
            Some(QueuedPluginChange {
                change: PendingPluginChange::Parameter(change),
                ..
            }) if change.value == 0.5
        ));
        assert!(matches!(
            pending.get(&PluginChangeKey::Parameter(
                "track:1".into(),
                "device:1".into(),
                2,
            )),
            Some(QueuedPluginChange {
                change: PendingPluginChange::Parameter(change),
                ..
            }) if change.value == 0.75
        ));
    }

    #[test]
    fn runtime_restart_discards_pending_plugin_changes() {
        let mut pending = HashMap::new();
        let mut next_order = 0;
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.5,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "runtime-restarted",
                serde_json::json!({"generation": 2}),
            ),
            &mut next_order,
        );

        assert!(pending.is_empty());
    }

    #[test]
    fn safe_mode_host_publishes_endpoint_and_handles_attached_mutation() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-host-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let descriptor = read_endpoint(&data_root).unwrap();

        {
            let mut stream = transport::connect(descriptor.endpoint()).unwrap();
            transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
            let hello: HelloResponse = transport::read_frame(&mut stream).unwrap();
            assert_eq!(hello.instance_id, descriptor.instance_id);

            transport::write_frame(
                &mut stream,
                &ControlRequest::new(
                    "session-get",
                    ControlCommand::new("session.get", serde_json::json!({})),
                    Some(0),
                ),
            )
            .unwrap();
            let session_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
            assert!(session_response.ok);
            assert_eq!(session_response.sequence, Some(0));
            assert_eq!(
                session_response
                    .result
                    .as_ref()
                    .map(|result| result.result_type.as_str()),
                Some("session")
            );

            let request = ControlRequest::new(
                "host-test",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
                Some(0),
            );
            transport::write_frame(&mut stream, &request).unwrap();
            let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
            assert!(response.ok);
            assert_eq!(response.sequence, Some(1));
        }

        assert_eq!(
            host.runtime_status().unwrap().state,
            crate::RuntimeProjectionState::Idle
        );
        host.shutdown();
        assert!(!endpoint_path(&data_root).exists());
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn stale_render_and_undo_requests_are_rejected_by_the_canonical_sequence() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-sequence-guard-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        let mutation = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));
        assert!(mutation.ok);
        assert_eq!(mutation.sequence, Some(1));

        let undo = host.dispatch_control(ControlRequest::new(
            "stale-undo",
            ControlCommand::new("undo", serde_json::json!({})),
            Some(0),
        ));
        assert!(!undo.ok);
        assert_eq!(
            undo.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        let render = host.dispatch_control(ControlRequest::new(
            "stale-render",
            ControlCommand::new("render.start", serde_json::json!({})),
            Some(0),
        ));
        assert!(!render.ok);
        assert_eq!(
            render.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn host_info_returns_the_lightweight_selector_payload() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-info-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();

        let response = client
            .request(&ControlRequest::new(
                "info",
                ControlCommand::new("host.info", serde_json::json!({})),
                None,
            ))
            .unwrap();

        assert!(response.ok);
        let info = response.result.unwrap().value;
        assert_eq!(info["instanceId"], host.identity().instance_id);
        assert_eq!(info["pid"], host.identity().pid);
        assert_eq!(info["dataRoot"], data_root.to_string_lossy().into_owned());
        assert!(info["projectName"].is_null());
        assert_eq!(info["safeMode"], true);
        assert_eq!(info["runtimeState"], "offline");

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn a_data_root_owned_by_another_host_is_reported_as_data_root_in_use() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-in-use-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let owner = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

        let second = DawHost::open(config, Arc::new(crate::NoopHostEventSink));

        assert!(matches!(second, Err(HostError::DataRootInUse)));
        owner.shutdown();
        drop(owner);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn shared_client_receives_bootstrap_and_canonical_events() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-client-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let client = LocalHostClient::connect_data_root(&data_root).unwrap();
        let mut events = client.open_event_stream().unwrap();

        let bootstrap = client
            .request(&ControlRequest::new(
                "bootstrap",
                ControlCommand::new("host.bootstrap", serde_json::json!({})),
                Some(0),
            ))
            .unwrap();
        assert!(bootstrap.ok);
        let bootstrap: HostBootstrap =
            serde_json::from_value(bootstrap.result.unwrap().value).unwrap();
        assert_eq!(bootstrap.canonical.sequence, 0);

        let mutation = client
            .request(&ControlRequest::new(
                "track-add",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Synth", "kind": "instrument"}),
                ),
                Some(0),
            ))
            .unwrap();
        assert!(mutation.ok);
        assert_eq!(
            mutation
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let mutation_result: crate::model::ArrangementMutationResult =
            serde_json::from_value(mutation.result.unwrap().value).unwrap();
        assert_eq!(mutation_result.canonical.sequence, 1);
        assert!(matches!(
            mutation_result.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));
        let event = events.recv().unwrap();
        assert_eq!(event.event, "canonical-state-changed");
        assert_eq!(event.payload["sequence"], 1);

        let discovered = LocalHostRegistry::current_user()
            .discover()
            .unwrap()
            .into_iter()
            .find(|entry| entry.registration.instance_id == host.identity().instance_id);
        assert!(discovered.is_some());
        drop(discovered);

        host.shutdown();
        assert!(!endpoint_path(&data_root).exists());
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn an_open_client_cannot_mutate_after_shutdown_and_the_root_reopens() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-shutdown-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();
        let descriptor = read_endpoint(&data_root).unwrap();
        let mut stream = transport::connect(descriptor.endpoint()).unwrap();
        transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
        let _: HelloResponse = transport::read_frame(&mut stream).unwrap();

        transport::write_frame(
            &mut stream,
            &ControlRequest::new(
                "shutdown-request",
                ControlCommand::new("host.shutdown", serde_json::json!({})),
                Some(0),
            ),
        )
        .unwrap();
        let shutdown_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(shutdown_response.ok);
        transport::write_frame(
            &mut stream,
            &ControlRequest::new(
                "after-shutdown",
                ControlCommand::new(
                    "track.add",
                    serde_json::json!({"name": "Rejected", "kind": "audio"}),
                ),
                Some(0),
            ),
        )
        .unwrap();
        let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::HostUnavailable)
        );
        drop(stream);
        drop(host);

        let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        assert_eq!(reopened.canonical_state().unwrap().sequence, 0);
        reopened.shutdown();
        drop(reopened);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn shutdown_waits_for_inflight_host_operations() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-shutdown-gate-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = Arc::new(DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap());
        let inflight = host
            .state
            .lifecycle_gate
            .read()
            .expect("Host lifecycle gate was not poisoned");
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let shutdown_host = Arc::clone(&host);
        let shutdown_thread = std::thread::spawn(move || {
            shutdown_host.shutdown();
            finished_tx.send(()).unwrap();
        });

        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        drop(inflight);
        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .is_ok()
        );
        shutdown_thread.join().unwrap();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn normal_host_returns_arrangement_mutation_before_shutdown() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-startup-shutdown-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
            ),
        };
        let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

        let response = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));

        assert!(response.ok);
        assert_eq!(
            response
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let mutation: crate::model::ArrangementMutationResult =
            serde_json::from_value(response.result.unwrap().value).unwrap();
        assert_eq!(mutation.canonical.sequence, 1);
        assert!(matches!(
            mutation.projection,
            crate::model::ArrangementProjectionOutcome::Queued
                | crate::model::ArrangementProjectionOutcome::Failed { .. }
        ));

        let marker = host.dispatch_control(ControlRequest::new(
            "marker-add",
            ControlCommand::new(
                "marker.add",
                serde_json::json!({"name": "Verse", "tick": 0}),
            ),
            Some(1),
        ));
        assert!(marker.ok);
        assert_eq!(
            marker
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let marker: crate::model::ArrangementMutationResult =
            serde_json::from_value(marker.result.unwrap().value).unwrap();
        assert_eq!(marker.canonical.sequence, 2);
        assert!(matches!(
            marker.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));

        let settings = host.dispatch_control(ControlRequest::new(
            "session-settings-update",
            ControlCommand::new(
                "session.settings.update",
                serde_json::json!({"note": "authoring note"}),
            ),
            Some(2),
        ));
        assert!(settings.ok);
        assert_eq!(
            settings
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("arrangementMutation")
        );
        let settings: crate::model::ArrangementMutationResult =
            serde_json::from_value(settings.result.unwrap().value).unwrap();
        assert_eq!(settings.canonical.sequence, 3);
        assert!(matches!(
            settings.projection,
            crate::model::ArrangementProjectionOutcome::NotRequired
        ));

        host.shutdown();
        drop(host);

        let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        reopened.shutdown();
        drop(reopened);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn normal_host_publishes_canonical_state_before_projection_status() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-event-order-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let events = host
            .state
            .subscribe_events()
            .expect("Host event subscription should be available");

        let response = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));
        assert!(response.ok);

        let mut canonical_index = None;
        let mut projection_index = None;
        for index in 0..16 {
            let event = events
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("Host should publish the mutation events");
            if event.event == "canonical-state-changed"
                && event.payload["sequence"].as_u64() == Some(1)
            {
                canonical_index = Some(index);
            }
            if event.event == "runtime-projection-status"
                && event.payload["targetProjectionSequence"].as_u64() == Some(1)
            {
                projection_index = Some(index);
                break;
            }
        }

        assert!(
            canonical_index.is_some_and(|canonical| {
                projection_index.is_some_and(|projection| canonical < projection)
            }),
            "canonical state must be published before projection status"
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn lifecycle_operations_are_rejected_after_shutdown() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-lifecycle-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        assert_eq!(host.with_lifecycle(|| Ok::<_, String>(7)), Ok(7));
        host.shutdown();
        assert_eq!(
            host.with_lifecycle(|| Ok::<_, String>(7)),
            Err("Riffra Host has shut down".to_owned())
        );

        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
