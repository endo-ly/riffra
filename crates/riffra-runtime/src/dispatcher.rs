use crate::model::{ArrangementMutationResult, ArrangementProjectionOutcome, TrackSummary};
use crate::session::commit::CanonicalMutationEffect;
use riffra_control::{ControlCommand, ControlRequest, ErrorCode, ProtocolError};
use riffra_core::application::{
    AudioAssetClipPlacement, ChordVoicingInput, HarmonyEventInput, HarmonyEventPatch,
    HarmonyRealizeSelection, MarkerPatch, MidiAssetClipPlacement, MidiNoteInput, MidiNotePatch,
    MidiNoteUpdate, MusicalMidiNoteInput, SessionSettingsPatch,
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
        let result = match request.name.as_str() {
            "session.get" => self.session(canonical.session.clone()),
            "session.settings.update" => {
                let params: SessionSettingsPatch = decode(request.params)?;
                let effect = if params.metronome_enabled.is_some() {
                    CanonicalMutationEffect::ProjectArrangement
                } else {
                    CanonicalMutationEffect::CanonicalOnly
                };
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .update_session_settings(params)?,
                    effect,
                )
            }
            "history.get" => self.value("history", canonical.history),
            "track.list" => self.value(
                "tracks",
                canonical
                    .session
                    .arrangement
                    .tracks
                    .iter()
                    .map(TrackSummary::from_track)
                    .collect::<Vec<_>>(),
            ),
            "track.add" => {
                let params: TrackAddParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .add_track(params.name, parse_track_kind(&params.kind)?)?,
                )
            }
            "track.update" => {
                let params: TrackUpdateParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_track(&params.track_id, params.patch)?,
                )
            }
            "track.remove" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_track(&params.track_id)?,
                )
            }
            "track.duplicate" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .duplicate_track(&params.track_id)?,
                )
            }
            "track.reorder" => {
                let params: ReorderParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .reorder_track(&params.track_id, params.target_index)?,
                )
            }
            "track.audio-input.set" => {
                let params: AudioInputParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_audio_input(&params.track_id, Some(params.channel_index))?,
                )
            }
            "track.audio-input.clear" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_audio_input(&params.track_id, None)?,
                )
            }
            "track.midi-input.set" => {
                let params: MidiInputParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).set_track_midi_input(
                    &params.track_id,
                    MidiInputRoute {
                        device_id: params.device_id,
                        channel: params.channel,
                    },
                )?)
            }
            "track.midi-input.clear" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_midi_input(&params.track_id, MidiInputRoute::default())?,
                )
            }
            "audio-clip.list" => self.value(
                "audioClips",
                canonical.session.arrangement.audio_clips.clone(),
            ),
            "audio-clip.add-asset" => self.add_audio_clip(decode(request.params)?)?,
            "audio-clip.update" => {
                let params: ClipPatchParams<AudioClipPatch> = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_audio_clip(&params.clip_id, params.patch)?,
                )
            }
            "audio-clip.move" => {
                let params: MovesParams<AudioClipMove> = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .move_audio_clips(params.moves)?,
                )
            }
            "audio-clip.trim" => {
                let params: AudioTrimParams = decode(request.params)?;
                let source_frames = self.audio_source_frames(&params.clip_id)?;
                self.session(self.core.application(&self.storage).trim_audio_clip(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.source_range,
                    source_frames,
                )?)
            }
            "audio-clip.split" => {
                let params: SplitParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .split_audio_clip(&params.clip_id, TimelineTick(params.split_tick))?,
                )
            }
            "audio-clip.duplicate" => {
                let params: ClipIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .duplicate_audio_clip(&params.clip_id)?,
                )
            }
            "audio-clip.crossfade" => {
                let params: CrossfadeParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .crossfade_audio_clips(&params.first_clip_id, &params.second_clip_id)?,
                )
            }
            "midi-clip.list" => self.value(
                "midiClips",
                canonical.session.arrangement.midi_clips.clone(),
            ),
            "midi-clip.create" => {
                let params: MidiClipCreateParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).create_midi_clip(
                    &params.track_id,
                    TimelineTick(params.start_tick),
                    params.duration_ticks,
                    params.name,
                )?)
            }
            "midi-clip.add-asset" => self.add_midi_clip(decode(request.params)?)?,
            "midi-clip.update" => {
                let params: ClipPatchParams<MidiClipPatch> = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_midi_clip(&params.clip_id, params.patch)?,
                )
            }
            "midi-clip.move" => {
                let params: MovesParams<MidiClipMove> = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .move_midi_clips(params.moves)?,
                )
            }
            "midi-clip.trim" => {
                let params: MidiTrimParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).trim_midi_clip(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.duration_ticks,
                )?)
            }
            "midi-clip.split" => {
                let params: SplitParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .split_midi_clip(&params.clip_id, TimelineTick(params.split_tick))?,
                )
            }
            "midi-clip.duplicate" => {
                let params: ClipIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .duplicate_midi_clip(&params.clip_id)?,
                )
            }
            "midi-note.add" => {
                let params: MidiNoteAddParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).add_midi_note(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.pitch,
                    params.duration_ticks,
                    params.velocity,
                    params.channel,
                )?)
            }
            "midi-note.insert" => {
                let params: MidiNoteInsertParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .insert_midi_notes(&params.clip_id, params.notes)?,
                )
            }
            "music.midi-clip.create" => {
                let params: MusicalMidiClipCreateParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .create_musical_midi_clip(
                            &params.track_id,
                            params.start,
                            params.end,
                            params.name,
                        )?,
                )
            }
            "music.note.insert" => {
                let params: MusicalNoteInsertParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .insert_musical_notes(&params.clip_id, params.notes)?,
                )
            }
            "music.harmony.resolve" => {
                let params: MusicalHarmonyResolveParams = decode(request.params)?;
                self.value(
                    "harmonyChord",
                    self.core
                        .application(&self.storage)
                        .resolve_harmony_chord(&params.chord)?,
                )
            }
            "music.harmony.list" => self.value(
                "harmonyEvents",
                self.core.application(&self.storage).list_harmony_events()?,
            ),
            "music.harmony.insert" => {
                let params: MusicalHarmonyInsertParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .insert_harmony_events(params.events)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.harmony.update" => {
                let params: MusicalHarmonyUpdateParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .update_harmony_event(&params.event_id, params.patch)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.harmony.remove" => {
                let params: MusicalHarmonyRemoveParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .remove_harmony_events(params.event_ids)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.harmony.realize" => {
                let params: MusicalHarmonyRealizeParams = decode(request.params)?;
                self.session_with_effect(
                    self.core.application(&self.storage).realize_harmony(
                        &params.clip_id,
                        HarmonyRealizeSelection {
                            start: params.start,
                            end: params.end,
                        },
                        ChordVoicingInput {
                            lowest_octave: params.lowest_octave.unwrap_or(3),
                        },
                        params.rhythm,
                        params.velocity,
                        params.channel,
                    )?,
                    CanonicalMutationEffect::ProjectArrangement,
                )
            }
            "music.phrase.insert" => {
                let params: MusicalPhraseInsertParams = decode(request.params)?;
                self.session_with_effect(
                    self.core.application(&self.storage).insert_phrase_pattern(
                        &params.clip_id,
                        params.pattern,
                        params.placements,
                        params.channel,
                    )?,
                    CanonicalMutationEffect::ProjectArrangement,
                )
            }
            "midi-note.update" => {
                let params: MidiNoteUpdateParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).update_midi_notes(
                    &params.clip_id,
                    vec![MidiNoteUpdate {
                        note_id: params.note_id,
                        patch: params.patch,
                    }],
                )?)
            }
            "midi-note.update-many" => {
                let params: MidiNoteUpdatesParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_midi_notes(&params.clip_id, params.updates)?,
                )
            }
            "midi-note.remove" => {
                let params: MidiNoteIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_midi_note(&params.clip_id, &params.note_id)?,
                )
            }
            "midi-note.remove-many" => {
                let params: MidiNoteIdsParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_midi_notes(&params.clip_id, params.note_ids)?,
                )
            }
            "midi-note.clear" => {
                let params: ClipIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .clear_midi_notes(&params.clip_id)?,
                )
            }
            "midi-note.quantize" => {
                let params: MidiNoteQuantizeParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).quantize_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.grid_ticks,
                )?)
            }
            "midi-note.transform" => {
                let params: MidiNoteTransformParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).transform_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.transpose_semitones,
                    params.velocity_offset,
                )?)
            }
            "midi-note.duplicate" => {
                let params: MidiNoteDuplicateParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).duplicate_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.offset_ticks,
                )?)
            }
            "clip.remove" => {
                let params: ClipRemoveParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_timeline_clips(params.audio_clip_ids, params.midi_clip_ids)?,
                )
            }
            "clip.paste" => {
                let params: ClipPasteParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).paste_timeline_clips(
                    params.audio_clip_ids,
                    params.midi_clip_ids,
                    TimelineTick(params.start_tick),
                )?)
            }
            "marker.add" => {
                let params: MarkerAddParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .add_marker(TimelineTick(params.tick), params.name)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "marker.update" => {
                let params: MarkerUpdateParams = decode(request.params)?;
                self.session_with_effect(
                    self.core.application(&self.storage).update_marker(
                        &params.marker_id,
                        MarkerPatch {
                            name: params.name,
                            tick: params.tick.map(TimelineTick),
                        },
                    )?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "marker.remove" => {
                let params: MarkerIdParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .remove_marker(&params.marker_id)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.region.list" => self.value(
                "regions",
                self.core.application(&self.storage).list_regions()?,
            ),
            "music.region.add" => {
                let params: MusicalRegionAddParams = decode(request.params)?;
                self.session_with_effect(
                    self.core.application(&self.storage).add_region(
                        params.name,
                        params.start,
                        params.end,
                    )?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.region.update" => {
                let params: MusicalRegionUpdateParams = decode(request.params)?;
                self.session_with_effect(
                    self.core.application(&self.storage).update_region(
                        &params.region_id,
                        params.name,
                        params.start,
                        params.end,
                    )?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "music.region.remove" => {
                let params: MusicalRegionIdParams = decode(request.params)?;
                self.session_with_effect(
                    self.core
                        .application(&self.storage)
                        .remove_region(&params.region_id)?,
                    CanonicalMutationEffect::CanonicalOnly,
                )
            }
            "timebase.update" => {
                let params: TimebasePatchParams = decode(request.params)?;
                if params.is_empty() {
                    return Err(DispatchError::invalid_request(
                        "timebase update requires at least one field",
                    ));
                }
                let current = canonical.session.arrangement.timebase;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_timebase(ProjectTimebase {
                            ppq: current.ppq,
                            bpm: params.bpm.unwrap_or(current.bpm),
                            time_signature_numerator: params
                                .time_signature_numerator
                                .unwrap_or(current.time_signature_numerator),
                            time_signature_denominator: params
                                .time_signature_denominator
                                .unwrap_or(current.time_signature_denominator),
                        })?,
                )
            }
            "loop-range.set" => {
                let params: RangeParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).update_loop_range(
                    params.enabled,
                    TimelineTick(params.start_tick),
                    TimelineTick(params.end_tick),
                )?)
            }
            "punch-range.set" => {
                let params: RangeParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).update_punch_range(
                    params.enabled,
                    TimelineTick(params.start_tick),
                    TimelineTick(params.end_tick),
                )?)
            }
            "automation.set" => {
                let params: AutomationParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).set_track_automation(
                    &params.track_id,
                    parse_automation_parameter(&params.parameter)?,
                    params.points,
                )?)
            }
            "automation.clear" => {
                let params: AutomationClearParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).set_track_automation(
                    &params.track_id,
                    parse_automation_parameter(&params.parameter)?,
                    Vec::new(),
                )?)
            }
            "asset.import-midi" => {
                let params: AssetImportParams = decode(request.params)?;
                let asset_id = riffra_host::import_midi_asset(
                    &self.data_root,
                    &params.path.to_string_lossy(),
                    params.name.as_deref(),
                )?;
                self.value("assetId", asset_id)
            }
            "project.export" => self.value(
                "projectExport",
                riffra_host::export_project(&self.data_root, &canonical.session, now_ms())?,
            ),
            "project.import" => {
                let params: ProjectImportParams = decode(request.params)?;
                let session = riffra_host::import_project(&self.data_root, &params.path)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .import_project(session)?,
                )
            }
            "instrument.set" => {
                let params: PluginPathParams = decode(request.params)?;
                let snapshot = self.core.snapshot()?;
                let track = snapshot
                    .session
                    .arrangement
                    .tracks
                    .iter()
                    .find(|track| track.id == params.track_id)
                    .ok_or_else(|| format!("track is not registered: {}", params.track_id))?;
                let id = track
                    .instrument
                    .as_ref()
                    .map(|device| device.id.clone())
                    .unwrap_or_else(|| format!("device:instrument:{}", params.track_id));
                self.session(self.core.application(&self.storage).set_track_instrument(
                    &params.track_id,
                    Some(plugin_device(id, params.plugin_path)?),
                )?)
            }
            "instrument.clear" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_instrument(&params.track_id, None)?,
                )
            }
            "effect.add" => {
                let params: PluginPathParams = decode(request.params)?;
                let device_id = format!(
                    "device:effect:{}:{}",
                    params.track_id,
                    self.core.snapshot()?.sequence + 1
                );
                self.session(self.core.application(&self.storage).add_track_effect(
                    &params.track_id,
                    plugin_device(device_id, params.plugin_path)?,
                )?)
            }
            "effect.remove" => {
                let params: EffectRemoveParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_track_effect(&params.track_id, &params.device_id)?,
                )
            }
            "effect.reorder" => {
                let params: EffectReorderParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .reorder_track_effects(&params.track_id, params.device_ids)?,
                )
            }
            "device.bypass" => {
                let params: DeviceBypassParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_device_bypassed(
                            &params.track_id,
                            &params.device_id,
                            params.bypassed,
                        )?,
                )
            }
            "device.parameter.set" => {
                let params: DeviceParameterParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_device_parameter(
                            &params.track_id,
                            &params.device_id,
                            params.parameter_index as usize,
                            params.value,
                        )?,
                )
            }
            "missing.relink" => {
                let params: MissingRelinkParams = decode(request.params)?;
                let old_id = parse_asset_id(&params.asset_id)?;
                let path = Path::new(&params.new_path);
                if !path.is_file() {
                    return Err(DispatchError::CommandFailed(format!(
                        "replacement asset does not exist: {}",
                        path.display()
                    )));
                }
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("audio");
                let new_id = riffra_host::register(
                    &self.data_root,
                    AssetKind::Audio,
                    name,
                    &path.to_string_lossy(),
                    Some(riffra_core::Provenance::imported()),
                )?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .replace_asset_references(&old_id, new_id)?,
                )
            }
            "missing.disable-plugin" => {
                let params: DeviceIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .disable_missing_plugin(&params.device_id)?,
                )
            }
            "missing.replace-plugin" => {
                let params: MissingPluginReplaceParams = decode(request.params)?;
                let path = Path::new(&params.new_path);
                if !path.exists() {
                    return Err(DispatchError::CommandFailed(format!(
                        "replacement VST3 path does not exist: {}",
                        path.display()
                    )));
                }
                let snapshot = self.core.snapshot()?;
                let mut replacement = snapshot
                    .session
                    .arrangement
                    .tracks
                    .iter()
                    .flat_map(|track| track.instrument.iter().chain(track.rack.devices.iter()))
                    .find(|device| device.id == params.device_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!("track device is not registered: {}", params.device_id)
                    })?;
                replacement.name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Plugin")
                    .to_owned();
                replacement.path = Some(path.to_string_lossy().into_owned());
                replacement.disabled_placeholder = false;
                self.session(
                    self.core
                        .application(&self.storage)
                        .replace_track_plugin(&params.device_id, replacement)?,
                )
            }
            "undo" => self.session(self.core.application(&self.storage).undo()?),
            "redo" => self.session(self.core.application(&self.storage).redo()?),
            _ => {
                return Err(DispatchError::invalid_request(format!(
                    "unknown command: {}",
                    request.name
                )));
            }
        };
        let sequence = if is_read_command(&request.name) {
            canonical.sequence
        } else {
            self.core
                .snapshot()
                .map_err(|error| error.to_string())?
                .sequence
        };
        if is_arrangement_mutation_command(&request.name) {
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

    fn add_audio_clip(&self, params: AudioAddParams) -> Result<DispatchResult, DispatchError> {
        let asset_id = parse_asset_id(&params.asset_id)?;
        let asset = riffra_host::load(&self.data_root, &asset_id)
            .ok_or_else(|| format!("Audio Asset is not registered: {asset_id}"))?;
        if asset.kind != AssetKind::Audio {
            return Err(format!("Asset {asset_id} is not an audio Asset.").into());
        }
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
        let metadata = riffra_host::parse_wav(&bytes)?;
        if metadata.sample_rate == 0 || metadata.frame_count == 0 {
            return Err("Audio Asset has no usable frames.".into());
        }
        Ok(
            self.session(self.core.application(&self.storage).add_audio_asset_clip(
                AudioAssetClipPlacement {
                    asset_id,
                    name: params.name,
                    start_tick: params.start_tick.map(TimelineTick),
                    track_id: params.track_id,
                    sample_rate: metadata.sample_rate,
                    source_frames: metadata.frame_count,
                },
                |id| riffra_host::load(&self.data_root, id).is_some(),
            )?),
        )
    }

    fn add_midi_clip(&self, params: MidiAddParams) -> Result<DispatchResult, DispatchError> {
        let asset_id = parse_asset_id(&params.asset_id)?;
        let asset = riffra_host::load(&self.data_root, &asset_id)
            .ok_or_else(|| format!("MIDI Asset is not registered: {asset_id}"))?;
        if asset.kind != AssetKind::Midi {
            return Err(format!("Asset {asset_id} is not a MIDI Asset.").into());
        }
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("MIDI Asset could not be read: {error}"))?;
        let (duration_ticks, notes, events) = riffra_host::parse_smf(&bytes)?;
        Ok(
            self.session(self.core.application(&self.storage).add_midi_asset_clip(
                MidiAssetClipPlacement {
                    asset_id,
                    name: params.name,
                    start_tick: params.start_tick.map(TimelineTick),
                    track_id: params.track_id,
                    duration_ticks,
                    notes,
                    events,
                },
            )?),
        )
    }

    fn audio_source_frames(&self, clip_id: &str) -> Result<u64, DispatchError> {
        let session = self
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .session;
        let clip = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| format!("Audio clip '{clip_id}' not found."))?;
        let asset = riffra_host::load(&self.data_root, &clip.asset_id)
            .ok_or_else(|| format!("Audio Asset is not registered: {}", clip.asset_id))?;
        let bytes = std::fs::read(&asset.content_location)
            .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
        Ok(riffra_host::parse_wav(&bytes)?.frame_count)
    }
}

