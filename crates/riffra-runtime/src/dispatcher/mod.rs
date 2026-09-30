use crate::RuntimeBinaries;
use crate::api::output::{ArrangementMutationResult, ArrangementProjectionOutcome, ProjectState};
use crate::api::{
    CanonicalAccess, CanonicalCommand, CommandDecodeError, CommandExecutor, CommandScope,
    ControlCommand, ControlOutput, attach_input_value, json_pointer_segment,
};
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_control::{ControlRequest, ErrorCode, ProtocolError};
use riffra_core::application::ApplicationMutation;
use riffra_core::ports::{PortError, SessionStorage};
use riffra_core::{AppCore, ApplicationError, AssetId, CreativeSession};
use riffra_host::{DataRootLease, ProjectStore, SessionStore};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod canonical;
mod clips;
mod device;
mod instrument;
mod music;
mod project;
mod session;
mod track;

#[derive(Debug)]
pub enum DispatchError {
    InvalidRequest {
        message: String,
        details: Option<Value>,
    },
    CommandFailed(String),
    RuntimeUnavailable(String),
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
    ProjectConflict {
        expected_project_id: String,
        current_project_id: String,
    },
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message: error, .. }
            | Self::CommandFailed(error)
            | Self::RuntimeUnavailable(error) => formatter.write_str(error),
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => write!(
                formatter,
                "canonical state changed: expected sequence {expected_sequence}, current sequence {current_sequence}"
            ),
            Self::ProjectConflict {
                expected_project_id,
                current_project_id,
            } => write!(
                formatter,
                "active project changed: expected {expected_project_id}, current {current_project_id}"
            ),
        }
    }
}

impl DispatchError {
    pub fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::InvalidRequest { message, details } => {
                let error = ProtocolError::new(ErrorCode::InvalidRequest, message);
                details
                    .clone()
                    .map_or(error.clone(), |details| error.with_details(details))
            }
            Self::CommandFailed(message) => ProtocolError::new(ErrorCode::CommandFailed, message),
            Self::RuntimeUnavailable(message) => {
                ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
            }
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => ProtocolError::conflict(*expected_sequence, *current_sequence),
            Self::ProjectConflict {
                expected_project_id,
                current_project_id,
            } => ProtocolError::project_conflict(expected_project_id, current_project_id),
        }
    }

    fn invalid_request(error: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: error.into(),
            details: None,
        }
    }

    fn invalid_request_with_details(error: impl Into<String>, details: Value) -> Self {
        Self::InvalidRequest {
            message: error.into(),
            details: Some(details),
        }
    }

    fn requires_live_host() -> Self {
        Self::RuntimeUnavailable("this command requires --attach to a running Riffra Host".into())
    }

    fn attach_input_value(mut self, params: &Value) -> Self {
        if let Self::InvalidRequest {
            details: Some(Value::Object(details)),
            ..
        } = &mut self
        {
            attach_input_value(details, params);
        }
        self
    }

    fn with_batch_context(self, operation_index: usize, command: &str) -> Self {
        let cause = self.to_string();
        let details = match self {
            Self::InvalidRequest { details, .. } => details,
            _ => None,
        };
        let mut details = match details {
            Some(Value::Object(details)) => details,
            _ => serde_json::Map::new(),
        };
        if let Some(Value::String(path)) = details.get_mut("path")
            && path.starts_with('/')
            && !path.starts_with("/params/")
            && path != "/command"
        {
            *path = format!("/params{path}");
        }
        details.insert("operationIndex".into(), serde_json::json!(operation_index));
        details.insert("command".into(), Value::String(command.to_owned()));
        details.insert("cause".into(), Value::String(cause));
        Self::invalid_request_with_details("batch operation failed", Value::Object(details))
    }
}

impl From<String> for DispatchError {
    fn from(error: String) -> Self {
        Self::CommandFailed(error)
    }
}

impl From<&'static str> for DispatchError {
    fn from(error: &'static str) -> Self {
        Self::CommandFailed(error.into())
    }
}

