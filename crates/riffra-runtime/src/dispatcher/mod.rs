use crate::model::{ArrangementMutationResult, ArrangementProjectionOutcome, TrackSummary};
use crate::session::commit::CanonicalMutationEffect;
use riffra_control::{ControlCommand, ControlRequest, ErrorCode, ProtocolError};
use riffra_core::application::{
    AudioAssetClipPlacement, ChordVoicingInput, HarmonyEventInput, HarmonyEventPatch,
    HarmonyRealizeSelection, MarkerPatch, MidiAssetClipPlacement, MidiNoteInput, MidiNotePatch,
    MidiNoteUpdate, MusicalMidiNoteInput, SessionInspectionQuery, SessionSettingsPatch,
    inspect_canonical_state,
};
use riffra_core::ports::{PortError, SessionStorage};
use riffra_core::{
    AppCore, ApplicationError, AssetId, AssetKind, AudioClipMove, AudioClipPatch,
    AutomationParameter, AutomationPoint, CreativeSession, DeviceKind, FrameRange, MidiClipMove,
    MidiClipPatch, MidiInputRoute, PhrasePattern, PhrasePlacement, ProjectTimebase, RackDevice,
    RhythmPattern, TimelineTick, TrackKind, TrackPatch,
};
use riffra_host::{DataRootLease, SessionStore, now_ms};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};

mod asset;
mod clips;
mod device;
mod music;
mod project;
mod session;
mod track;

pub(crate) use device::{
    DeviceBypassParams, DeviceIdParams, DeviceParameterParams, EffectRemoveParams,
    EffectReorderParams, MissingPluginReplaceParams, MissingRelinkParams, PluginPathParams,
};
pub(crate) use track::{AudioInputParams, MidiInputParams};

#[derive(Debug)]
pub enum DispatchError {
    InvalidRequest(String),
    CommandFailed(String),
    RuntimeUnavailable(String),
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(error)
            | Self::CommandFailed(error)
            | Self::RuntimeUnavailable(error) => formatter.write_str(error),
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => write!(
                formatter,
                "canonical state changed: expected sequence {expected_sequence}, current sequence {current_sequence}"
            ),
        }
    }
}

impl DispatchError {
    pub fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::InvalidRequest(message) => ProtocolError::new(ErrorCode::InvalidRequest, message),
            Self::CommandFailed(message) => ProtocolError::new(ErrorCode::CommandFailed, message),
            Self::RuntimeUnavailable(message) => {
                ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
            }
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => ProtocolError::conflict(*expected_sequence, *current_sequence),
        }
    }

    fn invalid_request(error: impl Into<String>) -> Self {
        Self::InvalidRequest(error.into())
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
            error => Self::CommandFailed(error.to_string()),
        }
    }
}

enum CoreRef<'a, A> {
    Owned(AppCore<A>),
    Borrowed(&'a AppCore<A>),
}

enum StorageRef<'a> {
    Owned(SessionStore),
    Borrowed(&'a SessionStore),
}

impl<'a> SessionStorage for StorageRef<'a> {
    fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
        match self {
            Self::Owned(storage) => SessionStorage::save(storage, session),
            Self::Borrowed(storage) => SessionStorage::save(*storage, session),
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
    data_root: PathBuf,
    allow_runtime_commands: bool,
}

/// Standalone dispatcher type retained as the CLI's editing entry point.
pub type Dispatcher = HostDispatcher<'static, ()>;

#[derive(Debug)]
pub struct DispatchResult {
    pub result_type: &'static str,
    pub value: Value,
    pub sequence: u64,
    projection_effect: CanonicalMutationEffect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackIdParams {
    pub(crate) track_id: String,
}

impl DispatchResult {
    pub(crate) fn projection_effect(&self) -> CanonicalMutationEffect {
        self.projection_effect
    }
}

impl HostDispatcher<'static, ()> {
    /// Opens the standalone canonical editing dispatcher.
    pub fn open(data_root: PathBuf) -> Result<Self, String> {
        let lease = DataRootLease::acquire(&data_root)
            .map_err(|error| format!("data root could not be opened: {error}"))?;
        let storage = SessionStore::new(&data_root);
        let loaded = storage
            .load_or_create()
            .map_err(|error| error.to_string())?;
        let core = AppCore::new(
            data_root.clone(),
            loaded.session,
            (),
            loaded.recovered_from_generation,
            false,
        );
        Ok(Self {
            _lease: Some(lease),
            core: CoreRef::Owned(core),
            storage: StorageRef::Owned(storage),
            data_root,
            allow_runtime_commands: false,
        })
    }
}

impl<'a, A> HostDispatcher<'a, A> {
    /// Creates a dispatcher view over an already-owned live Host.
    pub(crate) fn borrowed(
        core: &'a AppCore<A>,
        storage: &'a SessionStore,
        data_root: &'a Path,
    ) -> Self {
        Self {
            _lease: None,
            core: CoreRef::Borrowed(core),
            storage: StorageRef::Borrowed(storage),
            data_root: data_root.to_path_buf(),
            allow_runtime_commands: true,
        }
    }