fn is_read_command(command: &str) -> bool {
    matches!(
        command,
        "session.get"
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackAddParams {
    name: String,
    kind: String,
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
struct IdParams {
    track_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderParams {
    track_id: String,
    target_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioInputParams {
    pub(crate) track_id: String,
    pub(crate) channel_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MidiInputParams {
    pub(crate) track_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipPatchParams<T> {
    clip_id: String,
    patch: T,
}

#[derive(Debug, Deserialize)]
struct MovesParams<T> {
    moves: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioAddParams {
    asset_id: String,
    name: String,
    start_tick: Option<u64>,
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioTrimParams {
    clip_id: String,
    start_tick: u64,
    source_range: FrameRange,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitParams {
    clip_id: String,
    split_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipIdParams {
    clip_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrossfadeParams {
    first_clip_id: String,
    second_clip_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiClipCreateParams {
    track_id: String,
    start_tick: u64,
    duration_ticks: u64,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiAddParams {
    asset_id: String,
    name: String,
    start_tick: Option<u64>,
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiTrimParams {
    clip_id: String,
    start_tick: u64,
    duration_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteAddParams {
    clip_id: String,
    pitch: u8,
    start_tick: u64,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteInsertParams {
    clip_id: String,
    notes: Vec<MidiNoteInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalMidiClipCreateParams {
    track_id: String,
    start: riffra_core::MusicalPosition,
    end: riffra_core::MusicalPosition,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalNoteInsertParams {
    clip_id: String,
    notes: Vec<MusicalMidiNoteInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyResolveParams {
    chord: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyInsertParams {
    events: Vec<HarmonyEventInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyUpdateParams {
    event_id: String,
    #[serde(flatten)]
    patch: HarmonyEventPatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyRemoveParams {
    event_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalHarmonyRealizeParams {
    clip_id: String,
    start: Option<riffra_core::MusicalPosition>,
    end: Option<riffra_core::MusicalPosition>,
    lowest_octave: Option<i8>,
    rhythm: Option<RhythmPattern>,
    velocity: Option<u8>,
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalPhraseInsertParams {
    clip_id: String,
    pattern: PhrasePattern,
    placements: Vec<PhrasePlacement>,
    channel: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionAddParams {
    name: String,
    start: riffra_core::MusicalPosition,
    end: riffra_core::MusicalPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionUpdateParams {
    region_id: String,
    name: Option<String>,
    start: Option<riffra_core::MusicalPosition>,
    end: Option<riffra_core::MusicalPosition>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MusicalRegionIdParams {
    region_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteUpdateParams {
    clip_id: String,
    note_id: String,
    patch: MidiNotePatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteUpdatesParams {
    clip_id: String,
    updates: Vec<MidiNoteUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteIdParams {
    clip_id: String,
    note_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteIdsParams {
    clip_id: String,
    note_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteQuantizeParams {
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteTransformParams {
    clip_id: String,
    note_ids: Vec<String>,
    transpose_semitones: i16,
    velocity_offset: i16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiNoteDuplicateParams {
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipRemoveParams {
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipPasteParams {
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerAddParams {
    name: String,
    tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerUpdateParams {
    marker_id: String,
    name: Option<String>,
    tick: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerIdParams {
    marker_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct TimebasePatchParams {
    bpm: Option<f64>,
    time_signature_numerator: Option<u8>,
    time_signature_denominator: Option<u8>,
}

impl TimebasePatchParams {
    fn is_empty(&self) -> bool {
        self.bpm.is_none()
            && self.time_signature_numerator.is_none()
            && self.time_signature_denominator.is_none()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeParams {
    enabled: bool,
    start_tick: u64,
    end_tick: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationParams {
    track_id: String,
    parameter: String,
    points: Vec<AutomationPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationClearParams {
    track_id: String,
    parameter: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetImportParams {
    path: PathBuf,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectImportParams {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectRemoveParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectReorderParams {
    pub(crate) track_id: String,
    pub(crate) device_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceBypassParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPathParams {
    pub(crate) track_id: String,
    pub(crate) plugin_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceParameterParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) parameter_index: u32,
    pub(crate) value: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MissingRelinkParams {
    pub(crate) asset_id: String,
    pub(crate) new_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceIdParams {
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MissingPluginReplaceParams {
    pub(crate) device_id: String,
    pub(crate) new_path: String,
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