impl From<CommandDecodeError> for DispatchError {
    fn from(error: CommandDecodeError) -> Self {
        let message = error.to_string();
        Self::invalid_request_with_details(message, error.details())
    }
}

impl From<ApplicationError> for DispatchError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::Conflict {
                expected_sequence,
                current_sequence,
            } => Self::Conflict {
                expected_sequence,
                current_sequence,
            },
            ApplicationError::InvalidInput { location, message } => {
                let mut path = format!("/{}/{}", location.collection, location.index);
                if let Some(field) = location.field {
                    path.push('/');
                    path.push_str(&json_pointer_segment(&field));
                }
                Self::invalid_request_with_details(
                    format!("invalid command parameters: {message}"),
                    serde_json::json!({
                        "path": path,
                        "index": location.index,
                    }),
                )
            }
            error => Self::CommandFailed(error.to_string()),
        }
    }
}

enum CoreRef<'a, A> {
    Owned(AppCore<A>),
    Borrowed(&'a AppCore<A>),
}

enum StorageRef<'a> {
    Owned(Mutex<SessionStore>),
    Borrowed(&'a SessionStore),
    Memory(Arc<MemorySessionStorage>),
}

#[derive(Default)]
struct MemorySessionStorage;

impl SessionStorage for MemorySessionStorage {
    fn save(&self, _session: &CreativeSession) -> Result<(), PortError> {
        Ok(())
    }
}

impl StorageRef<'_> {
    fn store(&self) -> Result<SessionStore, String> {
        match self {
            Self::Owned(storage) => storage
                .lock()
                .map(|storage| storage.clone())
                .map_err(|_| "session storage lock was poisoned".into()),
            Self::Borrowed(storage) => Ok((*storage).clone()),
            Self::Memory(_) => Err("candidate session has no persistent storage".into()),
        }
    }

    fn replace_owned(&self, storage: SessionStore) {
        if let Self::Owned(current) = self {
            *current
                .lock()
                .expect("standalone session storage lock must not be poisoned") = storage;
        }
    }
}

enum ProjectStoreRef<'a> {
    Owned(ProjectStore),
    Borrowed(&'a ProjectStore),
}

impl ProjectStoreRef<'_> {
    fn as_ref(&self) -> &ProjectStore {
        match self {
            Self::Owned(store) => store,
            Self::Borrowed(store) => store,
        }
    }
}

impl<'a> SessionStorage for StorageRef<'a> {
    fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
        match self {
            Self::Owned(storage) => storage
                .lock()
                .map_err(|_| PortError::Storage("session storage lock was poisoned".into()))
                .and_then(|storage| SessionStorage::save(&*storage, session)),
            Self::Borrowed(storage) => SessionStorage::save(*storage, session),
            Self::Memory(storage) => SessionStorage::save(storage.as_ref(), session),
        }
    }
}

impl<'a, A> CoreRef<'a, A> {
    fn snapshot(&self) -> Result<riffra_core::CanonicalSnapshot, ApplicationError> {
        match self {
            Self::Owned(core) => core.snapshot(),
            Self::Borrowed(core) => (*core).snapshot(),
        }
    }

    fn canonical_state(&self) -> Result<riffra_core::CanonicalState, ApplicationError> {
        match self {
            Self::Owned(core) => core.canonical_state(),
            Self::Borrowed(core) => (*core).canonical_state(),
        }
    }

    fn application<'b>(
        &'b self,
        storage: &'b StorageRef<'a>,
    ) -> riffra_core::application::Application<'b, A, StorageRef<'a>> {
        match self {
            Self::Owned(core) => core.application(storage),
            Self::Borrowed(core) => (*core).application(storage),
        }
    }
}

/// Shared canonical command application used by Standalone and live Hosts.
pub struct HostDispatcher<'a, A> {
    _lease: Option<DataRootLease>,
    core: CoreRef<'a, A>,
    storage: StorageRef<'a>,
    project_store: ProjectStoreRef<'a>,
    data_root: PathBuf,
    sonalloy: PathBuf,
    built_in_instruments: Arc<BuiltInInstrumentCatalog>,
    validate_plugin_roles: bool,
}

