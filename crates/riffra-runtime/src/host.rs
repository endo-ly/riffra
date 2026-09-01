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
use crate::{HostEvent, HostEventHub, HostEventSubscription, SharedHostEventSink};
use crate::{analysis, library, missing, plugin_catalog, plugin_validation, plugins};
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
    render_worker: riffra_render_worker::RenderWorker,
    jobs: JobRegistry,
    audio_preferences: Mutex<AudioPreferences>,
    recording_gate: Mutex<()>,
    _command_gate: Mutex<()>,
    startup_gate: Mutex<()>,
    lifecycle_gate: RwLock<()>,
    shutting_down: AtomicBool,
    shutdown_requested: AtomicBool,
}

impl HostState {
    fn identity(&self) -> &HostIdentity {
        &self.identity
    }

    pub(crate) fn subscribe_events(&self) -> Option<HostEventSubscription> {
        self.event_hub.subscribe()
    }

    fn canonical(&self) -> Result<CanonicalState, HostError> {
        self.core
            .canonical_state()
            .map_err(|error| HostError::State(error.to_string()))
    }

    fn bootstrap(&self) -> Result<HostBootstrap, HostError> {
        let recovered_from_generation = self.core.recovered_from_generation();
        let recovery_candidates = if recovered_from_generation {
            self.storage
                .recovery_candidates()
                .map_err(|error| HostError::State(error.to_string()))?
        } else {
            Vec::new()
        };
        Ok(HostBootstrap {
            canonical: self.canonical()?,
            plugin_catalog: plugin_catalog::load(&self.data_root)
                .map_err(|error| HostError::State(error.to_string()))?,
            runtime_started: self.core.audio().startup_completed(),
            runtime_startup_finished: self.core.audio().startup_finished(),
            runtime_projection: self.runtime.status(),
            audio_status: self
                .core
                .audio()
                .status()
                .map_err(|error| HostError::State(error.to_string()))?,
            recovered_from_generation,
            safe_mode: self.core.safe_mode(),
            recovery_candidates,
            data_root: self.data_root.clone(),
        })
    }

    fn response(
        &self,
        request_id: String,
        result_type: &'static str,
        value: Value,
        sequence: u64,
    ) -> ControlResponse {
        ControlResponse::success(
            request_id,
            sequence,
            CommandResult {
                result_type: result_type.into(),
                value,
            },
        )
    }

    fn failure(request_id: String, error: ProtocolError) -> ControlResponse {
        ControlResponse::failure(request_id, None, error)
    }