    pub fn dispatch_request(
        &self,
        request: ControlRequest,
    ) -> Result<DispatchResult, DispatchError> {
        request
            .validate()
            .map_err(|error| DispatchError::InvalidRequest(error.message))?;
        if !self.allow_runtime_commands && is_runtime_host_only(&request.command) {
            return Err(DispatchError::RuntimeUnavailable(
                "this command requires --attach to a running Riffra Host".into(),
            ));
        }
        let canonical = self.core.canonical_state()?;
        if let Some(expected_sequence) = request.expected_sequence
            && expected_sequence != canonical.sequence
        {
            return Err(DispatchError::Conflict {
                expected_sequence,
                current_sequence: canonical.sequence,
            });
        }
        self.dispatch_with_canonical(request.control_command(), canonical)
    }

    pub fn dispatch(&self, request: ControlCommand) -> Result<DispatchResult, DispatchError> {
        let canonical = self.core.canonical_state()?;
        self.dispatch_with_canonical(request, canonical)
    }

    pub(crate) fn dispatch_with_canonical(
        &self,
        request: ControlCommand,
        canonical: riffra_core::CanonicalState,
    ) -> Result<DispatchResult, DispatchError> {
        if !self.allow_runtime_commands && is_runtime_host_only(&request.name) {
            return Err(DispatchError::RuntimeUnavailable(
                "this command requires --attach to a running Riffra Host".into(),
            ));
        }
        let command = request.name.clone();
        let canonical_sequence = canonical.sequence;
        let result = if session::handles(&command) {
            session::dispatch(self, request, canonical.clone())?
        } else if track::handles(&command) {
            track::dispatch(self, request, canonical.clone())?
        } else if clips::handles(&command) {
            clips::dispatch(self, request, canonical.clone())?
        } else if music::handles(&command) {
            music::dispatch(self, request, canonical.clone())?
        } else if asset::handles(&command) {
            asset::dispatch(self, request, canonical.clone())?
        } else if project::handles(&command) {
            project::dispatch(self, request, canonical.clone())?
        } else if device::handles(&command) {
            device::dispatch(self, request, canonical)?
        } else {
            return Err(DispatchError::invalid_request(format!(
                "unknown command: {command}"
            )));
        };
        let sequence = if is_read_command(&command) {
            canonical_sequence
        } else {
            self.core
                .snapshot()
                .map_err(|error| error.to_string())?
                .sequence
        };
        if is_arrangement_mutation_command(&command) {
            let canonical = self.core.canonical_state()?;
            return Ok(DispatchResult {
                result_type: "arrangementMutation",
                value: serde_json::to_value(ArrangementMutationResult {
                    canonical: canonical.clone(),
                    projection: ArrangementProjectionOutcome::NotRequired,
                })
                .expect("arrangement mutation results serialize"),
                sequence: canonical.sequence,
                projection_effect: CanonicalMutationEffect::CanonicalOnly,
            });
        }
        Ok(DispatchResult {
            result_type: result.result_type,
            value: result.value,
            sequence,
            projection_effect: result.projection_effect,
        })
    }

    fn session(&self, session: CreativeSession) -> DispatchResult {
        self.session_with_effect(session, CanonicalMutationEffect::ProjectArrangement)
    }

    fn session_with_effect(
        &self,
        session: CreativeSession,
        projection_effect: CanonicalMutationEffect,
    ) -> DispatchResult {
        self.value_with_effect("session", session, projection_effect)
    }

    fn value<T: serde::Serialize>(&self, result_type: &'static str, value: T) -> DispatchResult {
        self.value_with_effect(result_type, value, CanonicalMutationEffect::CanonicalOnly)
    }

