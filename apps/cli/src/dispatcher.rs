use crate::args::CommandRequest;
use riffra_core::application::{
    AudioAssetClipPlacement, MarkerPatch, MidiAssetClipPlacement, MidiNoteInput, MidiNotePatch,
    MidiNoteUpdate,
};
use riffra_core::{
    AppCore, ApplicationError, AssetId, AssetKind, AudioClipMove, AudioClipPatch,
    AutomationParameter, AutomationPoint, CreativeSession, FrameRange, MidiClipMove, MidiClipPatch,
    MidiInputRoute, ProjectTimebase, TimelineTick, TrackKind, TrackPatch,
};
use riffra_host::{DataRootLease, SessionStore, now_ms};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub struct DispatchError(String);

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<String> for DispatchError {
    fn from(error: String) -> Self {
        Self(error)
    }
}

impl From<&'static str> for DispatchError {
    fn from(error: &'static str) -> Self {
        Self(error.into())
    }
}

impl From<ApplicationError> for DispatchError {
    fn from(error: ApplicationError) -> Self {
        Self(error.to_string())
    }
}

pub struct Dispatcher {
    _lease: DataRootLease,
    core: AppCore<()>,
    storage: SessionStore,
    data_root: PathBuf,
}

#[derive(Debug)]
pub struct DispatchResult {
    pub result_type: &'static str,
    pub value: Value,
    pub sequence: u64,
}

impl Dispatcher {
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
            _lease: lease,
            core,
            storage,
            data_root,
        })
    }

    pub fn dispatch(&self, request: CommandRequest) -> Result<DispatchResult, DispatchError> {
        let result = match request.command.as_str() {
            "session.get" => self.session(self.core.application(&self.storage).get_session()?),
            "session.settings.update" => self.session(
                self.core
                    .application(&self.storage)
                    .update_session_settings(decode(request.params)?)?,
            ),
            "history.get" => self.value(
                "history",
                self.core.application(&self.storage).history_state()?,
            ),
            "track.list" => self.value(
                "tracks",
                self.core.application(&self.storage).list_tracks()?,
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
                self.core.application(&self.storage).list_audio_clips()?,
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
                self.core.application(&self.storage).list_midi_clips()?,
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
                self.session(
                    self.core
                        .application(&self.storage)
                        .add_marker(TimelineTick(params.tick), params.name)?,
                )
            }
            "marker.update" => {
                let params: MarkerUpdateParams = decode(request.params)?;
                self.session(self.core.application(&self.storage).update_marker(
                    &params.marker_id,
                    MarkerPatch {
                        name: params.name,
                        tick: params.tick.map(TimelineTick),
                    },
                )?)
            }
            "marker.remove" => {
                let params: MarkerIdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .remove_marker(&params.marker_id)?,
                )
            }
            "timebase.update" => {
                let params: TimebaseParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .update_timebase(params.timebase)?,
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
            "project.export" => {
                let session = self.core.application(&self.storage).get_session()?;
                self.value(
                    "projectExport",
                    riffra_host::export_project(&self.data_root, &session, now_ms())?,
                )
            }
            "project.import" => {
                let params: ProjectImportParams = decode(request.params)?;
                let session = riffra_host::import_project(&self.data_root, &params.path)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .import_project(session)?,
                )
            }
            "instrument.clear" => {
                let params: IdParams = decode(request.params)?;
                self.session(
                    self.core
                        .application(&self.storage)
                        .set_track_instrument(&params.track_id, None)?,
                )
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
            "undo" => self.session(self.core.application(&self.storage).undo()?),
            "redo" => self.session(self.core.application(&self.storage).redo()?),
            _ => return Err(format!("unknown command: {}", request.command).into()),
        };
        let sequence = self
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .sequence;
        Ok(DispatchResult {
            result_type: result.result_type,
            value: result.value,
            sequence,
        })
    }

    fn session(&self, session: CreativeSession) -> DispatchResult {
        self.value("session", session)
    }

    fn value<T: serde::Serialize>(&self, result_type: &'static str, value: T) -> DispatchResult {
        DispatchResult {
            result_type,
            value: serde_json::to_value(value).expect("canonical values must serialize"),
            sequence: 0,
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

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, DispatchError> {
    Ok(serde_json::from_value(value)
        .map_err(|error| format!("invalid command parameters: {error}"))?)
}

fn parse_asset_id(value: &str) -> Result<AssetId, DispatchError> {
    Ok(AssetId::from_normalized(value).map_err(|error| format!("Asset id is invalid: {error}"))?)
}

fn parse_track_kind(value: &str) -> Result<TrackKind, DispatchError> {
    match value {
        "audio" => Ok(TrackKind::Audio),
        "instrument" => Ok(TrackKind::Instrument),
        _ => Err("track kind must be audio or instrument".into()),
    }
}

fn parse_automation_parameter(value: &str) -> Result<AutomationParameter, DispatchError> {
    match value {
        "volume" => Ok(AutomationParameter::Volume),
        "pan" => Ok(AutomationParameter::Pan),
        _ => Err("automation parameter must be volume or pan".into()),
    }
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
struct AudioInputParams {
    track_id: String,
    channel_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MidiInputParams {
    track_id: String,
    device_id: Option<String>,
    channel: Option<u8>,
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
    #[serde(default)]
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
#[serde(rename_all = "camelCase")]
struct TimebaseParams {
    #[serde(flatten)]
    timebase: ProjectTimebase,
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
struct EffectRemoveParams {
    track_id: String,
    device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectReorderParams {
    track_id: String,
    device_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceBypassParams {
    track_id: String,
    device_id: String,
    bypassed: bool,
}

#[cfg(test)]
mod tests {
    use super::Dispatcher;
    use crate::args::CommandRequest;
    use riffra_host::now_ms;
    use serde_json::{Value, json};
    use std::fs;

    fn request(command: &str, params: Value) -> CommandRequest {
        CommandRequest {
            command: command.into(),
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
        let undone: riffra_core::CreativeSession = serde_json::from_value(undone.value).unwrap();
        assert!(undone.arrangement.tracks.is_empty());

        let redone = dispatcher.dispatch(request("redo", json!({}))).unwrap();
        let redone: riffra_core::CreativeSession = serde_json::from_value(redone.value).unwrap();
        assert_eq!(redone.arrangement.tracks.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
