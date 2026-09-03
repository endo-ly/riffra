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
    fn session_inspect_is_read_only_scoped_and_lightweight() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-inspect-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let clip = dispatcher
            .dispatch(request(
                "music.midi-clip.create",
                json!({"trackId":track_id,"start":"1:1","end":"5:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(clip.value).unwrap();
        let clip_id = session.arrangement.midi_clips[0].id.clone();
        dispatcher
            .dispatch(request(
                "music.note.insert",
                json!({
                    "clipId":clip_id,
                    "notes":[{"pitch":"C4","position":"2:1","duration":"1/4"}]
                }),
            ))
            .unwrap();
        let before = dispatcher
            .dispatch(request("session.get", json!({})))
            .unwrap();
        let inspected = dispatcher
            .dispatch(request("session.inspect", json!({})))
            .unwrap();

        assert_eq!(inspected.result_type, "sessionInspection");
        assert_eq!(inspected.sequence, before.sequence);
        assert_eq!(inspected.value["counts"]["tracks"], 1);
        assert_eq!(inspected.value["counts"]["midiClips"], 1);
        assert_eq!(inspected.value["counts"]["midiNotes"], 1);
        assert_eq!(inspected.value["tracks"][0]["clips"][0]["kind"], "midi");
        assert_eq!(inspected.value["tracks"][0]["clips"][0]["noteCount"], 1);
        let encoded = inspected.value.to_string();
        for field in [
            "notes",
            "events",
            "points",
            "stateData",
            "parameterValues",
            "startTick",
            "endTick",
        ] {
            assert!(!encoded.contains(field), "unexpected field {field}");
        }

        let focused = dispatcher
            .dispatch(request(
                "session.inspect",
                json!({"start":"3:1","end":"4:1","trackId":track_id}),
            ))
            .unwrap();
        assert_eq!(focused.value["selection"]["start"], "3:1");
        assert_eq!(focused.value["selection"]["end"], "4:1");
        assert_eq!(focused.value["counts"]["midiClips"], 1);
        assert_eq!(focused.value["counts"]["midiNotes"], 0);
        assert_eq!(focused.value["tracks"].as_array().unwrap().len(), 1);

        let after = dispatcher
            .dispatch(request("session.get", json!({})))
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(after.value, before.value);
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
    fn timebase_update_patches_only_the_requested_fields() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-timebase-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();

        let updated = dispatcher
            .dispatch(request("timebase.update", json!({"bpm": 140.0})))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(session.arrangement.timebase.ppq, 960);
        assert_eq!(session.arrangement.timebase.bpm, 140.0);
        assert_eq!(session.arrangement.timebase.time_signature_numerator, 4);
        assert_eq!(session.arrangement.timebase.time_signature_denominator, 4);

        let updated = dispatcher
            .dispatch(request(
                "timebase.update",
                json!({
                    "bpm": 100.0,
                    "timeSignatureNumerator": 7,
                    "timeSignatureDenominator": 8
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(
            session.arrangement.timebase,
            riffra_core::ProjectTimebase {
                ppq: 960,
                bpm: 100.0,
                time_signature_numerator: 7,
                time_signature_denominator: 8,
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timebase_update_rejects_ppq_as_an_external_field() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-ppq-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let error = dispatcher
            .dispatch(request("timebase.update", json!({"ppq": 960})))
            .unwrap_err();
        assert!(matches!(error, super::DispatchError::InvalidRequest(_)));
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

    #[test]
    fn musical_commands_create_canonical_notes_and_regions() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-music-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let created = dispatcher
            .dispatch(request(
                "music.midi-clip.create",
                json!({
                    "trackId": track_id,
                    "start": "5:1",
                    "end": "13:1",
                    "name": "Piano"
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(created.value).unwrap();
        let clip_id = session.arrangement.midi_clips[0].id.clone();
        let region = dispatcher
            .dispatch(request(
                "music.region.add",
                json!({"name":"A'","start":"5:1","end":"13:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(region.value).unwrap();
        assert_eq!(session.arrangement.regions[0].name, "A'");
        let inserted = dispatcher
            .dispatch(request(
                "music.note.insert",
                json!({
                    "clipId": clip_id,
                    "notes": [
                        {"pitch":"C4","position":"5:1","duration":"1/8"},
                        {"pitch":"E4","position":"5:1+1/2","duration":"1/8"},
                        {"pitch":"G4","position":"5:2","duration":"1/2","velocity":92},
                        {"pitch":"Bb4","position":"6:3+1/3","duration":"1/12"}
                    ]
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(inserted.value).unwrap();
        let clip = &session.arrangement.midi_clips[0];
        assert_eq!(clip.start_tick, riffra_core::TimelineTick(15_360));
        assert_eq!(clip.duration_ticks, 30_720);
        assert_eq!(clip.notes.len(), 4);
        assert_eq!(clip.notes[0].note, 60);
        assert_eq!(clip.notes[0].start_tick, riffra_core::TimelineTick(0));
        assert_eq!(clip.notes[1].start_tick, riffra_core::TimelineTick(480));
        assert_eq!(clip.notes[2].note, 67);
        assert_eq!(clip.notes[3].note, 70);
        assert_eq!(clip.notes[3].start_tick, riffra_core::TimelineTick(6_080));
        assert_eq!(clip.notes[3].duration_ticks, 320);

        let listed = dispatcher
            .dispatch(request("music.region.list", json!({})))
            .unwrap();
        assert_eq!(listed.result_type, "regions");
        assert_eq!(listed.value.as_array().unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn harmony_and_phrase_commands_use_music_level_contracts() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-harmony-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let clip = dispatcher
            .dispatch(request(
                "music.midi-clip.create",
                json!({
                    "trackId": session.arrangement.tracks[0].id,
                    "start": "1:1",
                    "end": "3:1"
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(clip.value).unwrap();
        let clip_id = session.arrangement.midi_clips[0].id.clone();

        let resolved = dispatcher
            .dispatch(request(
                "music.harmony.resolve",
                json!({"chord":"G7(b9,#11)/F"}),
            ))
            .unwrap();
        assert_eq!(resolved.result_type, "harmonyChord");
        assert_eq!(resolved.value["root"], "G");
        assert_eq!(resolved.value["bass"], "F");
        assert_eq!(
            resolved.value["tones"],
            json!(["G", "B", "D", "F", "Ab", "C#"])
        );

        let inserted = dispatcher
            .dispatch(request(
                "music.harmony.insert",
                json!({
                    "events": [
                        {"start":"1:1","end":"2:1","chord":"C/E"},
                        {"start":"2:1","end":"3:1","pitches":["Bb","C","E"],"bass":"F","label":"cluster"}
                    ]
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession =
            serde_json::from_value(inserted.value.clone()).unwrap();
        let harmony_ids = session
            .arrangement
            .harmony_events
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(harmony_ids.len(), 2);

        let listed = dispatcher
            .dispatch(request("music.harmony.list", json!({})))
            .unwrap();
        assert_eq!(listed.result_type, "harmonyEvents");
        assert_eq!(listed.value[0]["start"], "1:1");
        assert!(listed.value[0].get("startTick").is_none());

        let realized = dispatcher
            .dispatch(request(
                "music.harmony.realize",
                json!({"clipId":clip_id,"start":"1:1","end":"3:1"}),
            ))
            .unwrap();
        assert_eq!(
            realized.projection_effect(),
            super::CanonicalMutationEffect::ProjectArrangement
        );
        let updated = dispatcher
            .dispatch(request(
                "music.harmony.update",
                json!({"eventId": harmony_ids[0], "chord":"Dm9"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession =
            serde_json::from_value(updated.value.clone()).unwrap();
        assert_eq!(session.arrangement.harmony_events[0].chord.name, "Dm9");

        let phrase = dispatcher
            .dispatch(request(
                "music.phrase.insert",
                json!({
                    "clipId": clip_id,
                    "pattern": {
                        "length":"1/4",
                        "notes":[
                            {"offset":"0/1","duration":"1/8","semitones":0},
                            {"offset":"1/8","duration":"1/8","semitones":2}
                        ]
                    },
                    "placements":[{"position":"1:1","anchor":"C4","repeats":1}]
                }),
            ))
            .unwrap();
        assert_eq!(
            phrase.projection_effect(),
            super::CanonicalMutationEffect::ProjectArrangement
        );
        let session: riffra_core::CreativeSession =
            serde_json::from_value(phrase.value.clone()).unwrap();
        assert_eq!(session.arrangement.midi_clips[0].notes.len(), 9);

        dispatcher
            .dispatch(request(
                "music.harmony.remove",
                json!({"eventIds":[harmony_ids[0], harmony_ids[1]]}),
            ))
            .unwrap();
        let listed = dispatcher
            .dispatch(request("music.harmony.list", json!({})))
            .unwrap();
        assert!(listed.value.as_array().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clearing_midi_notes_preserves_clip_and_is_undoable() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-clear-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let created = dispatcher
            .dispatch(request(
                "midi-clip.create",
                json!({
                    "trackId": track_id,
                    "startTick": 480,
                    "durationTicks": 1920,
                    "name": "Lead"
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(created.value).unwrap();
        let original_clip = session.arrangement.midi_clips[0].clone();
        let clip_id = original_clip.id.clone();
        dispatcher
            .dispatch(request(
                "midi-note.insert",
                json!({
                    "clipId": clip_id,
                    "notes": [{
                        "pitch": 60,
                        "startTick": 0,
                        "durationTicks": 480,
                        "velocity": 100,
                        "channel": 1
                    }]
                }),
            ))
            .unwrap();

        let cleared = dispatcher
            .dispatch(request("midi-note.clear", json!({"clipId": clip_id})))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(cleared.value).unwrap();
        let cleared_clip = &session.arrangement.midi_clips[0];
        assert!(cleared_clip.notes.is_empty());
        assert_eq!(cleared_clip.id, original_clip.id);
        assert_eq!(cleared_clip.name, original_clip.name);
        assert_eq!(cleared_clip.start_tick, original_clip.start_tick);
        assert_eq!(cleared_clip.duration_ticks, original_clip.duration_ticks);

        let undone = dispatcher.dispatch(request("undo", json!({}))).unwrap();
        assert_eq!(
            undone.value["canonical"]["session"]["arrangement"]["midiClips"][0]["notes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn track_list_omits_device_parameter_values() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-track-list-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let added = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(added.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let instrument = dispatcher
            .dispatch(request(
                "instrument.set",
                json!({
                    "trackId": track_id,
                    "pluginPath": "C:\\Plugins\\Synth.vst3"
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession =
            serde_json::from_value(instrument.value["canonical"]["session"].clone()).unwrap();
        let device_id = session.arrangement.tracks[0]
            .instrument
            .as_ref()
            .unwrap()
            .id
            .clone();
        dispatcher
            .dispatch(request(
                "device.parameter.set",
                json!({
                    "trackId": track_id,
                    "deviceId": device_id,
                    "parameterIndex": 0,
                    "value": 0.5
                }),
            ))
            .unwrap();

        let listed = dispatcher
            .dispatch(request("track.list", json!({})))
            .unwrap();
        let track = &listed.value[0];
        assert_eq!(track["name"], "Keys");
        assert!(track["instrument"].get("parameterValues").is_none());
        assert!(
            track["rack"]["devices"]
                .as_array()
                .unwrap()
                .iter()
                .all(|device| device.get("parameterValues").is_none())
        );
        let _ = fs::remove_dir_all(root);
    }
}
