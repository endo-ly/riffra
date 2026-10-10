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

impl HostDispatcher<'_> {
    pub(super) fn run_canonical<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        command: CanonicalCommand,
        canonical: CanonicalState,
    ) -> Result<ControlOutput, DispatchError> {
        let timebase = canonical.session.arrangement.timebase.clone();
        let tick = |position| {
            timebase
                .musical_position_to_tick(position)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))
        };
        match command {
            CanonicalCommand::MixdownGet(_) => {
                Ok(ControlOutput::Mixdown(canonical.session.settings.mixdown))
            }
            CanonicalCommand::MixdownSet(mixdown) => self.edited(
                application.update_session_settings(SessionSettingsPatch {
                    mixdown: Some(mixdown),
                    ..Default::default()
                })?,
                application,
            ),
            CanonicalCommand::TrackExternalAudioInputSet(params) => self.edited(
                application.update_track(
                    &params.track_id,
                    TrackPatch {
                        external_audio_source_track_id: Some(Some(params.source_track_id)),
                        ..Default::default()
                    },
                )?,
                application,
            ),
            CanonicalCommand::TrackExternalAudioInputClear(params) => self.edited(
                application.update_track(
                    &params.track_id,
                    TrackPatch {
                        external_audio_source_track_id: Some(None),
                        ..Default::default()
                    },
                )?,
                application,
            ),
            CanonicalCommand::InstrumentEventList(params) => {
                let clip = canonical
                    .session
                    .arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.id == params.clip_id)
                    .ok_or_else(|| DispatchError::invalid_request("midi clip was not found"))?;
                Ok(ControlOutput::InstrumentEvents(
                    clip.instrument_control_events.clone(),
                ))
            }
            command @ (CanonicalCommand::InstrumentEventAdd(_)
            | CanonicalCommand::InstrumentEventSet(_)
            | CanonicalCommand::InstrumentEventUpdate(_)
            | CanonicalCommand::InstrumentEventRemove(_)) => {
                let clip_id = match &command {
                    CanonicalCommand::InstrumentEventAdd(params) => &params.clip_id,
                    CanonicalCommand::InstrumentEventSet(params) => &params.clip_id,
                    CanonicalCommand::InstrumentEventUpdate(params) => &params.clip_id,
                    CanonicalCommand::InstrumentEventRemove(params) => &params.clip_id,
                    _ => unreachable!(),
                }
                .clone();
                let mut events = canonical
                    .session
                    .arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .ok_or_else(|| DispatchError::invalid_request("midi clip was not found"))?
                    .instrument_control_events
                    .clone();
                match command {
                    CanonicalCommand::InstrumentEventAdd(params) => {
                        let source_order = events
                            .iter()
                            .map(|event| event.source_order)
                            .max()
                            .map(|order| order.checked_add(1))
                            .unwrap_or(Some(0))
                            .ok_or_else(|| {
                                DispatchError::invalid_request("instrument event order overflow")
                            })?;
                        events.push(riffra_core::InstrumentControlEvent {
                            id: riffra_control::new_instance_id(),
                            tick: TimelineTick(params.tick),
                            source_order,
                            kind: params.kind,
                        });
                    }
                    CanonicalCommand::InstrumentEventSet(params) => events = params.events,
                    CanonicalCommand::InstrumentEventUpdate(params) => {
                        let event = events
                            .iter_mut()
                            .find(|event| event.id == params.event.id)
                            .ok_or_else(|| {
                                DispatchError::invalid_request("instrument event was not found")
                            })?;
                        *event = params.event;
                    }
                    CanonicalCommand::InstrumentEventRemove(params) => {
                        let index = events
                            .iter()
                            .position(|event| event.id == params.event_id)
                            .ok_or_else(|| {
                                DispatchError::invalid_request("instrument event was not found")
                            })?;
                        events.remove(index);
                    }
                    _ => unreachable!(),
                }
                self.edited(
                    application.update_midi_clip(
                        &clip_id,
                        riffra_core::MidiClipPatch {
                            instrument_control_events: Some(events),
                            ..Default::default()
                        },
                    )?,
                    application,
                )
            }
            CanonicalCommand::SessionGet(_) => Ok(ControlOutput::Session(canonical.session)),
            CanonicalCommand::SessionInspect(query) => Ok(ControlOutput::SessionInspection(
                inspect_canonical_state(&canonical, query)
                    .map_err(|error| DispatchError::invalid_request(error.to_string()))?,
            )),
            CanonicalCommand::SessionApply(params) => {
                self.apply_batch(application, canonical, params)
            }
            CanonicalCommand::SessionSettingsUpdate(patch) => {
                self.edited(application.update_session_settings(patch)?, application)
            }
            CanonicalCommand::HistoryGet(_) => Ok(ControlOutput::History(canonical.history)),
            CanonicalCommand::Undo(_) => self.edited(application.undo()?, application),
            CanonicalCommand::Redo(_) => self.edited(application.redo()?, application),
            CanonicalCommand::MasterGainSet(params) => {
                if !params.gain_db.is_finite() {
                    return Err(DispatchError::invalid_request("master gain must be finite"));
                }
                self.edited(
                    application.update_session_settings(SessionSettingsPatch {
                        master_db: Some(params.gain_db),
                        ..SessionSettingsPatch::default()
                    })?,
                    application,
                )
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
            CanonicalCommand::TrackAdd(params) => self.created(
                application.add_track_with_created_ids(params.name, params.kind)?,
                application,
            ),
            CanonicalCommand::TrackUpdate(params) => self.edited(
                application.update_track(
                    &params.track_id,
                    TrackPatch {
                        pan_law: params.pan_law,
                        external_audio_source_track_id: None,
                        name: params.name,
                        gain_db: params.gain_db,
                        pan: params.pan,
                        muted: params.muted,
                        solo: params.solo,
                        armed: params.armed,
                        monitoring: params.monitoring,
                        color: params.color,
                    },
                )?,
                application,
            ),
            CanonicalCommand::TrackRemove(params) => {
                self.edited(application.remove_track(&params.track_id)?, application)
            }
            CanonicalCommand::TrackDuplicate(params) => self.created(
                application.duplicate_track_with_created_ids(&params.track_id)?,
                application,
            ),
            CanonicalCommand::TrackReorder(params) => self.edited(
                application.reorder_track(&params.track_id, params.target_index)?,
                application,
            ),
            CanonicalCommand::TrackAudioInputSet(params) => self.edited(
                application.set_track_audio_input(&params.track_id, Some(params.channel_index))?,
                application,
            ),
            CanonicalCommand::TrackAudioInputClear(params) => self.edited(
                application.set_track_audio_input(&params.track_id, None)?,
                application,
            ),
            CanonicalCommand::TrackMidiInputSet(params) => self.edited(
                application.set_track_midi_input(
                    &params.track_id,
                    MidiInputRoute {
                        device_id: params.device_id,
                        channel: params.channel,
                    },
                )?,
                application,
            ),
            CanonicalCommand::TrackMidiInputClear(params) => self.edited(
                application.set_track_midi_input(&params.track_id, MidiInputRoute::default())?,
                application,
            ),
            CanonicalCommand::MarkerAdd(params) => self.created(
                application.add_marker_with_created_ids(tick(params.position)?, params.name)?,
                application,
            ),
            CanonicalCommand::MarkerUpdate(params) => self.edited(
                application.update_marker(
                    &params.marker_id,
                    MarkerPatch {
                        name: params.name,
                        tick: params.position.map(tick).transpose()?,
                    },
                )?,
                application,
            ),
            CanonicalCommand::MarkerRemove(params) => {
                self.edited(application.remove_marker(&params.marker_id)?, application)
            }
            CanonicalCommand::TimebaseUpdate(params) => self.edited(
                application.update_timebase(self.timebase_update(timebase, params)?)?,
                application,
            ),
            CanonicalCommand::TimebaseGetMap(_) => Ok(ControlOutput::Timebase(timebase)),
            CanonicalCommand::TimebaseSetMap(params) => self.edited(
                application.update_timebase(riffra_core::ProjectTimebase {
                    ppq: riffra_core::TIMELINE_PPQ,
                    tempo_changes: params.tempo_changes,
                    time_signature_changes: params.time_signature_changes,
                })?,
                application,
            ),
            CanonicalCommand::LoopRangeSet(params) => self.edited(
                application.update_loop_range(
                    params.enabled,
                    tick(params.start)?,
                    tick(params.end)?,
                )?,
                application,
            ),
            CanonicalCommand::PunchRangeSet(params) => self.edited(
                application.update_punch_range(
                    params.enabled,
                    tick(params.start)?,
                    tick(params.end)?,
                )?,
                application,
            ),
            CanonicalCommand::AutomationSet(params) => self.created(
                application.set_track_automation_with_created_ids(
                    &params.track_id,
                    params.parameter,
                    params.points,
                )?,
                application,
            ),
            CanonicalCommand::AutomationClear(params) => self.created(
                application.set_track_automation_with_created_ids(
                    &params.track_id,
                    params.parameter,
                    Vec::new(),
                )?,
                application,
            ),

            CanonicalCommand::AudioClipList(_) => Ok(ControlOutput::AudioClips(
                canonical.session.arrangement.audio_clips,
            )),
            CanonicalCommand::AudioClipAddAsset(params) => self.add_audio_clip(application, params),
            CanonicalCommand::AudioClipUpdate(params) => self.edited(
                application.update_audio_clip(&params.clip_id, params.patch)?,
                application,
            ),
            CanonicalCommand::AudioClipMove(params) => {
                self.edited(application.move_audio_clips(params.moves)?, application)
            }
            CanonicalCommand::AudioClipTrim(params) => {
                let source_frames = self.audio_source_frames(application, &params.clip_id)?;
                self.edited(
                    application.trim_audio_clip(
                        &params.clip_id,
                        TimelineTick(params.start_tick),
                        params.source_range,
                        source_frames,
                    )?,
                    application,
                )
            }
            CanonicalCommand::AudioClipSplit(params) => self.created(
                application.split_audio_clip_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.split_tick),
                )?,
                application,
            ),
            CanonicalCommand::AudioClipDuplicate(params) => self.created(
                application.duplicate_audio_clip_with_created_ids(&params.clip_id)?,
                application,
            ),
            CanonicalCommand::AudioClipCrossfade(params) => self.edited(
                application.crossfade_audio_clips(&params.first_clip_id, &params.second_clip_id)?,
                application,
            ),
            CanonicalCommand::MidiClipList(_) => Ok(ControlOutput::MidiClips(
                canonical.session.arrangement.midi_clips,
            )),
            CanonicalCommand::MidiClipCreate(params) => self.created(
                application.create_midi_clip_with_created_ids(
                    &params.track_id,
                    TimelineTick(params.start_tick),
                    params.duration_ticks,
                    params.name,
                )?,
                application,
            ),
            CanonicalCommand::MidiClipAddAsset(params) => self.add_midi_clip(application, params),
            CanonicalCommand::MidiClipUpdate(params) => self.edited(
                application.update_midi_clip(&params.clip_id, params.patch)?,
                application,
            ),
            CanonicalCommand::MidiClipMove(params) => {
                self.edited(application.move_midi_clips(params.moves)?, application)
            }
            CanonicalCommand::MidiClipTrim(params) => self.edited(
                application.trim_midi_clip(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.duration_ticks,
                )?,
                application,
            ),
            CanonicalCommand::MidiClipSplit(params) => self.created(
                application.split_midi_clip_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.split_tick),
                )?,
                application,
            ),
            CanonicalCommand::MidiClipDuplicate(params) => self.created(
                application.duplicate_midi_clip_with_created_ids(&params.clip_id)?,
                application,
            ),
            CanonicalCommand::MidiNoteAdd(params) => self.created(
                application.add_midi_note_with_created_ids(
                    &params.clip_id,
                    TimelineTick(params.start_tick),
                    params.pitch,
                    params.duration_ticks,
                    params.velocity,
                    params.channel,
                )?,
                application,
            ),
            CanonicalCommand::MidiNoteInsert(params) => self.created(
                application.insert_midi_notes_with_created_ids(&params.clip_id, params.notes)?,
                application,
            ),
            CanonicalCommand::MidiNoteUpdate(params) => self.edited(
                application.update_midi_notes(
                    &params.clip_id,
                    vec![MidiNoteUpdate {
                        note_id: params.note_id,
                        patch: params.patch,
                    }],
                )?,
                application,
            ),
            CanonicalCommand::MidiNoteUpdateMany(params) => self.edited(
                application.update_midi_notes(&params.clip_id, params.updates)?,
                application,
            ),
            CanonicalCommand::MidiNoteRemove(params) => self.edited(
                application.remove_midi_note(&params.clip_id, &params.note_id)?,
                application,
            ),
            CanonicalCommand::MidiNoteRemoveMany(params) => self.edited(
                application.remove_midi_notes(&params.clip_id, params.note_ids)?,
                application,
            ),
            CanonicalCommand::MidiNoteClear(params) => {
                self.edited(application.clear_midi_notes(&params.clip_id)?, application)
            }
            CanonicalCommand::MidiNoteQuantize(params) => self.edited(
                application.quantize_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.grid_ticks,
                )?,
                application,
            ),
            CanonicalCommand::MidiNoteTransform(params) => self.edited(
                application.transform_midi_notes(
                    &params.clip_id,
                    params.note_ids,
                    params.transpose_semitones,
                    params.velocity_offset,
                )?,
                application,
            ),
            CanonicalCommand::MidiNoteDuplicate(params) => self.created(
                application.duplicate_midi_notes_with_created_ids(
                    &params.clip_id,
                    params.note_ids,
                    params.offset_ticks,
                )?,
                application,
            ),
            CanonicalCommand::ClipRemove(params) => self.edited(
                application.remove_timeline_clips(params.audio_clip_ids, params.midi_clip_ids)?,
                application,
            ),
            CanonicalCommand::ClipPaste(params) => self.created(
                application.paste_timeline_clips_with_created_ids(
                    params.audio_clip_ids,
                    params.midi_clip_ids,
                    TimelineTick(params.start_tick),
                )?,
                application,
            ),

            CanonicalCommand::MusicMidiClipCreate(params) => self.created(
                application.create_musical_midi_clip_with_created_ids(
                    &params.track_id,
                    params.start,
                    params.end,
                    params.name,
                )?,
                application,
            ),
            CanonicalCommand::MusicMidiClipResize(params) => {
                if params.start.is_none() && params.end.is_none() {
                    return Err(DispatchError::invalid_request(
                        "MIDI clip resize requires a start or end",
                    ));
                }
                self.edited(
                    application.resize_musical_midi_clip(
                        &params.clip_id,
                        params.start,
                        params.end,
                    )?,
                    application,
                )
            }
            CanonicalCommand::MusicNoteInsert(params) => self.created(
                application.insert_musical_notes_with_created_ids(&params.clip_id, params.notes)?,
                application,
            ),
            CanonicalCommand::MusicNoteList(params) => Ok(ControlOutput::MusicNotes(
                application.list_musical_notes(MusicalNoteListRequest {
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
                application.get_musical_note(&params.clip_id, &params.note_id)?,
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
                self.edited(
                    application.update_musical_note(&params.clip_id, &params.note_id, patch)?,
                    application,
                )
            }
            CanonicalCommand::MusicNoteRemove(params) => self.edited(
                application.remove_musical_note(&params.clip_id, &params.note_id)?,
                application,
            ),
            CanonicalCommand::MusicNoteTransform(params) => self.created(
                application.transform_musical_notes(MusicalNoteTransformRequest {
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
                application,
            ),
            CanonicalCommand::MusicHarmonyResolve(params) => Ok(ControlOutput::HarmonyChord(
                application.resolve_harmony_chord(&params.chord)?,
            )),
            CanonicalCommand::MusicHarmonyList(_) => Ok(ControlOutput::HarmonyEvents(
                application.list_harmony_events()?,
            )),
            CanonicalCommand::MusicHarmonyInsert(params) => self.created(
                application.insert_harmony_events_with_created_ids(params.events)?,
                application,
            ),
            CanonicalCommand::MusicHarmonyUpdate(params) => self.edited(
                application.update_harmony_event(
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
                )?,
                application,
            ),
            CanonicalCommand::MusicHarmonyRemove(params) => self.edited(
                application.remove_harmony_events(params.event_ids)?,
                application,
            ),
            CanonicalCommand::MusicHarmonyRealize(params) => self.created(
                application.realize_harmony_with_created_ids(
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
                application,
            ),
            CanonicalCommand::MusicPhraseInsert(params) => self.created(
                application.insert_phrase_pattern_with_created_ids(
                    &params.clip_id,
                    params.pattern,
                    params.placements,
                    params.channel,
                )?,
                application,
            ),
            CanonicalCommand::MusicPhrasePreview(params) => {
                self.phrase_preview(application, timebase, params)
            }
            CanonicalCommand::MusicRegionList(_) => {
                Ok(ControlOutput::Regions(application.list_regions()?))
            }
            CanonicalCommand::MusicRegionAdd(params) => self.created(
                application.add_region_with_created_ids(params.name, params.start, params.end)?,
                application,
            ),
            CanonicalCommand::MusicRegionUpdate(params) => self.edited(
                application.update_region(
                    &params.region_id,
                    params.name,
                    params.start,
                    params.end,
                )?,
                application,
            ),
            CanonicalCommand::MusicRegionRemove(params) => {
                self.edited(application.remove_region(&params.region_id)?, application)
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
            CanonicalCommand::InstrumentApply(params) => self.apply_instrument(application, params),
            CanonicalCommand::InstrumentVst3Set(params) => {
                self.set_vst3_instrument(application, params)
            }
            CanonicalCommand::InstrumentClear(params) => self.edited(
                application.set_track_instrument(&params.track_id, None)?,
                application,
            ),
            CanonicalCommand::EffectAdd(params) => self.add_effect(application, params),
            CanonicalCommand::EffectRemove(params) => self.edited(
                application.remove_track_effect(&params.track_id, &params.device_id)?,
                application,
            ),
            CanonicalCommand::EffectReorder(params) => self.edited(
                application.reorder_track_effects(&params.track_id, params.device_ids)?,
                application,
            ),
            CanonicalCommand::DeviceBypass(params) => self.edited(
                application.set_track_device_bypassed(
                    &params.track_id,
                    &params.device_id,
                    params.bypassed,
                )?,
                application,
            ),
            CanonicalCommand::DeviceParameterSet(params) => self.edited(
                application.set_track_device_parameter(
                    &params.track_id,
                    &params.device_id,
                    params.parameter_index as usize,
                    params.value,
                )?,
                application,
            ),
            CanonicalCommand::MissingRelink(params) => self.relink_missing(application, params),
            CanonicalCommand::MissingDisablePlugin(params) => self.edited(
                application.disable_missing_plugin(&params.device_id)?,
                application,
            ),
            CanonicalCommand::MissingReplacePlugin(params) => {
                self.replace_missing_plugin(application, params)
            }
        }
    }
}
