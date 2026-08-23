use crate::audio::AudioSupervisor;
use crate::binaries::RuntimeBinaries;
use crate::control::ControlServer;
use crate::model::RuntimeProjectionStatus;
use crate::runtime::{RuntimeError, RuntimeReconciler};
use crate::{HostEvent, SharedHostEventSink};
use riffra_control::{CommandResult, ControlRequest, ControlResponse, ErrorCode, ProtocolError};
use riffra_core::{AppCore, CanonicalState, CreativeSession, TrackKind, TrackPatch};
use riffra_host::{DataRootLease, SessionStore};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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
    #[error("data root could not be opened: {0}")]
    DataRoot(String),
    #[error("session could not be loaded: {0}")]
    Session(String),
    #[error("control server could not start: {0}")]
    Control(String),
    #[error("host state could not be read: {0}")]
    State(String),
}

pub(crate) struct HostState {
    _lease: DataRootLease,
    pub(crate) data_root: PathBuf,
    core: AppCore<AudioSupervisor>,
    storage: SessionStore,
    runtime: Arc<RuntimeReconciler<AudioSupervisor>>,
    events: SharedHostEventSink,
    _command_gate: Mutex<()>,
}

impl HostState {
    fn canonical(&self) -> Result<CanonicalState, HostError> {
        self.core
            .canonical_state()
            .map_err(|error| HostError::State(error.to_string()))
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

    pub(crate) fn dispatch_request(&self, request: ControlRequest) -> ControlResponse {
        let _command_gate = if is_canonical_command(request.command.as_str()) {
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
        match command {
            "host.status" => Ok((
                "hostStatus",
                serde_json::json!({
                    "safeMode": self.core.safe_mode(),
                    "dataRoot": self.data_root,
                    "runtimeGeneration": self.core.audio().runtime_generation(),
                }),
                current.sequence,
            )),
            "session.get" => Ok((
                "canonicalState",
                serde_json::to_value(&current).map_err(serialize_error)?,
                current.sequence,
            )),
            "history.get" => Ok((
                "history",
                serde_json::to_value(current.history).map_err(serialize_error)?,
                current.sequence,
            )),
            "track.list" => {
                let tracks = self
                    .core
                    .application(&self.storage)
                    .list_tracks()
                    .map_err(application_error)?;
                Ok((
                    "tracks",
                    serde_json::to_value(tracks).map_err(serialize_error)?,
                    current.sequence,
                ))
            }
            "track.add" => {
                let params: TrackAddParams = decode(params)?;
                let session = self
                    .core
                    .application(&self.storage)
                    .add_track(params.name, parse_track_kind(&params.kind)?)
                    .map_err(application_error)?;
                self.after_canonical_commit(&session)?;
                let canonical = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let sequence = canonical.sequence;
                Ok((
                    "canonicalState",
                    serde_json::to_value(canonical).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "track.update" => {
                let params: TrackUpdateParams = decode(params)?;
                let session = self
                    .core
                    .application(&self.storage)
                    .update_track(&params.track_id, params.patch)
                    .map_err(application_error)?;
                self.after_canonical_commit(&session)?;
                let canonical = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let sequence = canonical.sequence;
                Ok((
                    "canonicalState",
                    serde_json::to_value(canonical).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "track.remove" => {
                let params: TrackIdParams = decode(params)?;
                let session = self
                    .core
                    .application(&self.storage)
                    .remove_track(&params.track_id)
                    .map_err(application_error)?;
                self.after_canonical_commit(&session)?;
                let canonical = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let sequence = canonical.sequence;
                Ok((
                    "canonicalState",
                    serde_json::to_value(canonical).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "undo" => {
                let session = self
                    .core
                    .application(&self.storage)
                    .undo()
                    .map_err(application_error)?;
                self.after_canonical_commit(&session)?;
                let canonical = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let sequence = canonical.sequence;
                Ok((
                    "canonicalState",
                    serde_json::to_value(canonical).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "redo" => {
                let session = self
                    .core
                    .application(&self.storage)
                    .redo()
                    .map_err(application_error)?;
                self.after_canonical_commit(&session)?;
                let canonical = self
                    .canonical()
                    .map_err(|error| command_error(error.to_string()))?;
                let sequence = canonical.sequence;
                Ok((
                    "canonicalState",
                    serde_json::to_value(canonical).map_err(serialize_error)?,
                    sequence,
                ))
            }
            "runtime.projection.get" => Ok((
                "runtimeProjection",
                serde_json::to_value(self.runtime.status()).map_err(serialize_error)?,
                current.sequence,
            )),
            "transport.play" => {
                let params: TransportParams = decode(params)?;
                self.runtime
                    .apply_and_play(
                        params.transport_sequence,
                        crate::session_value(&current.session)
                            .map_err(|error| command_error(error.to_string()))?,
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
                let params: TransportParams = decode(params)?;
                self.runtime
                    .stop(params.transport_sequence)
                    .map_err(runtime_error)?;
                Ok(("ok", Value::Null, current.sequence))
            }
            "transport.seek" => {
                let params: SeekParams = decode(params)?;
                self.core
                    .audio()
                    .send(serde_json::json!({"type": "seekTimeline", "tick": params.tick}))
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
                "audioStatus",
                if self.core.safe_mode() {
                    return Err(ProtocolError::new(
                        ErrorCode::RuntimeUnavailable,
                        "Safe Mode keeps audio device probing offline",
                    ));
                } else {
                    serde_json::to_value(self.core.audio().status().map_err(audio_error)?)
                        .map_err(serialize_error)?
                },
                current.sequence,
            )),
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                format!("unknown command: {command}"),
            )),
        }
    }

    fn after_canonical_commit(&self, session: &CreativeSession) -> Result<(), ProtocolError> {
        let canonical = self
            .canonical()
            .map_err(|error| command_error(error.to_string()))?;
        self.events
            .emit(HostEvent::CanonicalStateChanged(canonical));
        if self.core.safe_mode() {
            return Ok(());
        }
        let key = riffra_core::ProjectionKey {
            sequence: self
                .canonical()
                .map_err(|error| command_error(error.to_string()))?
                .sequence,
            session_revision: session.arrangement.revision,
        };
        if let Ok(snapshot) = crate::session_value(session) {
            let _ = self.runtime.submit_nonblocking(snapshot, key);
        }
        Ok(())
    }
}

/// One live canonical Host and its shared runtime services.
pub struct DawHost {
    state: Arc<HostState>,
    control: Mutex<Option<ControlServer>>,
}

impl DawHost {
    /// Opens a live Host, acquires its Data Root lease, and publishes Host
    /// control after canonical state is ready.
    pub fn open(config: HostConfig, events: SharedHostEventSink) -> Result<Self, HostError> {
        std::fs::create_dir_all(&config.data_root)
            .map_err(|error| HostError::DataRoot(error.to_string()))?;
        let lease = DataRootLease::acquire(&config.data_root)
            .map_err(|error| HostError::DataRoot(error.to_string()))?;
        let storage = SessionStore::new(&config.data_root);
        let loaded = storage
            .load_or_create()
            .map_err(|error| HostError::Session(error.to_string()))?;
        let audio = if config.safe_mode {
            AudioSupervisor::offline(
                "Safe Mode is active; native audio, MIDI, and external plugins remain isolated",
                Arc::clone(&events),
            )
        } else {
            let arguments = vec!["--serve".to_string()];
            AudioSupervisor::start(&config.binaries, &arguments, Arc::clone(&events))
        };
        let audio = Arc::new(audio);
        let runtime_events = Arc::clone(&events);
        let runtime = match RuntimeReconciler::with_status_listener(
            Arc::clone(&audio),
            None,
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
            data_root: config.data_root.clone(),
            core: AppCore::new(
                config.data_root.clone(),
                loaded.session,
                (*audio).clone(),
                loaded.recovered_from_generation,
                config.safe_mode,
            ),
            storage,
            runtime,
            events,
            _command_gate: Mutex::new(()),
        });
        let control = ControlServer::start(Arc::clone(&state)).map_err(HostError::Control)?;
        Ok(Self {
            state,
            control: Mutex::new(Some(control)),
        })
    }

    /// Returns the current canonical state without opening the Data Root.
    pub fn canonical_state(&self) -> Result<CanonicalState, HostError> {
        self.state.canonical()
    }

    /// Returns the current projection status.
    pub fn runtime_status(&self) -> Result<RuntimeProjectionStatus, HostError> {
        Ok(self.state.runtime.status())
    }

    /// Performs the explicit shutdown sequence for the Host.
    pub fn shutdown(&self) {
        if let Ok(mut control) = self.control.lock()
            && let Some(control) = control.take()
        {
            control.shutdown();
        }
        self.state.core.audio().force_shutdown();
    }
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

fn parse_track_kind(value: &str) -> Result<TrackKind, ProtocolError> {
    match value {
        "audio" => Ok(TrackKind::Audio),
        "instrument" => Ok(TrackKind::Instrument),
        _ => Err(ProtocolError::new(
            ErrorCode::InvalidRequest,
            "track kind must be audio or instrument",
        )),
    }
}

fn application_error(error: riffra_core::ApplicationError) -> ProtocolError {
    match error {
        riffra_core::ApplicationError::Conflict {
            expected_sequence,
            current_sequence,
        } => ProtocolError::conflict(expected_sequence, current_sequence),
        error => command_error(error.to_string()),
    }
}

fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

fn runtime_error(error: RuntimeError) -> ProtocolError {
    match error {
        RuntimeError::RuntimeUnavailable(message) => {
            ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
        }
        error => ProtocolError::new(ErrorCode::CommandFailed, error.to_string()),
    }
}

fn audio_error(error: crate::AudioError) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, error.to_string())
}

fn serialize_error(error: serde_json::Error) -> ProtocolError {
    command_error(error.to_string())
}

fn is_canonical_command(command: &str) -> bool {
    matches!(
        command,
        "track.add" | "track.update" | "track.remove" | "undo" | "redo"
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackAddParams {
    name: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackIdParams {
    track_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackUpdateParams {
    track_id: String,
    #[serde(flatten)]
    patch: TrackPatch,
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

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{
        ControlCommand, HelloRequest, HelloResponse, endpoint_path, new_instance_id, read_endpoint,
        transport,
    };

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
}
