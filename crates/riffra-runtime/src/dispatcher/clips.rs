//! clips command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "audio-clip.list"
            | "audio-clip.add-asset"
            | "audio-clip.update"
            | "audio-clip.move"
            | "audio-clip.trim"
            | "audio-clip.split"
            | "audio-clip.duplicate"
            | "audio-clip.crossfade"
            | "midi-clip.list"
            | "midi-clip.create"
            | "midi-clip.add-asset"
            | "midi-clip.update"
            | "midi-clip.move"
            | "midi-clip.trim"
            | "midi-clip.split"
            | "midi-clip.duplicate"
            | "midi-note.add"
            | "midi-note.insert"
            | "midi-note.update"
            | "midi-note.update-many"
            | "midi-note.remove"
            | "midi-note.remove-many"
            | "midi-note.clear"
            | "midi-note.quantize"
            | "midi-note.transform"
            | "midi-note.duplicate"
            | "clip.remove"
            | "clip.paste"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "audio-clip.list" => dispatcher.value(
            "audioClips",
            canonical.session.arrangement.audio_clips.clone(),
        ),
        "audio-clip.add-asset" => dispatcher.add_audio_clip(decode(request.params)?)?,
        "audio-clip.update" => {
            let params: ClipPatchParams<AudioClipPatch> = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_audio_clip(&params.clip_id, params.patch)?,
            )
        }
        "audio-clip.move" => {
            let params: MovesParams<AudioClipMove> = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .move_audio_clips(params.moves)?,
            )
        }
        "audio-clip.trim" => {
            let params: AudioTrimParams = decode(request.params)?;
            let source_frames = dispatcher.audio_source_frames(&params.clip_id)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .trim_audio_clip(
                        &params.clip_id,
                        TimelineTick(params.start_tick),
                        params.source_range,
                        source_frames,
                    )?,
            )
        }
        "audio-clip.split" => {
            let params: SplitParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .split_audio_clip(&params.clip_id, TimelineTick(params.split_tick))?,
            )
        }
        "audio-clip.duplicate" => {
            let params: ClipIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .duplicate_audio_clip(&params.clip_id)?,
            )
        }
        "audio-clip.crossfade" => {
            let params: CrossfadeParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .crossfade_audio_clips(&params.first_clip_id, &params.second_clip_id)?,
            )
        }
        "midi-clip.list" => dispatcher.value(
            "midiClips",
            canonical.session.arrangement.midi_clips.clone(),
        ),
        "midi-clip.create" => {
            let params: MidiClipCreateParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .create_midi_clip(
                        &params.track_id,
                        TimelineTick(params.start_tick),
                        params.duration_ticks,
                        params.name,
                    )?,
            )
        }
        "midi-clip.add-asset" => dispatcher.add_midi_clip(decode(request.params)?)?,
        "midi-clip.update" => {
            let params: ClipPatchParams<MidiClipPatch> = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_midi_clip(&params.clip_id, params.patch)?,
            )
        }
        "midi-clip.move" => {
            let params: MovesParams<MidiClipMove> = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .move_midi_clips(params.moves)?,
            )
        }
        "midi-clip.trim" => {
            let params: MidiTrimParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .trim_midi_clip(
                        &params.clip_id,
                        TimelineTick(params.start_tick),
                        params.duration_ticks,
                    )?,
            )
        }
        "midi-clip.split" => {
            let params: SplitParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .split_midi_clip(&params.clip_id, TimelineTick(params.split_tick))?,
            )
        }
        "midi-clip.duplicate" => {
            let params: ClipIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .duplicate_midi_clip(&params.clip_id)?,
            )
        }
        "midi-note.add" => {
            let params: MidiNoteAddParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_midi_note(
                        &params.clip_id,
                        TimelineTick(params.start_tick),
                        params.pitch,
                        params.duration_ticks,
                        params.velocity,
                        params.channel,
                    )?,
            )
        }
        "midi-note.insert" => {
            let params: MidiNoteInsertParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .insert_midi_notes(&params.clip_id, params.notes)?,
            )
        }
        "midi-note.update" => {
            let params: MidiNoteUpdateParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_midi_notes(
                        &params.clip_id,
                        vec![MidiNoteUpdate {
                            note_id: params.note_id,
                            patch: params.patch,
                        }],
                    )?,
            )
        }
        "midi-note.update-many" => {
            let params: MidiNoteUpdatesParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_midi_notes(&params.clip_id, params.updates)?,
            )
        }
        "midi-note.remove" => {
            let params: MidiNoteIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_midi_note(&params.clip_id, &params.note_id)?,
            )
        }
        "midi-note.remove-many" => {
            let params: MidiNoteIdsParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_midi_notes(&params.clip_id, params.note_ids)?,
            )
        }
        "midi-note.clear" => {
            let params: ClipIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .clear_midi_notes(&params.clip_id)?,
            )
        }
        "midi-note.quantize" => {
            let params: MidiNoteQuantizeParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .quantize_midi_notes(&params.clip_id, params.note_ids, params.grid_ticks)?,
            )
        }
        "midi-note.transform" => {
            let params: MidiNoteTransformParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .transform_midi_notes(
                        &params.clip_id,
                        params.note_ids,
                        params.transpose_semitones,
                        params.velocity_offset,
                    )?,
            )
        }
        "midi-note.duplicate" => {
            let params: MidiNoteDuplicateParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .duplicate_midi_notes(&params.clip_id, params.note_ids, params.offset_ticks)?,
            )
        }
        "clip.remove" => {
            let params: ClipRemoveParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_timeline_clips(params.audio_clip_ids, params.midi_clip_ids)?,
            )
        }
        "clip.paste" => {
            let params: ClipPasteParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .paste_timeline_clips(
                        params.audio_clip_ids,
                        params.midi_clip_ids,
                        TimelineTick(params.start_tick),
                    )?,
            )
        }
        _ => unreachable!("unsupported clips command family"),
    })
}

impl<'a, A> HostDispatcher<'a, A> {
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
