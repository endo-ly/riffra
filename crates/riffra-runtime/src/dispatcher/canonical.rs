//! Execution of canonical commands.

use super::{DispatchError, HostDispatcher};
use crate::api::output::TrackSummary;
use crate::api::{CanonicalCommand, ControlOutput};
use riffra_core::application::{
    ChordVoicingInput, HarmonyEventPatch, HarmonyRealizeSelection, MarkerPatch, MidiNoteUpdate,
    MusicalMidiNotePatch, MusicalNoteListRequest, MusicalNoteScope, MusicalNoteTransformRequest,
    SessionSettingsPatch, inspect_canonical_state,
};
use riffra_core::{CanonicalState, MidiInputRoute, TimelineTick, TrackPatch};

impl<A> HostDispatcher<'_, A> {
    pub(super) fn run_canonical(
        &self,
        command: CanonicalCommand,
        canonical: CanonicalState,
    ) -> Result<ControlOutput, DispatchError> {
        let application = || self.core.application(&self.storage);
        let timebase = canonical.session.arrangement.timebase;
        let tick = |position| {
            timebase
                .musical_position_to_tick(position)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))
        };
        match command {
            CanonicalCommand::SessionGet(_) => Ok(ControlOutput::Session(canonical.session)),
            CanonicalCommand::SessionInspect(query) => Ok(ControlOutput::SessionInspection(
                inspect_canonical_state(&canonical, query)
                    .map_err(|error| DispatchError::invalid_request(error.to_string()))?,
            )),
            CanonicalCommand::SessionApply(params) => self.apply_batch(canonical, params),
            CanonicalCommand::SessionSettingsUpdate(patch) => {
                self.edited(application().update_session_settings(patch)?)
            }
            CanonicalCommand::HistoryGet(_) => Ok(ControlOutput::History(canonical.history)),
            CanonicalCommand::Undo(_) => self.edited(application().undo()?),
            CanonicalCommand::Redo(_) => self.edited(application().redo()?),
            CanonicalCommand::MasterGainSet(params) => {
                if !params.gain_db.is_finite() {
                    return Err(DispatchError::invalid_request("master gain must be finite"));
                }
                self.edited(application().update_session_settings(SessionSettingsPatch {
                    master_db: Some(params.gain_db),
                    ..SessionSettingsPatch::default()
                })?)
            }

            CanonicalCommand::TrackList(_) => Ok(ControlOutput::Tracks(
                canonical
                    .session
                    .arrangement
                    .tracks
                    .iter()
                    .map(TrackSummary::from_track)
                    .collect(),
            )),
            CanonicalCommand::TrackAdd(params) => {
                self.created(application().add_track_with_created_ids(params.name, params.kind)?)
            }
            CanonicalCommand::TrackUpdate(params) => self.edited(application().update_track(
                &params.track_id,
                TrackPatch {
                    name: params.name,
                    gain_db: params.gain_db,
                    pan: params.pan,
                    muted: params.muted,
                    solo: params.solo,
                    armed: params.armed,
                    monitoring: params.monitoring,
                    color: params.color,
                },
            )?),
            CanonicalCommand::TrackRemove(params) => {
                self.edited(application().remove_track(&params.track_id)?)
            }
            CanonicalCommand::TrackDuplicate(params) => {
                self.created(application().duplicate_track_with_created_ids(&params.track_id)?)
            }
            CanonicalCommand::TrackReorder(params) => {
                self.edited(application().reorder_track(&params.track_id, params.target_index)?)
            }
            CanonicalCommand::TrackAudioInputSet(params) => self.edited(
                application()
                    .set_track_audio_input(&params.track_id, Some(params.channel_index))?,
            ),
            CanonicalCommand::TrackAudioInputClear(params) => {
                self.edited(application().set_track_audio_input(&params.track_id, None)?)
            }
            CanonicalCommand::TrackMidiInputSet(params) => {
                self.edited(application().set_track_midi_input(
                    &params.track_id,
                    MidiInputRoute {
                        device_id: params.device_id,
                        channel: params.channel,
                    },
                )?)
            }
            CanonicalCommand::TrackMidiInputClear(params) => self.edited(
                application().set_track_midi_input(&params.track_id, MidiInputRoute::default())?,
            ),
            CanonicalCommand::MarkerAdd(params) => self.created(
                application().add_marker_with_created_ids(tick(params.position)?, params.name)?,
            ),
            CanonicalCommand::MarkerUpdate(params) => self.edited(application().update_marker(
                &params.marker_id,
                MarkerPatch {
                    name: params.name,
                    tick: params.position.map(tick).transpose()?,
                },
            )?),
            CanonicalCommand::MarkerRemove(params) => {
                self.edited(application().remove_marker(&params.marker_id)?)
            }
            CanonicalCommand::TimebaseUpdate(params) => {
                self.edited(application().update_timebase(self.timebase_update(timebase, params)?)?)
            }
            CanonicalCommand::LoopRangeSet(params) => {
                self.edited(application().update_loop_range(
                    params.enabled,
                    tick(params.start)?,
                    tick(params.end)?,
                )?)
            }
            CanonicalCommand::PunchRangeSet(params) => {
                self.edited(application().update_punch_range(
                    params.enabled,
                    tick(params.start)?,
                    tick(params.end)?,
                )?)
            }
            CanonicalCommand::AutomationSet(params) => {
                self.created(application().set_track_automation_with_created_ids(
                    &params.track_id,
                    params.parameter,
                    params.points,
                )?)
            }
            CanonicalCommand::AutomationClear(params) => {
                self.created(application().set_track_automation_with_created_ids(
                    &params.track_id,
                    params.parameter,
                    Vec::new(),
                )?)
            }

            CanonicalCommand::AudioClipList(_) => Ok(ControlOutput::AudioClips(
                canonical.session.arrangement.audio_clips,
            )),
            CanonicalCommand::AudioClipAddAsset(params) => self.add_audio_clip(params),
            CanonicalCommand::AudioClipUpdate(params) => {
                self.edited(application().update_audio_clip(&params.clip_id, params.patch)?)
            }
            CanonicalCommand::AudioClipMove(params) => {
                self.edited(application().move_audio_clips(params.moves)?)
            }
            CanonicalCommand::AudioClipTrim(params) => {
                let source_frames = self.audio_source_frames(&params.clip_id)?;
                self.edited(application().trim_audio_clip(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.source_range,
                    source_frames,
                )?)
            }
            CanonicalCommand::AudioClipSplit(params) => {
                self.created(application().split_audio_clip_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.split_tick),
                )?)
            }
            CanonicalCommand::AudioClipDuplicate(params) => {
                self.created(application().duplicate_audio_clip_with_created_ids(&params.clip_id)?)
            }
            CanonicalCommand::AudioClipCrossfade(params) => self.edited(
                application()
                    .crossfade_audio_clips(&params.first_clip_id, &params.second_clip_id)?,
            ),
            CanonicalCommand::MidiClipList(_) => Ok(ControlOutput::MidiClips(
                canonical.session.arrangement.midi_clips,
            )),
            CanonicalCommand::MidiClipCreate(params) => {
                self.created(application().create_midi_clip_with_created_ids(
                    &params.track_id,
                    TimelineTick(params.start_tick),
                    params.duration_ticks,
                    params.name,
                )?)
            }
            CanonicalCommand::MidiClipAddAsset(params) => self.add_midi_clip(params),
            CanonicalCommand::MidiClipUpdate(params) => {
                self.edited(application().update_midi_clip(&params.clip_id, params.patch)?)
            }
            CanonicalCommand::MidiClipMove(params) => {
                self.edited(application().move_midi_clips(params.moves)?)
            }
            CanonicalCommand::MidiClipTrim(params) => self.edited(application().trim_midi_clip(
                &params.clip_id,
                TimelineTick(params.start_tick),
                params.duration_ticks,
            )?),
            CanonicalCommand::MidiClipSplit(params) => {
                self.created(application().split_midi_clip_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.split_tick),
                )?)
            }
            CanonicalCommand::MidiClipDuplicate(params) => {
                self.created(application().duplicate_midi_clip_with_created_ids(&params.clip_id)?)
            }
            CanonicalCommand::MidiNoteAdd(params) => {
                self.created(application().add_midi_note_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.pitch,
                    params.duration_ticks,
                    params.velocity,
                    params.channel,
                )?)
            }
            CanonicalCommand::MidiNoteInsert(params) => self.created(
                application().insert_midi_notes_with_created_ids(&params.clip_id, params.notes)?,
            ),
            CanonicalCommand::MidiNoteUpdate(params) => {
                self.edited(application().update_midi_notes(
                    &params.clip_id,
                    vec![MidiNoteUpdate {
                        note_id: params.note_id,
                        patch: params.patch,
                    }],
                )?)
            }
            CanonicalCommand::MidiNoteUpdateMany(params) => {
                self.edited(application().update_midi_notes(&params.clip_id, params.updates)?)
            }
            CanonicalCommand::MidiNoteRemove(params) => {
                self.edited(application().remove_midi_note(&params.clip_id, &params.note_id)?)
            }
            CanonicalCommand::MidiNoteRemoveMany(params) => {
                self.edited(application().remove_midi_notes(&params.clip_id, params.note_ids)?)
            }
            CanonicalCommand::MidiNoteClear(params) => {
                self.edited(application().clear_midi_notes(&params.clip_id)?)
            }
            CanonicalCommand::MidiNoteQuantize(params) => {
                self.edited(application().quantize_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.grid_ticks,
                )?)
            }
            CanonicalCommand::MidiNoteTransform(params) => {
                self.edited(application().transform_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.transpose_semitones,
                    params.velocity_offset,
                )?)
            }
            CanonicalCommand::MidiNoteDuplicate(params) => {
                self.created(application().duplicate_midi_notes_with_created_ids(
                    &params.clip_id,
                    params.note_ids,
                    params.offset_ticks,
                )?)
            }
            CanonicalCommand::ClipRemove(params) => self.edited(
                application().remove_timeline_clips(params.audio_clip_ids, params.midi_clip_ids)?,
            ),
            CanonicalCommand::ClipPaste(params) => {
                self.created(application().paste_timeline_clips_with_created_ids(
                    params.audio_clip_ids,
                    params.midi_clip_ids,
                    TimelineTick(params.start_tick),
                )?)
            }

            CanonicalCommand::MusicMidiClipCreate(params) => {
                self.created(application().create_musical_midi_clip_with_created_ids(
                    &params.track_id,
                    params.start,
                    params.end,
                    params.name,
                )?)
            }
            CanonicalCommand::MusicMidiClipResize(params) => {
                if params.start.is_none() && params.end.is_none() {
                    return Err(DispatchError::invalid_request(
                        "MIDI clip resize requires a start or end",
                    ));
                }
                self.edited(application().resize_musical_midi_clip(
                    &params.clip_id,
                    params.start,
                    params.end,
                )?)
            }
            CanonicalCommand::MusicNoteInsert(params) => self.created(
                application()
                    .insert_musical_notes_with_created_ids(&params.clip_id, params.notes)?,
            ),
            CanonicalCommand::MusicNoteList(params) => Ok(ControlOutput::MusicNotes(
                application().list_musical_notes(MusicalNoteListRequest {
                    scope: MusicalNoteScope {
                        clip_id: params.clip_id,
                        track_id: params.track_id,
                    },
                    start: params.start,
                    end: params.end,
                    include_ids: params.include_ids,
                    raw: params.raw,
                })?,
            )),
            CanonicalCommand::MusicNoteGet(params) => Ok(ControlOutput::MusicNote(
                application().get_musical_note(&params.clip_id, &params.note_id)?,
            )),
            CanonicalCommand::MusicNoteUpdate(params) => {
                let patch = MusicalMidiNotePatch {
                    pitch: params.pitch,
                    position: params.position,
                    duration: params.duration,
                    velocity: params.velocity,
                    channel: params.channel,
                };
                if patch.pitch.is_none()
                    && patch.position.is_none()
                    && patch.duration.is_none()
                    && patch.velocity.is_none()
                    && patch.channel.is_none()
                {
                    return Err(DispatchError::invalid_request(
                        "at least one musical note field is required",
                    ));
                }
                self.edited(application().update_musical_note(
                    &params.clip_id,
                    &params.note_id,
                    patch,
                )?)
            }
            CanonicalCommand::MusicNoteRemove(params) => {
                self.edited(application().remove_musical_note(&params.clip_id, &params.note_id)?)
            }
            CanonicalCommand::MusicNoteTransform(params) => self.created(
                application().transform_musical_notes(MusicalNoteTransformRequest {
                    scope: MusicalNoteScope {
                        clip_id: params.clip_id,
                        track_id: params.track_id,
                    },
                    start: params.start,
                    end: params.end,
                    pitch: params.pitch,
                    channel: params.channel,
                    timing_offset: params.timing_offset,
                    velocity_offset: params.velocity_offset,
                    transpose_semitones: params.transpose_semitones,
                })?,
            ),
            CanonicalCommand::MusicHarmonyResolve(params) => Ok(ControlOutput::HarmonyChord(
                application().resolve_harmony_chord(&params.chord)?,
            )),
            CanonicalCommand::MusicHarmonyList(_) => Ok(ControlOutput::HarmonyEvents(
                application().list_harmony_events()?,
            )),
            CanonicalCommand::MusicHarmonyInsert(params) => {
                self.created(application().insert_harmony_events_with_created_ids(params.events)?)
            }
            CanonicalCommand::MusicHarmonyUpdate(params) => {
                self.edited(application().update_harmony_event(
                    &params.event_id,
                    HarmonyEventPatch {
                        start: params.start,
                        end: params.end,
                        chord: params.chord,
                        pitches: params.pitches,
                        root: params.root,
                        bass: params.bass,
                        label: params.label,
                    },
                )?)
            }
            CanonicalCommand::MusicHarmonyRemove(params) => {
                self.edited(application().remove_harmony_events(params.event_ids)?)
            }
            CanonicalCommand::MusicHarmonyRealize(params) => {
                self.created(application().realize_harmony_with_created_ids(
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
                )?)
            }
            CanonicalCommand::MusicPhraseInsert(params) => {
                self.created(application().insert_phrase_pattern_with_created_ids(
                    &params.clip_id,
                    params.pattern,
                    params.placements,
                    params.channel,
                )?)
            }
            CanonicalCommand::MusicPhrasePreview(params) => self.phrase_preview(timebase, params),
            CanonicalCommand::MusicRegionList(_) => {
                Ok(ControlOutput::Regions(application().list_regions()?))
            }
            CanonicalCommand::MusicRegionAdd(params) => self.created(
                application().add_region_with_created_ids(params.name, params.start, params.end)?,
            ),
            CanonicalCommand::MusicRegionUpdate(params) => {
                self.edited(application().update_region(
                    &params.region_id,
                    params.name,
                    params.start,
                    params.end,
                )?)
            }
            CanonicalCommand::MusicRegionRemove(params) => {
                self.edited(application().remove_region(&params.region_id)?)
            }

            CanonicalCommand::AssetImportMidi(params) => {
                Ok(ControlOutput::AssetId(riffra_host::import_midi_asset(
                    &self.data_root,
                    &params.path.to_string_lossy(),
                    params.name.as_deref(),
                )?))
            }
            CanonicalCommand::InstrumentList(_) => self.list_instruments(),
            CanonicalCommand::InstrumentSave(params) => self.save_instrument(params),
            CanonicalCommand::InstrumentExport(params) => self.export_instrument(params),
            CanonicalCommand::InstrumentApply(params) => self.apply_instrument(params),
            CanonicalCommand::InstrumentVst3Set(params) => self.set_vst3_instrument(params),
            CanonicalCommand::InstrumentClear(params) => {
                self.edited(application().set_track_instrument(&params.track_id, None)?)
            }
            CanonicalCommand::EffectAdd(params) => self.add_effect(params),
            CanonicalCommand::EffectRemove(params) => {
                self.edited(application().remove_track_effect(&params.track_id, &params.device_id)?)
            }
            CanonicalCommand::EffectReorder(params) => self
                .edited(application().reorder_track_effects(&params.track_id, params.device_ids)?),
            CanonicalCommand::DeviceBypass(params) => {
                self.edited(application().set_track_device_bypassed(
                    &params.track_id,
                    &params.device_id,
                    params.bypassed,
                )?)
            }
            CanonicalCommand::DeviceParameterSet(params) => {
                self.edited(application().set_track_device_parameter(
                    &params.track_id,
                    &params.device_id,
                    params.parameter_index as usize,
                    params.value,
                )?)
            }
            CanonicalCommand::MissingRelink(params) => self.relink_missing(params),
            CanonicalCommand::MissingDisablePlugin(params) => {
                self.edited(application().disable_missing_plugin(&params.device_id)?)
            }
            CanonicalCommand::MissingReplacePlugin(params) => self.replace_missing_plugin(params),
        }
    }
}