/// Standalone dispatcher type retained as the CLI's editing entry point.
pub type Dispatcher = HostDispatcher<'static, ()>;

/// The typed result of one dispatched command and the canonical sequence it
/// was answered at.
#[derive(Debug)]
pub struct DispatchResult {
    pub output: ControlOutput,
    pub sequence: u64,
}

impl HostDispatcher<'static, ()> {
    /// Opens the standalone canonical editing dispatcher.
    pub fn open(data_root: PathBuf, built_in_instruments_root: PathBuf) -> Result<Self, String> {
        let lease = DataRootLease::acquire(&data_root)
            .map_err(|error| format!("data root could not be opened: {error}"))?;
        let built_in_instruments = Arc::new(
            BuiltInInstrumentCatalog::load(built_in_instruments_root).map_err(|error| {
                format!("built-in instrument catalog could not be opened: {error}")
            })?,
        );
        let project_store = ProjectStore::new(&data_root);
        let loaded = project_store
            .initialize()
            .map_err(|error| error.to_string())?
            .loaded;
        let storage = project_store
            .active_session_store()
            .map_err(|error| error.to_string())?;
        let core = AppCore::new(
            data_root.clone(),
            loaded.session,
            (),
            loaded.recovered_from_generation,
            false,
        );
        let sonalloy = RuntimeBinaries::beside_current_executable()?.sonalloy;
        Ok(Self {
            _lease: Some(lease),
            core: CoreRef::Owned(core),
            storage: StorageRef::Owned(Mutex::new(storage)),
            project_store: ProjectStoreRef::Owned(project_store),
            data_root,
            sonalloy,
            built_in_instruments,
            validate_plugin_roles: false,
        })
    }

    /// Validates, decodes, and executes one request in Standalone mode.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is malformed, requires a live Host,
    /// fails its preconditions, or fails while executing.
    pub fn dispatch_request(
        &self,
        request: ControlRequest,
    ) -> Result<DispatchResult, DispatchError> {
        request
            .validate()
            .map_err(|error| DispatchError::invalid_request(error.message))?;
        let command = ControlCommand::decode(&request.command, request.params)?;
        self.dispatch_checked(
            command,
            request.expected_sequence,
            request.expected_project_id.as_deref(),
        )
    }

    /// Executes one typed command in Standalone mode.
    ///
    /// # Errors
    ///
    /// Returns an error when the command requires a live Host, the canonical
    /// sequence differs from `expected_sequence`, or the command fails.
    pub fn dispatch(
        &self,
        command: ControlCommand,
        expected_sequence: Option<u64>,
    ) -> Result<DispatchResult, DispatchError> {
        self.dispatch_checked(command, expected_sequence, None)
    }

    fn dispatch_checked(
        &self,
        command: ControlCommand,
        expected_sequence: Option<u64>,
        expected_project_id: Option<&str>,
    ) -> Result<DispatchResult, DispatchError> {
        let policy = command.policy();
        if policy.executor == CommandExecutor::Runtime {
            return Err(DispatchError::requires_live_host());
        }
        if let (CommandScope::Project { .. }, Some(expected_project_id)) =
            (policy.scope, expected_project_id)
        {
            let current_project_id = self.active_project_id()?;
            if expected_project_id != current_project_id {
                return Err(DispatchError::ProjectConflict {
                    expected_project_id: expected_project_id.to_owned(),
                    current_project_id,
                });
            }
        }
        let canonical = self.core.canonical_state()?;
        if let Some(expected_sequence) = expected_sequence
            && expected_sequence != canonical.sequence
        {
            return Err(DispatchError::Conflict {
                expected_sequence,
                current_sequence: canonical.sequence,
            });
        }
        match (command, policy.executor) {
            (ControlCommand::Canonical(command), CommandExecutor::Canonical { access }) => {
                self.execute_canonical(command, access, canonical)
            }
            (ControlCommand::Project(command), _) => self.execute_project(command, canonical),
            _ => Err(DispatchError::requires_live_host()),
        }
    }
}