    fn value_with_effect<T: serde::Serialize>(
        &self,
        result_type: &'static str,
        value: T,
        projection_effect: CanonicalMutationEffect,
    ) -> DispatchResult {
        DispatchResult {
            result_type,
            value: serde_json::to_value(value).expect("canonical values must serialize"),
            sequence: 0,
            projection_effect,
        }
    }
}

fn is_read_command(command: &str) -> bool {
    matches!(
        command,
        "session.get"
            | "session.inspect"
            | "history.get"
            | "track.list"
            | "audio-clip.list"
            | "midi-clip.list"
            | "music.harmony.resolve"
            | "music.harmony.list"
            | "music.region.list"
            | "project.export"
    )
}

fn is_arrangement_mutation_command(command: &str) -> bool {
    matches!(
        command,
        "track.audio-input.set"
            | "track.audio-input.clear"
            | "track.midi-input.set"
            | "track.midi-input.clear"
            | "instrument.set"
            | "instrument.clear"
            | "effect.add"
            | "effect.remove"
            | "effect.reorder"
            | "device.bypass"
            | "device.parameter.set"
            | "missing.relink"
            | "missing.disable-plugin"
            | "missing.replace-plugin"
            | "undo"
            | "redo"
    )
}

fn is_runtime_host_only(command: &str) -> bool {
    matches!(
        command,
        "runtime.projection.get"
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
            | "audio.driver.get"
            | "audio.driver.set"
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
    )
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, DispatchError> {
    serde_json::from_value(value).map_err(|error| {
        DispatchError::invalid_request(format!("invalid command parameters: {error}"))
    })
}

fn parse_asset_id(value: &str) -> Result<AssetId, DispatchError> {
    AssetId::from_normalized(value)
        .map_err(|error| DispatchError::invalid_request(format!("Asset id is invalid: {error}")))
}

fn parse_track_kind(value: &str) -> Result<TrackKind, DispatchError> {
    match value {
        "audio" => Ok(TrackKind::Audio),
        "instrument" => Ok(TrackKind::Instrument),
        _ => Err(DispatchError::invalid_request(
            "track kind must be audio or instrument",
        )),
    }
}

fn parse_automation_parameter(value: &str) -> Result<AutomationParameter, DispatchError> {
    match value {
        "volume" => Ok(AutomationParameter::Volume),
        "pan" => Ok(AutomationParameter::Pan),
        _ => Err(DispatchError::invalid_request(
            "automation parameter must be volume or pan",
        )),
    }
}

fn plugin_device(id: String, path: String) -> Result<RackDevice, DispatchError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(DispatchError::invalid_request(
            "plugin path must not be empty",
        ));
    }
    let name = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned();
    Ok(RackDevice {
        id,
        name,
        kind: DeviceKind::Plugin,
        path: Some(path.to_owned()),
        bypassed: false,
        gain_db: 0.0,
        parameter_values: Vec::new(),
        state_data: None,
        disabled_placeholder: false,
    })
}

#[cfg(test)]
mod tests {
    use super::Dispatcher;
    use riffra_control::ControlCommand;
    use riffra_host::now_ms;
    use serde_json::{Value, json};
    use std::fs;

    fn request(command: &str, params: Value) -> ControlCommand {
        ControlCommand {
            name: command.into(),
            params,
        }
    }

    #[test]
    fn track_and_midi_note_edits_share_core_and_persist() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(request(
                "midi-clip.create",
                json!({"trackId":track_id,"startTick":0,"durationTicks":3840}),
            ))
            .unwrap();
        drop(dispatcher);

        let reopened = Dispatcher::open(root.clone()).unwrap();
        let result = reopened
            .dispatch(request("session.get", json!({})))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(result.value).unwrap();
        assert_eq!(session.arrangement.midi_clips.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_command_does_not_change_current_session() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-invalid-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let before = fs::read(root.join("scratch/current.json")).unwrap();
        assert!(
            dispatcher
                .dispatch(request("track.remove", json!({"trackId":"track:missing"}),))
                .is_err()
        );
        assert_eq!(fs::read(root.join("scratch/current.json")).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interactive_history_undoes_and_redoes_a_committed_edit() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-history-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();

        let undone = dispatcher.dispatch(request("undo", json!({}))).unwrap();
        assert_eq!(undone.result_type, "arrangementMutation");
        assert_eq!(
            undone.value["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let redone = dispatcher.dispatch(request("redo", json!({}))).unwrap();
        assert_eq!(redone.result_type, "arrangementMutation");
        assert_eq!(
            redone.value["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_mutations_report_when_runtime_projection_is_unnecessary() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-effect-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();

        let settings = dispatcher
            .dispatch(request(
                "session.settings.update",
                json!({"note":"authoring note"}),
            ))
            .unwrap();
        assert_eq!(
            settings.projection_effect,
            super::CanonicalMutationEffect::CanonicalOnly
        );

        let marker = dispatcher
            .dispatch(request("marker.add", json!({"name":"Verse","tick":0})))
            .unwrap();
        assert_eq!(
            marker.projection_effect,
            super::CanonicalMutationEffect::CanonicalOnly
        );

        let metronome = dispatcher
            .dispatch(request(
                "session.settings.update",
                json!({"metronomeEnabled":true}),
            ))
            .unwrap();
        assert_eq!(
            metronome.projection_effect,
            super::CanonicalMutationEffect::ProjectArrangement
        );

        let _ = fs::remove_dir_all(root);
    }
}