    fn session_context(&self) -> SessionContext<'_> {
        SessionContext {
            core: self.core.as_ref(),
            audio: self.core.audio(),
            runtime: self.runtime.as_ref(),
            data_root: &self.data_root,
            safe_mode: self.core.safe_mode(),
            events: self.events.as_ref(),
        }
    }

    pub(crate) fn dispatch_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_with_shutdown(request, false)
    }

    fn dispatch_persistence_request(&self, request: ControlRequest) -> ControlResponse {
        self.dispatch_request_inner(request, true)
    }

    fn dispatch_request_with_shutdown(
        &self,
        request: ControlRequest,
        allow_shutdown: bool,
    ) -> ControlResponse {
        let _lifecycle = self
            .lifecycle_gate
            .read()
            .expect("Host lifecycle gate was poisoned");
        self.dispatch_request_inner(request, allow_shutdown)
    }

    fn dispatch_request_inner(
        &self,
        request: ControlRequest,
        allow_shutdown: bool,
    ) -> ControlResponse {
        if !allow_shutdown && self.shutting_down.load(Ordering::Acquire) {
            return Self::failure(
                request.request_id,
                ProtocolError::new(ErrorCode::HostUnavailable, "Riffra Host has shut down"),
            );
        }
        let _command_gate = if requires_command_gate(request.command.as_str()) {
            Some(
                self._command_gate
                    .lock()
                    .expect("Host command gate was poisoned"),
            )
        } else {
            None
        };
        let request_id = request.request_id.clone();
        if let Err(error) = request.validate() {
            return Self::failure(request_id, error);
        }
        let current = match self.canonical() {
            Ok(current) => current,
            Err(error) => return Self::failure(request_id, command_error(error.to_string())),
        };
        if let Some(expected_sequence) = request.expected_sequence
            && expected_sequence != current.sequence
        {
            return Self::failure(
                request_id,
                ProtocolError::conflict(expected_sequence, current.sequence),
            );
        }
        match self.dispatch(request.command.as_str(), request.params, current) {
            Ok((result_type, value, sequence)) => {
                self.response(request_id, result_type, value, sequence)
            }
            Err(error) => Self::failure(request_id, error),
        }
    }

    fn dispatch(
        &self,
        command: &str,
        params: Value,
        current: CanonicalState,
    ) -> Result<(&'static str, Value, u64), ProtocolError> {
        if command == "audio.master-gain.set" {
            let params: MasterGainParams = decode(params)?;
            let pair = session_adapter::set_master_gain_db(&self.session_context(), params.gain_db)
                .map_err(|error| error.protocol_error())?;
            return Ok((
                "sessionAudioPair",
                serde_json::to_value(&pair).map_err(serialize_error)?,
                pair.canonical.sequence,
            ));
        }
        if !is_host_runtime_command(command) {
            if let Some(result) =
                self.dispatch_shared_session(command, params.clone(), current.sequence)?
            {
                return Ok(result);
            }
            let current_sequence = current.sequence;
            let result = HostDispatcher::borrowed(&self.core, &self.storage, &self.data_root)
                .dispatch_with_canonical(
                    riffra_control::ControlCommand::new(command, params),
                    current,
                )
                .map_err(|error| error.protocol_error())?;
            if result.sequence > current_sequence {
                let mutation = self.after_canonical_commit(result.projection_effect())?;
                let sequence = mutation.canonical.sequence;
                return Ok((
                    "arrangementMutation",
                    serde_json::to_value(mutation).map_err(serialize_error)?,
                    sequence,
                ));
            }
            return Ok((result.result_type, result.value, result.sequence));
        }

        match command {
            "host.status" => Ok((
                "hostStatus",
                serde_json::json!({
                    "instanceId": self.identity().instance_id.clone(),
                    "pid": self.identity().pid,
                    "safeMode": self.core.safe_mode(),
                    "dataRoot": self.data_root.to_string_lossy(),
                    "runtimeGeneration": self.core.audio().runtime_generation(),
                }),
                current.sequence,
            )),
            "host.info" => Ok((
                "hostInfo",
                serde_json::json!({
                    "instanceId": self.identity().instance_id.clone(),
                    "pid": self.identity().pid,
                    "dataRoot": self.data_root.to_string_lossy(),
                    "projectName": current.session.project_name,
                    "safeMode": self.core.safe_mode(),
                    "runtimeState": serde_json::to_value(
                        self.core.audio().status().map_err(audio_error)?.state,
                    )
                    .map_err(serialize_error)?,
                }),
                current.sequence,
            )),
            "host.bootstrap" => Ok((
                "hostBootstrap",
                serde_json::to_value(
                    self.bootstrap()
                        .map_err(|error| command_error(error.to_string()))?,
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "host.shutdown" => {
                self.shutdown_requested.store(true, Ordering::Release);
                self.shutting_down.store(true, Ordering::Release);
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.master-gain.preview" => {
                let params: MasterGainParams = decode(params)?;
                if !params.gain_db.is_finite() {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidRequest,
                        "master gain must be finite",
                    ));
                }
                self.core
                    .audio()
                    .preview_master_gain_db(params.gain_db)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.emergency-mute" => {
                let params: MuteParams = decode(params)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(
                        self.core
                            .audio()
                            .set_emergency_mute_from_user(params.muted)
                            .map_err(audio_error)?,
                    )
                    .map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "midi.listening.enable" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks MIDI input; offline MIDI remains available",
                    ));
                }
                Ok((
                    "audioStatus",
                    serde_json::to_value(
                        self.core
                            .audio()
                            .enable_midi_listening()
                            .map_err(audio_error)?,
                    )
                    .map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "midi.listening.disable" => Ok((
                "audioStatus",
                serde_json::to_value(
                    self.core
                        .audio()
                        .disable_midi_listening()
                        .map_err(audio_error)?,
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "plugin.editor.open" => {
                let params: PluginEditorParams = decode(params)?;
                session_adapter::open_track_plugin_editor(
                    &self.session_context(),
                    &params.track_id,
                    &params.device_id,
                )
                .map_err(|error| error.protocol_error())?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "take.comparison.start" => {
                let params: TakeIdParams = decode(params)?;
                let status = session_adapter::start_take_comparison(
                    &self.session_context(),
                    &params.take_id,
                )
                .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.switch" => {
                let params: TakeComparisonParams = decode(params)?;
                let status = session_adapter::switch_take_comparison_variant(
                    &self.session_context(),
                    params.variant,
                )
                .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "take.comparison.stop" => {
                let status = session_adapter::stop_take_comparison(&self.session_context())
                    .map_err(|error| error.protocol_error())?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "runtime.projection.get" => Ok((
                "runtimeProjection",
                serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                current.sequence,
            )),
            "runtime.projection.retry" => {
                if self.runtime.reset_for_repair() {
                    Ok((
                        "runtimeProjection",
                        serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                        current.sequence,
                    ))
                } else {
                    Err(ProtocolError::new(
                        ErrorCode::CommandFailed,
                        "runtime projection is not waiting for repair",
                    ))
                }
            }
            "transport.play" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .apply_and_play(
                        params.transport_sequence,
                        crate::runtime_snapshot::runtime_timeline_snapshot(
                            &self.data_root,
                            &current.session,
                        ),
                        riffra_core::ProjectionKey {
                            sequence: current.sequence,
                            session_revision: current.session.arrangement.revision,
                        },
                        std::time::Duration::from_secs(30),
                    )
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.stop" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .stop(params.transport_sequence)
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.go-to-start" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: TransportParams = decode(params)?;
                self.runtime
                    .stop_and_seek_to_start(params.transport_sequence, || {
                        self.core
                            .audio()
                            .seek_timeline(0)
                            .map_err(RuntimeError::from)
                    })
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.seek" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps transport playback offline",
                    ));
                }
                let params: SeekParams = decode(params)?;
                self.core
                    .audio()
                    .seek_timeline(params.tick)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "audio.status" => Ok((
                "audioStatus",
                serde_json::to_value(self.core.audio().status().map_err(audio_error)?)
                    .map_err(serialize_error)?,
                current.sequence,
            )),
            "audio.probe" => Ok((
                "audioProbe",
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps audio device probing offline",
                    ));
                } else {
                    serde_json::to_value(
                        self.core
                            .audio()
                            .probe_devices(std::time::Duration::from_secs(10))
                            .map_err(command_error)?,
                    )
                    .map_err(serialize_error)?
                },
                current.sequence,
            )),
            "audio.channels.probe" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps audio channel probing offline",
                    ));
                }
                let params: AudioChannelsProbeParams = decode(params)?;
                let channels = self
                    .core
                    .audio()
                    .probe_device_channels(
                        &params.driver,
                        &params.input_device,
                        &params.output_device,
                        std::time::Duration::from_secs(10),
                    )
                    .map_err(command_error)?;
                Ok((
                    "deviceChannels",
                    serde_json::to_value(channels).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.recover" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let status = self
                    .recover_audio_device()
                    .map_err(|error| command_error(error.to_string()))?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.startup.retry" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let status = self
                    .retry_runtime_startup()
                    .map_err(|error| command_error(error.to_string()))?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "audio.driver.get" => Ok((
                "audioDriver",
                serde_json::to_value(
                    self.audio_preferences
                        .lock()
                        .map_err(|_| command_error("audio preferences lock was poisoned"))?
                        .clone(),
                )
                .map_err(serialize_error)?,
                current.sequence,
            )),
            "audio.driver.set" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode keeps external audio devices isolated",
                    ));
                }
                let config: AudioDriverConfig = decode(params)?;
                let status = self.set_audio_driver(config)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "asset.preview" => {
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode blocks live sample preview",
                    ));
                }
                let params: AssetPreviewParams = decode(params)?;
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                let status = crate::asset::application::preview_asset(
                    &AssetPreviewContext {
                        audio: self.core.audio(),
                        data_root: &self.data_root,
                        safe_mode: false,
                    },
                    asset_id,
                    AssetPreviewOptions {
                        start_ms: params.start_ms,
                        end_ms: params.end_ms,
                        looped: params.looped,
                        gain: params.gain,
                    },
                )
                .map_err(command_error)?;
                Ok((
                    "audioStatus",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "asset.preview.stop" => Ok((
                "audioStatus",
                serde_json::to_value(self.core.audio().stop_preview().map_err(audio_error)?)
                    .map_err(serialize_error)?,
                current.sequence,
            )),
            "midi.send" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable("Safe Mode keeps MIDI output offline"));
                }
                let params: MidiSendParams = decode(params)?;
                self.core
                    .audio()
                    .send_track_midi(&params.track_id, &params.bytes)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "midi.panic" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable("Safe Mode keeps MIDI output offline"));
                }
                let params: TrackIdParams = decode(params)?;
                self.core
                    .audio()
                    .panic_track_midi(&params.track_id)
                    .map_err(audio_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "plugin.catalog.list" => {
                let catalog = plugin_catalog::load(&self.data_root).map_err(|error| {
                    command_error(format!("plugin catalog could not be loaded: {error}"))
                })?;
                Ok((
                    "plugins",
                    serde_json::to_value(catalog).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.scan" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks VST3 discovery and load validation",
                    ));
                }
                let params: PluginScanParams = decode(params)?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                let report = self
                    .scan_plugins(root)
                    .map_err(|error| command_error(format!("plugin scan failed: {error}")))?;
                Ok((
                    "pluginScan",
                    serde_json::to_value(report).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "plugin.scan.start" => {
                if self.core.safe_mode() {
                    return Err(runtime_unavailable(
                        "Safe Mode blocks VST3 discovery and load validation",
                    ));
                }
                let params: PluginScanParams = decode(params)?;
                let root = params
                    .path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_plugin_root);
                let status = self.start_plugin_scan(root).map_err(|error| {
                    command_error(format!("plugin scan could not start: {error}"))
                })?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "missing.list" => {
                let missing = missing::collect_missing(&self.data_root, &current.session);
                Ok((
                    "missing",
                    serde_json::to_value(missing).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "record.start" | "record.stop" | "record.status" | "record.list" | "record.rename"
            | "record.archive" | "record.promote" | "record.tag" | "record.delete"
            | "record.duplicates" => {
                let _recording = self
                    .recording_gate
                    .lock()
                    .map_err(|_| command_error("recording operation lock was poisoned"))?;
                let context = RecordingContext {
                    core: &self.core,
                    audio: self.core.audio(),
                    runtime: &self.runtime,
                    data_root: &self.data_root,
                    safe_mode: self.core.safe_mode(),
                };
                let mut sequence = current.sequence;
                let value = match command {
                    "record.start" => {
                        if self.core.safe_mode() {
                            return Err(runtime_unavailable(
                                "Safe Mode keeps recording input offline",
                            ));
                        }
                        let params: RecordStartParams = decode(params)?;
                        let status = match params.recording_session_id.as_deref() {
                            Some(id) => recording::record_another_take(&context, id),
                            None => recording::start_recording(&context),
                        }
                        .map_err(command_error)?;
                        serde_json::to_value(status).map_err(serialize_error)?
                    }
                    "record.stop" => {
                        let result = recording::stop_recording(&context).map_err(command_error)?;
                        sequence = result.canonical.sequence;
                        if sequence > current.sequence {
                            self.events
                                .emit(HostEvent::CanonicalStateChanged(result.canonical.clone()));
                        }
                        serde_json::to_value(result).map_err(serialize_error)?
                    }
                    "record.status" => serde_json::to_value(
                        context
                            .audio
                            .refresh_status()
                            .map_err(|error| error.to_string())
                            .map_err(command_error)?,
                    )
                    .map_err(serialize_error)?,
                    "record.list" => {
                        let params: RecordListParams = decode(params)?;
                        serde_json::to_value(
                            recording::list_recordings(&context, params.query.as_deref())
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.rename" => {
                        let params: RecordRenameParams = decode(params)?;
                        serde_json::to_value(
                            recording::rename_recording(&context, &params.id, &params.new_name)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.archive" => {
                        let params: RecordIdParams = decode(params)?;
                        serde_json::to_value(
                            recording::archive_recording(&context, &params.id)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.promote" => {
                        let params: RecordIdParams = decode(params)?;
                        serde_json::to_value(
                            recording::promote_recording(&context, &params.id)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.tag" => {
                        let params: RecordTagParams = decode(params)?;
                        serde_json::to_value(
                            recording::tag_recording(&context, &params.id, params.tag, params.note)
                                .map_err(command_error)?,
                        )
                        .map_err(serialize_error)?
                    }
                    "record.delete" => {
                        let params: RecordIdParams = decode(params)?;
                        recording::delete_recording(&context, &params.id).map_err(command_error)?;
                        Value::Null
                    }
                    "record.duplicates" => serde_json::to_value(
                        recording::detect_duplicate_recordings(&context).map_err(command_error)?,
                    )
                    .map_err(serialize_error)?,
                    _ => unreachable!(),
                };
                Ok(("recording", value, sequence))
            }
            "render.start" => {
                let params: RenderStartParams = decode(params)?;
                let options = params.options.unwrap_or_default();
                let session = current.session.clone();
                let data_root = self.data_root.clone();
                let worker = self.render_worker.clone();
                let jobs = self.jobs.clone();
                let (id, status) = jobs.start(JobKind::Render);
                let Some(cancelled) = jobs.cancellation_flag(&id) else {
                    return Err(command_error("render job could not be registered"));
                };
                let job_id = id.clone();
                let worker_jobs = jobs.clone();
                jobs.spawn_worker(&id, "riffra-render-job", move || {
                    worker_jobs.set_running(&job_id, "Rendering the canonical arrangement.");
                    match render::render_timeline_with_cancellation(
                        &worker,
                        &data_root,
                        &session,
                        riffra_host::now_ms(),
                        options,
                        cancelled.as_ref(),
                    ) {
                        Ok(result) => match serde_json::to_value(result) {
                            Ok(value) => {
                                worker_jobs.complete(&job_id, value, "Offline render completed.")
                            }
                            Err(error) => {
                                jobs::fail(&worker_jobs, &data_root, &job_id, error.to_string())
                            }
                        },
                        Err(error) => jobs::fail(&worker_jobs, &data_root, &job_id, error),
                    }
                })
                .map_err(|error| command_error(format!("render job could not start: {error}")))?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "job.get" | "job.cancel" => {
                let params: JobIdParams = decode(params)?;
                let status = if command == "job.cancel" {
                    self.jobs.cancel(&params.id)
                } else {
                    self.jobs.status(&params.id)
                };
                let status = status
                    .map(jobs::to_background_status)
                    .transpose()
                    .map_err(command_error)?;
                Ok((
                    "job",
                    serde_json::to_value(status).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.search" => {
                let params: LibrarySearchParams = decode(params)?;
                let result =
                    library::search(&self.data_root, &params.query).map_err(command_error)?;
                Ok((
                    "library",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.asset.update" => {
                let params: LibraryUpdateParams = decode(params)?;
                let result =
                    library::update_metadata(&self.data_root, &params.id, params.tag, params.note)
                        .map_err(command_error)?;
                Ok((
                    "libraryAsset",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "library.related" => {
                let params: LibraryIdParams = decode(params)?;
                let result =
                    library::related(&self.data_root, &params.id).map_err(command_error)?;
                Ok((
                    "library",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "analysis.start" => {
                let params: AnalysisParams = decode(params)?;
                let path = if let Some(asset_id) = params.asset_id {
                    let id = riffra_core::AssetId::from_normalized(&asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                    PathBuf::from(
                        crate::asset::resolve_content_location(&self.data_root, &id).ok_or_else(
                            || command_error(format!("asset is not available: {id}")),
                        )?,
                    )
                } else {
                    params.path.map(PathBuf::from).ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::InvalidRequest,
                            "analysis requires assetId or path",
                        )
                    })?
                };
                let result = analysis::analyze(&path).map_err(command_error)?;
                Ok((
                    "analysis",
                    serde_json::to_value(result).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                format!("unknown command: {command}"),
            )),
        }
    }

    fn dispatch_shared_session(
        &self,
        command: &str,
        params: Value,
        current_sequence: u64,
    ) -> Result<Option<(&'static str, Value, u64)>, ProtocolError> {
        let context = self.session_context();
        let result = match command {
            "track.audio-input.set" => {
                let params: AudioInputParams = decode(params)?;
                Some(
                    session_adapter::set_track_audio_input(
                        &context,
                        &params.track_id,
                        Some(params.channel_index),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "track.audio-input.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::set_track_audio_input(&context, &params.track_id, None)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "track.midi-input.set" => {
                let params: MidiInputParams = decode(params)?;
                Some(
                    session_adapter::set_track_midi_input(
                        &context,
                        &params.track_id,
                        riffra_core::MidiInputRoute {
                            device_id: params.device_id,
                            channel: params.channel,
                        },
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "track.midi-input.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::set_track_midi_input(
                        &context,
                        &params.track_id,
                        riffra_core::MidiInputRoute::default(),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "instrument.set" => {
                let params: PluginPathParams = decode(params)?;
                Some(
                    session_adapter::set_track_instrument_with_expected_sequence(
                        &context,
                        &params.track_id,
                        &params.plugin_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "instrument.clear" => {
                let params: SessionTrackIdParams = decode(params)?;
                Some(
                    session_adapter::clear_track_instrument(&context, &params.track_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.add" => {
                let params: PluginPathParams = decode(params)?;
                Some(
                    session_adapter::add_track_effect_with_expected_sequence(
                        &context,
                        &params.track_id,
                        &params.plugin_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.remove" => {
                let params: EffectRemoveParams = decode(params)?;
                Some(
                    session_adapter::remove_track_effect(
                        &context,
                        &params.track_id,
                        &params.device_id,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "effect.reorder" => {
                let params: EffectReorderParams = decode(params)?;
                Some(
                    session_adapter::reorder_track_effects(
                        &context,
                        &params.track_id,
                        &params.device_ids,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "device.bypass" => {
                let params: DeviceBypassParams = decode(params)?;
                Some(
                    session_adapter::set_track_device_bypassed(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.bypassed,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "device.parameter.set" => {
                let params: DeviceParameterParams = decode(params)?;
                Some(
                    session_adapter::set_track_device_parameter(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_index,
                        params.value,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.relink" => {
                let params: MissingRelinkParams = decode(params)?;
                let asset_id =
                    riffra_core::AssetId::from_normalized(&params.asset_id).map_err(|error| {
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string())
                    })?;
                Some(
                    session_adapter::relink_missing_dependency(
                        &context,
                        asset_id,
                        &params.new_path,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.disable-plugin" => {
                let params: DeviceIdParams = decode(params)?;
                Some(
                    session_adapter::disable_missing_plugin(&context, &params.device_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "missing.replace-plugin" => {
                let params: MissingPluginReplaceParams = decode(params)?;
                Some(
                    session_adapter::replace_missing_track_plugin_with_expected_sequence(
                        &context,
                        &params.device_id,
                        &params.new_path,
                        Some(current_sequence),
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "undo" => {
                Some(session_adapter::undo(&context).map_err(|error| error.protocol_error())?)
            }
            "redo" => {
                Some(session_adapter::redo(&context).map_err(|error| error.protocol_error())?)
            }
            "project.restore-generation" => {
                let params: ProjectRestoreParams = decode(params)?;
                Some(
                    session_adapter::restore_generation(&context, &params.file_name)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "project.import-scratch" => {
                let params: ProjectImportParams = decode(params)?;
                Some(
                    session_adapter::import_session(&context, &params.path)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "plugin.state.persist" => {
                let params: PluginStatePersistParams = decode(params)?;
                Some(
                    session_adapter::persist_track_plugin_state(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_values,
                        params.state_data,
                        params.bypassed,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "plugin.parameter.persist" => {
                let params: PluginParameterPersistParams = decode(params)?;
                Some(
                    session_adapter::persist_track_plugin_parameter(
                        &context,
                        &params.track_id,
                        &params.device_id,
                        params.parameter_index,
                        params.value,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "audio-clip.take-variant.set" => {
                let params: TakeVariantParams = decode(params)?;
                Some(
                    session_adapter::set_audio_clip_take_variant(
                        &context,
                        &params.clip_id,
                        params.variant,
                    )
                    .map_err(|error| error.protocol_error())?,
                )
            }
            "take.activate" => {
                let params: TakeActivateParams = decode(params)?;
                Some(
                    session_adapter::activate_take(&context, &params.session_id, &params.take_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            "take.place-separate-clip" => {
                let params: TakeIdParams = decode(params)?;
                Some(
                    session_adapter::place_take_as_separate_clip(&context, &params.take_id)
                        .map_err(|error| error.protocol_error())?,
                )
            }
            _ => None,
        };
        Ok(result.map(|value| {
            let sequence = value.canonical.sequence;
            (
                "arrangementMutation",
                serde_json::to_value(value).expect("runtime mutation results serialize"),
                sequence,
            )
        }))
    }

    fn after_canonical_commit(
        &self,
        effect: CanonicalMutationEffect,
    ) -> Result<crate::model::ArrangementMutationResult, ProtocolError> {
        let canonical = self
            .canonical()
            .map_err(|error| command_error(error.to_string()))?;
        library::index::refresh(&self.data_root, &canonical.session);
        self.events
            .emit(HostEvent::CanonicalStateChanged(canonical.clone()));
        let mutation = commit::finalize_arrangement_mutation(
            canonical,
            self.runtime.as_ref(),
            &self.data_root,
            self.core.safe_mode(),
            effect,
        )
        .map_err(command_error)?;
        Ok(mutation)
    }

    fn scan_plugins(&self, root: PathBuf) -> Result<plugins::ScanReport, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let mut report = plugins::discover(&root);
        plugin_catalog::reuse_cached_scan_results(&self.data_root, &mut report);
        let mut report = plugin_validation::validate_report(report, &self.binaries.plugin_scan)?;
        report.finished_at_ms = now_ms();
        plugin_catalog::save(&self.data_root, &report)
            .map_err(|error| format!("plugin catalog could not be saved: {error}"))?;
        library::sync_plugins(&self.data_root, &report.plugins)?;
        Ok(report)
    }

    fn start_plugin_scan(&self, root: PathBuf) -> Result<BackgroundJobStatus, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let (id, status) = self.jobs.start(JobKind::Scan);
        let registry = self.jobs.clone();
        let data_root = self.data_root.clone();
        let scanner = self.binaries.plugin_scan.clone();
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return Err("plugin scan job could not be registered".into());
        };
        let job_id = id.clone();
        self.jobs
            .spawn_worker(&id, "riffra-plugin-scan-job", move || {
                registry.set_running(
                    &job_id,
                    "Discovering and validating VST3 plugins in the background.",
                );
                let mut report =
                    match plugins::discover_with_cancel(&root, Some(cancelled.as_ref())) {
                        Ok(report) => report,
                        Err(error) => {
                            jobs::fail(&registry, &data_root, &job_id, error);
                            return;
                        }
                    };
                plugin_catalog::reuse_cached_scan_results(&data_root, &mut report);
                let report = match plugin_validation::validate_report_with_cancel(
                    report,
                    &scanner,
                    Some(cancelled.clone()),
                ) {
                    Ok(mut report) => {
                        report.finished_at_ms = now_ms();
                        report
                    }
                    Err(error) => {
                        jobs::fail(&registry, &data_root, &job_id, error);
                        return;
                    }
                };
                if registry.is_cancelled(&job_id) {
                    registry.mark_cancelled(&job_id);
                    return;
                }
                if let Err(error) = plugin_catalog::save(&data_root, &report) {
                    jobs::fail(
                        &registry,
                        &data_root,
                        &job_id,
                        format!("plugin catalog could not be saved: {error}"),
                    );
                    return;
                }
                if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
                    jobs::fail(&registry, &data_root, &job_id, error);
                    return;
                }
                match jobs::serialize_result(&report) {
                    Ok(value) => registry.complete(&job_id, value, "VST3 scan completed."),
                    Err(error) => jobs::fail(&registry, &data_root, &job_id, error),
                }
            })
            .map_err(|error| format!("plugin scan job could not start: {error}"))?;
        jobs::to_background_status(status)
    }

    fn set_audio_driver(&self, config: AudioDriverConfig) -> Result<AudioStatus, ProtocolError> {
        let requested = AudioPreferences {
            driver: config.driver,
            input_device: config.input_device,
            input_channel: config.input_channel,
            output_device: config.output_device,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        }
        .validate_and_normalize()
        .map_err(|error| ProtocolError::new(ErrorCode::InvalidRequest, error))?;
        let previous = self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))?
            .clone();
        let outcome = match self
            .core
            .audio()
            .set_audio_driver(&requested.as_driver_config())
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = error.to_string();
                return Err(command_error(self.rollback_audio_change(&previous, reason)));
            }
        };
        let restarted = matches!(&outcome, AudioDeviceReopenOutcome::SidecarRestarted(_));
        let mut status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status) => status,
            AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        if !active_device_matches_preferences(&status, &requested) {
            let reason = format!(
                "requested audio device was not activated: {}",
                status.message
            );
            return Err(command_error(if restarted {
                self.restore_previous_audio_preferences(&previous)
                    .map(|()| format!("{reason}; the previous audio device and dependent Runtime were restored"))
                    .unwrap_or_else(|error| format!("{reason}; the previous audio device and dependent Runtime could not be restored: {error}"))
            } else {
                self.rollback_audio_change(&previous, reason)
            }));
        }
        let effective = match AudioPreferences::from_effective_status(&status) {
            Ok(effective) => effective,
            Err(error) => {
                return Err(command_error(self.rollback_audio_change(&previous, error)));
            }
        };
        if let Err(error) = self.core.audio().set_restart_preferences(effective.clone()) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio runtime restart preferences could not be updated: {error}"),
            )));
        }
        if !restarted && let Err(error) = self.reconcile_runtime_after_audio_device_change() {
            return Err(command_error(self.rollback_audio_change(&previous, error)));
        }
        if let Err(error) = AudioPreferencesStore::new(&self.data_root).save(&effective) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio preferences could not be saved: {error}"),
            )));
        }
        *self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))? = effective;
        let access_message = match crate::access_mode_for_driver(
            status.driver.as_deref().unwrap_or(&requested.driver),
        ) {
            crate::AudioAccessMode::Shared => None,
            crate::AudioAccessMode::Exclusive => Some(
                "Exclusive audio is active; other applications using this device will be paused.",
            ),
            crate::AudioAccessMode::DriverManaged => Some(
                "Audio sharing is controlled by this driver; other applications may be paused.",
            ),
        };
        if let Some(access_message) = access_message {
            status.message = if status.message.is_empty() {
                access_message.into()
            } else {
                format!("{access_message} {}", status.message)
            };
        }
        Ok(status)
    }

    fn reconcile_runtime_after_audio_device_change(&self) -> Result<(), String> {
        self.core
            .audio()
            .mark_runtime_recovery_mute()
            .map_err(|error| format!("runtime recovery mute could not be recorded: {error}"))?;
        if !self.runtime.invalidate_for_audio_device_change() {
            return Err(
                "audio runtime graph is busy; the audio device change can be retried shortly"
                    .into(),
            );
        }
        let snapshot = self.canonical().map_err(|error| error.to_string())?;
        self.runtime
            .apply_and_wait(
                crate::runtime_snapshot::runtime_timeline_snapshot(
                    &self.data_root,
                    &snapshot.session,
                ),
                riffra_core::ProjectionKey {
                    sequence: snapshot.sequence,
                    session_revision: snapshot.session.arrangement.revision,
                },
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| {
                format!(
                    "arrangement runtime restoration failed after the audio device change: {error}"
                )
            })?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn confirm_restored_previous_device(&self, previous: &AudioPreferences) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        let status = self
            .core
            .audio()
            .refresh_status()
            .map_err(|error| error.to_string())?;
        if !active_device_matches_preferences(&status, previous) {
            return Err(format!(
                "the previous audio device was not confirmed: {}",
                status.message
            ));
        }
        Ok(())
    }

    fn restore_previous_audio_preferences(
        &self,
        previous: &AudioPreferences,
    ) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        match self
            .core
            .audio()
            .set_audio_driver(&previous.as_driver_config())
        {
            Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)) => {
                if !active_device_matches_preferences(&status, previous) {
                    return Err(format!(
                        "the previous audio device was not confirmed: {}",
                        status.message
                    ));
                }
                self.reconcile_runtime_after_audio_device_change()
            }
            Ok(AudioDeviceReopenOutcome::SidecarRestarted(_)) => {
                self.confirm_restored_previous_device(previous)
            }
            Err(error) => {
                let error = error.to_string();
                self.confirm_restored_previous_device(previous)
                    .map_err(|restore_error| format!("{error}; {restore_error}"))
            }
        }
    }

    fn rollback_audio_change(&self, previous: &AudioPreferences, reason: String) -> String {
        match self.restore_previous_audio_preferences(previous) {
            Ok(()) => {
                format!("{reason}; the previous audio device and dependent Runtime were restored")
            }
            Err(error) => format!(
                "{reason}; the previous audio device and dependent Runtime could not be restored: {error}"
            ),
        }
    }

    fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let outcome = self
            .core
            .audio()
            .recover_audio_device()
            .map_err(|error| HostError::State(error.to_string()))?;
        if matches!(outcome, AudioDeviceReopenOutcome::SidecarRestarted(_)) {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        let snapshot = self.canonical()?;
        self.runtime.invalidate_for_audio_device_change();
        self.runtime
            .apply_and_wait(
                crate::runtime_snapshot::runtime_timeline_snapshot(
                    &self.data_root,
                    &snapshot.session,
                ),
                riffra_core::ProjectionKey {
                    sequence: snapshot.sequence,
                    session_revision: snapshot.session.arrangement.revision,
                },
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core
            .audio()
            .refresh_status()
            .map_err(|error| HostError::State(error.to_string()))
    }

    fn retry_runtime_startup(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let _startup = self
            .startup_gate
            .lock()
            .map_err(|_| HostError::State("Host startup gate was poisoned".into()))?;
        if self.core.audio().startup_completed() {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        self.core.audio().mark_startup_pending();
        let initialized = startup::initialize_runtime(
            &self.core,
            &self.runtime,
            &self.data_root,
            &self.shutting_down,
        );
        let succeeded = initialized
            .as_ref()
            .is_ok_and(|initialization| initialization.runtime_error.is_none());
        self.events
            .emit(HostEvent::RuntimeStartupFinished { succeeded });
        match initialized {
            Ok(initialization) => initialization
                .runtime_error
                .map_or(Ok(initialization.status), |error| {
                    Err(HostError::State(error))
                }),
            Err(error) => Err(HostError::State(error)),
        }
    }
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
    /// Opens a live Host, acquires its Data Root lease, and publishes Host
    /// control after canonical state is ready.
    pub fn open(config: HostConfig, events: SharedHostEventSink) -> Result<Self, HostError> {
        let identity = HostIdentity::new();
        std::fs::create_dir_all(&config.data_root)
            .map_err(|error| HostError::DataRoot(error.to_string()))?;
        let lease = DataRootLease::acquire(&config.data_root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                HostError::DataRootInUse
            } else {
                HostError::DataRoot(error.to_string())
            }
        })?;
        let storage = SessionStore::new(&config.data_root);
        let loaded = storage
            .load_or_create()
            .map_err(|error| HostError::Session(error.to_string()))?;
        let preferences = load_or_default(&config.data_root).map_err(HostError::State)?;
        let event_hub = HostEventHub::new(events);
        let events: SharedHostEventSink = event_hub.clone();
        let audio = if config.safe_mode {
            AudioSupervisor::offline_with_events(
                "Safe Mode is active; native audio, MIDI, and external plugins remain isolated",
                Arc::clone(&events),
            )
        } else {
            AudioSupervisor::start(&config.binaries, preferences.clone(), Arc::clone(&events))
        };
        let audio = Arc::new(audio);
        let runtime_events = Arc::clone(&events);
        let runtime_recovery: Option<RuntimeRecovery> = if config.safe_mode {
            None
        } else {
            let recovery_audio = Arc::clone(&audio);
            Some(Arc::new(move |generation, timeout| {
                recovery_audio
                    .restart_sidecar_for_runtime(generation, timeout)
                    .map_err(RuntimeError::from)
            }))
        };
        let runtime = match RuntimeReconciler::with_status_listener(
            Arc::clone(&audio),
            runtime_recovery,
            Arc::new(move |status| {
                runtime_events.emit(HostEvent::RuntimeProjectionStatus(status));
            }),
        ) {
            Ok(runtime) => Arc::new(runtime),
            Err(error) => {
                audio.force_shutdown();
                return Err(HostError::State(error.to_string()));
            }
        };
        let state = Arc::new(HostState {
            _lease: lease,
            identity: identity.clone(),
            data_root: config.data_root.clone(),
            core: Arc::new(AppCore::new(
                config.data_root.clone(),
                loaded.session,
                (*audio).clone(),
                loaded.recovered_from_generation,
                config.safe_mode,
            )),
            storage,
            runtime,
            events,
            event_hub,
            binaries: config.binaries.clone(),
            render_worker: riffra_render_worker::RenderWorker::new(config.binaries.render.clone()),
            jobs: JobRegistry::default(),
            audio_preferences: Mutex::new(preferences.clone()),
            recording_gate: Mutex::new(()),
            _command_gate: Mutex::new(()),
            startup_gate: Mutex::new(()),
            lifecycle_gate: RwLock::new(()),
            shutting_down: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
        });
        if let Err(error) = audio.set_restart_preferences(preferences) {
            audio.force_shutdown();
            return Err(HostError::State(error.to_string()));
        }
        let runtime_for_restart = Arc::downgrade(&state.runtime);
        if let Err(error) =
            audio.set_runtime_restart_handler(Arc::new(move |runtime_audio, generation| {
                if let Some(runtime) = runtime_for_restart.upgrade()
                    && !runtime.requeue_after_runtime_restart(generation)
                    && let Err(error) = runtime_audio.release_runtime_mute_if_allowed()
                {
                    tracing::warn!(
                        generation,
                        error = %error,
                        "audio runtime restarted without an active graph"
                    );
                }
            }))
        {
            audio.force_shutdown();
            return Err(HostError::State(error.to_string()));
        }
        if let Ok(canonical) = state.canonical() {
            library::index::refresh(&state.data_root, &canonical.session);
        }
        let plugin_persistence = PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
        let control = match ControlServer::start(Arc::clone(&state), identity.clone()) {
            Ok(control) => control,
            Err(error) => {
                plugin_persistence.shutdown();
                audio.force_shutdown();
                return Err(HostError::Control(error));
            }
        };
        let startup = queue_runtime_startup(Arc::clone(&state), config.safe_mode);
        Ok(Self {
            state,
            identity,
            control: Mutex::new(Some(control)),
            startup: Mutex::new(startup),
            plugin_persistence: Mutex::new(Some(plugin_persistence)),
        })
    }

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
            .scan_plugins(path.unwrap_or_else(default_plugin_root))
            .map_err(HostError::State)
    }

    /// Starts a cancellable Host-owned plugin scan job.
    pub fn start_plugin_scan(
        &self,
        path: Option<PathBuf>,
    ) -> Result<BackgroundJobStatus, HostError> {
        self.state
            .start_plugin_scan(path.unwrap_or_else(default_plugin_root))
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

    /// Performs the explicit shutdown sequence for the Host.
    pub fn shutdown(&self) {
        self.state.shutting_down.store(true, Ordering::Release);
        // Wait for ordinary Host commands before closing the event fan-out.
        // Persistence flushes use their dedicated dispatch path and can finish
        // while this write barrier is held.
        let _lifecycle_shutdown = self
            .state
            .lifecycle_gate
            .write()
            .expect("Host lifecycle gate was poisoned");
        self.state.event_hub.close();
        if let Ok(mut persistence) = self.plugin_persistence.lock()
            && let Some(persistence) = persistence.take()
        {
            persistence.shutdown();
        }
        if let Ok(mut control) = self.control.lock()
            && let Some(control) = control.take()
        {
            control.shutdown();
        }
        self.state.jobs.cancel_all_and_wait();
        self.state.core.audio().force_shutdown();
        if let Ok(mut startup) = self.startup.lock()
            && let Some(startup) = startup.take()
        {
            let _ = startup.join();
        }
    }
}

fn queue_runtime_startup(
    state: Arc<HostState>,
    safe_mode: bool,
) -> Option<std::thread::JoinHandle<()>> {
    if safe_mode {
        state
            .events
            .emit(HostEvent::RuntimeStartupFinished { succeeded: false });
        return None;
    }
    let weak_state = Arc::downgrade(&state);
    std::thread::Builder::new()
        .name("riffra-runtime-startup".into())
        .spawn(move || {
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            if state.shutting_down.load(Ordering::Acquire) {
                return;
            }
            let audio = state.core.audio();
            let _startup = state
                .startup_gate
                .lock()
                .expect("Host startup gate was poisoned");
            let initialized = startup::initialize_runtime(
                &state.core,
                &state.runtime,
                &state.data_root,
                &state.shutting_down,
            );
            let succeeded = initialized
                .as_ref()
                .is_ok_and(|initialization| initialization.runtime_error.is_none());
            if let Ok(initialization) = &initialized
                && let Some(error) = initialization.runtime_error.as_deref()
            {
                tracing::warn!(error, "shared runtime startup did not complete");
            }
            if let Err(error) = &initialized {
                tracing::warn!(error, "shared runtime startup did not complete");
            }
            audio.emit_status();
            state
                .events
                .emit(HostEvent::RuntimeStartupFinished { succeeded });
        })
        .map_err(|error| {
            tracing::warn!(error = %error, "shared runtime startup thread could not be created");
        })
        .ok()
}

impl Drop for DawHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ProtocolError> {
    serde_json::from_value(value).map_err(|error| {
        ProtocolError::new(
            ErrorCode::InvalidRequest,
            format!("invalid command parameters: {error}"),
        )
    })
}

fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

fn runtime_unavailable(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
}

fn runtime_error(error: RuntimeError) -> ProtocolError {
    match error {
        RuntimeError::RuntimeUnavailable(message) => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
        }
        RuntimeError::ShuttingDown => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, "runtime is shutting down")
        }
        error => ProtocolError::new(ErrorCode::CommandFailed, error.to_string()),
    }
}

fn audio_error(error: crate::NativeAudioError) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, error.to_string())
}

fn serialize_error(error: serde_json::Error) -> ProtocolError {
    command_error(error.to_string())
}

fn requires_command_gate(command: &str) -> bool {
    !is_host_runtime_command(command)
        && !matches!(
            command,
            // These operations validate and prepare an external VST candidate
            // before attempting the canonical commit. Their expected
            // sequence is checked by the adapter at commit time, so holding
            // the short canonical-operation gate across process work would
            // only block unrelated reads and transport controls.
            "instrument.set" | "effect.add" | "missing.replace-plugin"
        )
}

fn is_host_runtime_command(command: &str) -> bool {
    matches!(
        command,
        "host.status"
            | "host.info"
            | "host.bootstrap"
            | "host.shutdown"
            | "audio.master-gain.preview"
            | "audio.emergency-mute"
            | "midi.listening.enable"
            | "midi.listening.disable"
            | "runtime.projection.get"
            | "runtime.projection.retry"
            | "transport.play"
            | "transport.stop"
            | "transport.go-to-start"
            | "transport.seek"
            | "audio.status"
            | "audio.probe"
            | "audio.channels.probe"
            | "audio.recover"
            | "audio.startup.retry"
            | "audio.driver.set"
            | "audio.driver.get"
            | "asset.preview"
            | "asset.preview.stop"
            | "midi.send"
            | "midi.panic"
            | "plugin.catalog.list"
            | "plugin.scan"
            | "plugin.scan.start"
            | "missing.list"
            | "record.start"
            | "record.stop"
            | "record.status"
            | "record.list"
            | "record.rename"
            | "record.archive"
            | "record.promote"
            | "record.tag"
            | "record.delete"
            | "record.duplicates"
            | "render.start"
            | "job.get"
            | "job.cancel"
            | "library.search"
            | "library.asset.update"
            | "library.related"
            | "analysis.start"
            | "plugin.editor.open"
            | "take.comparison.start"
            | "take.comparison.switch"
            | "take.comparison.stop"
    )
}

fn default_plugin_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Program Files\Common Files\VST3")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/usr/lib/vst3")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackIdParams {
    track_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekParams {
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransportParams {
    transport_sequence: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MasterGainParams {
    gain_db: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MuteParams {
    muted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginEditorParams {
    track_id: String,
    device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStatePersistParams {
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterPersistParams {
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRestoreParams {
    file_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectImportParams {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeIdParams {
    take_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeActivateParams {
    session_id: String,
    take_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeVariantParams {
    clip_id: String,
    variant: riffra_core::AudioTakeVariant,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeComparisonParams {
    variant: riffra_core::AudioTakeVariant,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiSendParams {
    track_id: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginScanParams {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioChannelsProbeParams {
    driver: String,
    input_device: String,
    output_device: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetPreviewParams {
    asset_id: String,
    #[serde(default)]
    start_ms: u64,
    #[serde(default)]
    end_ms: Option<u64>,
    #[serde(default)]
    looped: bool,
    #[serde(default = "default_preview_gain")]
    gain: f32,
}

fn default_preview_gain() -> f32 {
    1.0
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordStartParams {
    recording_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordListParams {
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordRenameParams {
    id: String,
    new_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordTagParams {
    id: String,
    tag: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderStartParams {
    options: Option<RenderOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibrarySearchParams {
    query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryUpdateParams {
    id: String,
    tag: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisParams {
    asset_id: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionTrackIdParams {
    track_id: String,
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