impl<'a, A> HostDispatcher<'a, A> {
    fn batch_candidate(&self, session: CreativeSession) -> HostDispatcher<'static, ()> {
        HostDispatcher {
            _lease: None,
            core: CoreRef::Owned(AppCore::new(
                self.data_root.clone(),
                session,
                (),
                false,
                false,
            )),
            storage: StorageRef::Memory(Arc::new(MemorySessionStorage)),
            project_store: ProjectStoreRef::Owned(ProjectStore::new(&self.data_root)),
            data_root: self.data_root.clone(),
            sonalloy: self.sonalloy.clone(),
            built_in_instruments: Arc::clone(&self.built_in_instruments),
            validate_plugin_roles: self.validate_plugin_roles,
        }
    }

    fn commit_batch_candidate(
        &self,
        candidate: CreativeSession,
        expected_sequence: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core
            .application(&self.storage)
            .commit_prepared_candidate(candidate, expected_sequence)
    }

    /// Creates a dispatcher view over an already-owned live Host.
    pub(crate) fn borrowed(
        core: &'a AppCore<A>,
        storage: &'a SessionStore,
        project_store: &'a ProjectStore,
        data_root: &'a Path,
        sonalloy: &'a Path,
        built_in_instruments: &'a Arc<BuiltInInstrumentCatalog>,
    ) -> Self {
        Self {
            _lease: None,
            core: CoreRef::Borrowed(core),
            storage: StorageRef::Borrowed(storage),
            project_store: ProjectStoreRef::Borrowed(project_store),
            data_root: data_root.to_path_buf(),
            sonalloy: sonalloy.to_path_buf(),
            built_in_instruments: Arc::clone(built_in_instruments),
            validate_plugin_roles: true,
        }
    }

    /// Executes one canonical command against `canonical`.
    ///
    /// Reads answer at the sequence of `canonical`; mutations answer at the
    /// sequence they committed.
    pub(crate) fn execute_canonical(
        &self,
        command: CanonicalCommand,
        access: CanonicalAccess,
        canonical: riffra_core::CanonicalState,
    ) -> Result<DispatchResult, DispatchError> {
        let mut wire = serde_json::to_value(&command).expect("canonical commands serialize");
        let params = wire["params"].take();
        let canonical_sequence = canonical.sequence;
        let output = self
            .run_canonical(command, canonical)
            .map_err(|error| error.attach_input_value(&params))?;
        let sequence = match access {
            CanonicalAccess::Read => canonical_sequence,
            CanonicalAccess::Mutation { .. } => self.core.snapshot()?.sequence,
        };
        Ok(DispatchResult { output, sequence })
    }

    fn active_project_id(&self) -> Result<String, DispatchError> {
        self.project_store
            .as_ref()
            .active_project_id()
            .map_err(|error| DispatchError::CommandFailed(error.to_string()))
    }

    fn project_state(&self) -> Result<ProjectState, DispatchError> {
        crate::projects::state(self.project_store.as_ref()).map_err(DispatchError::CommandFailed)
    }

    fn set_core_recovery(&self, recovered: bool) {
        match &self.core {
            CoreRef::Owned(core) => core.set_recovered_from_generation(recovered),
            CoreRef::Borrowed(core) => core.set_recovered_from_generation(recovered),
        }
    }

    fn activate_core_session(
        &self,
        session: CreativeSession,
    ) -> Result<riffra_core::CanonicalState, ApplicationError> {
        match &self.core {
            CoreRef::Owned(core) => core.activate_session(session),
            CoreRef::Borrowed(core) => core.activate_session(session),
        }
    }

    /// Reports a committed edit that created no entities.
    fn edited(&self, _session: CreativeSession) -> Result<ControlOutput, DispatchError> {
        self.arrangement_mutation(BTreeMap::new())
    }

    /// Reports a committed edit and the entities it created.
    fn created(&self, mutation: ApplicationMutation) -> Result<ControlOutput, DispatchError> {
        self.arrangement_mutation(mutation.created_entity_ids)
    }

    fn arrangement_mutation(
        &self,
        created_entity_ids: BTreeMap<String, Vec<String>>,
    ) -> Result<ControlOutput, DispatchError> {
        Ok(ControlOutput::ArrangementMutation(
            ArrangementMutationResult {
                canonical: self.core.canonical_state()?,
                projection: ArrangementProjectionOutcome::NotRequired,
                created_entity_ids,
            },
        ))
    }
}

/// Checks the active Project precondition of a Project-bound command.
pub(crate) fn validate_project_precondition(
    scope: CommandScope,
    expected_project_id: Option<&str>,
    current_project_id: &str,
) -> Result<(), ProtocolError> {
    if scope == CommandScope::Host {
        return Ok(());
    }
    let Some(expected_project_id) = expected_project_id else {
        return Err(ProtocolError::new(
            ErrorCode::InvalidRequest,
            "expectedProjectId is required for Project-bound commands",
        ));
    };
    if expected_project_id != current_project_id {
        return Err(ProtocolError::project_conflict(
            expected_project_id,
            current_project_id,
        ));
    }
    Ok(())
}

fn parse_asset_id(value: &str) -> Result<AssetId, DispatchError> {
    AssetId::from_normalized(value)
        .map_err(|error| DispatchError::invalid_request(format!("Asset id is invalid: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{Dispatcher, HostDispatcher};
    use crate::api::{CommandExecutor, ControlCommand};
    use crate::test_support::{command, mutated_session, output_value};
    use riffra_control::{ControlRequest, ErrorCode};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    fn open(label: &str) -> (Dispatcher, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-{label}-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        (dispatcher, root)
    }

    #[test]
    fn unknown_command_request_reports_command_path() {
        // Arrange
        let (dispatcher, root) = open("unknown-command");

        // Act
        let error = dispatcher
            .dispatch_request(ControlRequest::new(
                "unknown-command",
                "missing.command",
                json!({}),
                None,
            ))
            .unwrap_err()
            .protocol_error();

        // Assert
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.details.unwrap()["path"], "/command");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn standalone_rejects_every_runtime_command() {
        // Arrange
        let (dispatcher, root) = open("runtime-only");
        let device = json!({"trackId": "track:1", "deviceId": "device:1"});
        let runtime_commands = [
            ("host.status", json!({})),
            ("host.info", json!({})),
            ("host.bootstrap", json!({})),
            ("host.shutdown", json!({})),
            ("runtime.projection.get", json!({})),
            ("runtime.projection.retry", json!({})),
            ("transport.play", json!({})),
            ("transport.stop", json!({})),
            ("transport.go-to-start", json!({})),
            ("transport.seek", json!({"tick": 0})),
            ("audio.master-gain.preview", json!({"gainDb": 0.0})),
            (
                "track.mix.preview",
                json!({"trackId": "track:1", "gainDb": 0.0}),
            ),
            ("audio.status", json!({})),
            ("audio.diagnostics", json!({})),
            ("audio.probe", json!({})),
            (
                "audio.channels.probe",
                json!({"driver": "d", "inputDevice": "i", "outputDevice": "o"}),
            ),
            ("audio.recover", json!({})),
            ("audio.startup.retry", json!({})),
            ("audio.driver.get", json!({})),
            (
                "audio.driver.set",
                json!({"driver": "d", "inputDevice": null, "inputChannel": 0, "outputDevice": null, "sampleRate": null, "bufferSize": null}),
            ),
            ("audio.emergency-mute", json!({"muted": true})),
            ("audio.feedback-protection.reset", json!({})),
            ("asset.preview", json!({"assetId": "asset:1"})),
            ("asset.preview.stop", json!({})),
            ("instrument.preview", json!({"instrumentId": "builtin:1"})),
            ("instrument.preview.stop", json!({})),
            ("midi.listening.enable", json!({})),
            ("midi.listening.disable", json!({})),
            (
                "midi.send",
                json!({"trackId": "track:1", "bytes": [144, 60, 100]}),
            ),
            ("midi.target.set", json!({"trackId": null})),
            ("midi.panic", json!({"trackId": "track:1"})),
            ("plugin.catalog.list", json!({})),
            ("plugin.scan", json!({})),
            ("plugin.scan.start", json!({})),
            ("plugin.editor.open", device.clone()),
            ("device.inspect", device.clone()),
            ("device.parameter.list", device.clone()),
            (
                "device.parameter.get",
                json!({"trackId": "track:1", "deviceId": "device:1", "parameterIndex": 0}),
            ),
            ("plugin.preset.list", device.clone()),
            ("plugin.preset.get", device.clone()),
            (
                "plugin.preset.set",
                json!({"trackId": "track:1", "deviceId": "device:1", "presetIndex": 0}),
            ),
            ("plugin.state.get", device.clone()),
            (
                "plugin.state.set",
                json!({"trackId": "track:1", "deviceId": "device:1", "state": {"schemaVersion": 1, "pluginPath": "p.vst3", "parameterValues": []}}),
            ),
            (
                "plugin.state.persist",
                json!({"trackId": "track:1", "deviceId": "device:1", "parameterValues": [], "stateData": null, "bypassed": false}),
            ),
            (
                "plugin.parameter.persist",
                json!({"trackId": "track:1", "deviceId": "device:1", "parameterIndex": 0, "value": 0.5}),
            ),
            ("missing.list", json!({})),
            ("record.start", json!({})),
            ("record.stop", json!({})),
            ("record.status", json!({})),
            ("record.list", json!({})),
            ("record.rename", json!({"id": "take", "newName": "renamed"})),
            ("record.archive", json!({"id": "take"})),
            ("record.promote", json!({"id": "take"})),
            (
                "record.tag",
                json!({"id": "take", "tag": null, "note": null}),
            ),
            ("record.delete", json!({"id": "take"})),
            ("record.duplicates", json!({})),
            (
                "take.activate",
                json!({"sessionId": "session", "takeId": "take"}),
            ),
            ("take.place-separate-clip", json!({"takeId": "take"})),
            (
                "audio-clip.take-variant.set",
                json!({"clipId": "clip", "variant": "raw"}),
            ),
            ("take.comparison.start", json!({"takeId": "take"})),
            ("take.comparison.switch", json!({"variant": "raw"})),
            ("take.comparison.stop", json!({})),
            (
                "project.restore-generation",
                json!({"fileName": "generation.json"}),
            ),
            ("render.start", json!({})),
            ("job.get", json!({"id": "job"})),
            ("job.cancel", json!({"id": "job"})),
            ("analysis.start", json!({"path": "take.wav"})),
            ("library.search", json!({"query": "kick"})),
            (
                "library.asset.update",
                json!({"id": "asset", "tag": null, "note": null}),
            ),
            ("library.related", json!({"id": "asset"})),
            ("library.instrument.list", json!({})),
            (
                "library.instrument.favorite.set",
                json!({"instrumentId": "builtin:1", "favorite": true}),
            ),
            (
                "library.instrument.category.set",
                json!({"instrumentId": "builtin:1", "category": null}),
            ),
            (
                "library.instrument.tags.set",
                json!({"instrumentId": "builtin:1", "tags": []}),
            ),
            ("library.instrument.collection.list", json!({})),
            (
                "library.instrument.collection.create",
                json!({"name": "Set"}),
            ),
            (
                "library.instrument.collection.rename",
                json!({"id": 1, "name": "Set"}),
            ),
            ("library.instrument.collection.delete", json!({"id": 1})),
            (
                "library.instrument.collection.membership.set",
                json!({"collectionId": 1, "instrumentId": "builtin:1", "included": true}),
            ),
        ];

        for (name, params) in runtime_commands {
            // Act
            let executor = ControlCommand::decode(name, params.clone())
                .unwrap()
                .policy()
                .executor;
            let error = dispatcher
                .dispatch_request(ControlRequest::new(name, name, params, None))
                .unwrap_err()
                .protocol_error();

            // Assert
            assert_eq!(executor, CommandExecutor::Runtime, "{name}");
            assert_eq!(error.code, ErrorCode::RuntimeUnavailable, "{name}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn master_gain_set_updates_canonical_settings_in_standalone() {
        // Arrange
        let (dispatcher, root) = open("master-gain");

        // Act
        let result = dispatcher
            .dispatch(
                command("audio.master-gain.set", json!({"gainDb": -6.0})),
                None,
            )
            .unwrap();

        // Assert
        assert_eq!(mutated_session(&result).settings.master_db, -6.0);
        assert_eq!(result.sequence, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn track_and_midi_note_edits_share_core_and_persist() {
        let (dispatcher, root) = open("persist");
        let track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let track_id = mutated_session(&track).arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(
                command(
                    "midi-clip.create",
                    json!({"trackId":track_id,"startTick":0,"durationTicks":3840}),
                ),
                None,
            )
            .unwrap();
        drop(dispatcher);

        let reopened = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let session = output_value(
            &reopened
                .dispatch(command("session.get", json!({})), None)
                .unwrap(),
        );
        assert_eq!(
            session["arrangement"]["midiClips"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_command_does_not_change_current_session() {
        let (dispatcher, root) = open("invalid");
        let projects = output_value(
            &dispatcher
                .dispatch(command("project.list", json!({})), None)
                .unwrap(),
        );
        let project_id = projects["activeProjectId"].as_str().unwrap();
        let current = root.join("projects").join(project_id).join("session.json");
        let before = fs::read(&current).unwrap();
        assert!(
            dispatcher
                .dispatch(
                    command("track.remove", json!({"trackId":"track:missing"})),
                    None
                )
                .is_err()
        );
        assert_eq!(fs::read(current).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn borrowed_session_apply_enforces_plugin_roles() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-batch-role-{}", now_ms()));
        let built_in_root = crate::test_support::prepare_built_in_resource_root(&root);
        let built_in_instruments = std::sync::Arc::new(
            crate::instrument::BuiltInInstrumentCatalog::load(built_in_root).unwrap(),
        );
        let project_store = riffra_host::ProjectStore::new(&root);
        let loaded = project_store.initialize().unwrap().loaded;
        let storage = project_store.active_session_store().unwrap();
        let core = riffra_core::AppCore::new(
            root.clone(),
            loaded.session,
            (),
            loaded.recovered_from_generation,
            false,
        );
        let sonalloy = std::path::PathBuf::new();
        let plugin_path = root.join("VST3/Synth.vst3");
        fs::create_dir_all(&plugin_path).unwrap();
        crate::plugins::save(
            &root,
            &crate::api::output::ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                plugins: vec![crate::api::output::PluginEntry {
                    id: "vst3-synth".into(),
                    name: "Synth".into(),
                    vendor: None,
                    version: None,
                    format: crate::api::output::PluginFormat::Vst3,
                    role: Some(crate::api::output::PluginRole::Instrument),
                    path: plugin_path.to_string_lossy().into_owned(),
                    bundle: true,
                    modified_at_ms: None,
                    scan_state: crate::api::output::PluginScanState::Validated,
                }],
                issues: Vec::new(),
            },
        )
        .unwrap();
        let dispatcher = HostDispatcher::borrowed(
            &core,
            &storage,
            &project_store,
            &root,
            &sonalloy,
            &built_in_instruments,
        );
        let execute = |name: &str, params: serde_json::Value| {
            let command = command(name, params);
            let CommandExecutor::Canonical { access } = command.policy().executor else {
                unreachable!("{name} is a canonical command");
            };
            let ControlCommand::Canonical(command) = command else {
                unreachable!("{name} is a canonical command");
            };
            dispatcher.execute_canonical(command, access, core.canonical_state().unwrap())
        };
        let track = execute("track.add", json!({"name":"Lead","kind":"instrument"})).unwrap();
        let track_id = mutated_session(&track).arrangement.tracks[0].id.clone();

        let error = execute(
            "session.apply",
            json!({"operations":[{"command":"effect.add","params":{"trackId":track_id,"pluginPath":plugin_path.display().to_string()}}]}),
        )
        .unwrap_err();
        assert!(
            error.protocol_error().details.unwrap()["cause"]
                .as_str()
                .unwrap()
                .contains("role Instrument")
        );

        let _ = fs::remove_dir_all(root);
    }
}
