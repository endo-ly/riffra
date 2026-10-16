//! Execution of canonical commands.

use super::{DispatchError, HostDispatcher};
use crate::api::output::TrackSummary;
use crate::api::{CanonicalCommand, ControlOutput};
use riffra_core::application::{
    ChordVoicingInput, HarmonyEventPatch, HarmonyRealizeSelection, MarkerPatch, MidiNoteUpdate,
    MusicalMidiNotePatch, MusicalNoteListRequest, MusicalNoteScope, MusicalNoteTransformRequest,
    SessionSettingsPatch, inspect_canonical_state,
};
use riffra_core::{
    AutomationInterpolation, AutomationLane, AutomationTarget, CanonicalState, CreativeSession,
    MidiInputRoute, TimelineTick, TrackPatch,
};

impl HostDispatcher<'_> {
    fn sidechain_capability(
        &self,
        owner: &riffra_core::EffectOwner,
        device_id: &str,
        plugin_path: &str,
    ) -> Result<crate::api::output::DeviceSidechainCapability, DispatchError> {
        if let Some(audio) = self.audio
            && let Ok(inspection) = audio.inspect_device(owner, device_id)
        {
            return Ok(inspection.capabilities.sidechain);
        }
        crate::plugins::validation::probe_sidechain_capability(
            &self.plugin_scan,
            std::path::Path::new(plugin_path),
        )
        .map_err(|error| {
            DispatchError::invalid_request(format!(
                "sidechain target '{device_id}' capability is unknown; verification failed: {error}"
            ))
        })
    }

    fn validate_sidechain_activation(
        &self,
        command: &CanonicalCommand,
        canonical: &CanonicalState,
    ) -> Result<(), DispatchError> {
        let Some(target_device_id) = sidechain_target_to_activate(command, canonical) else {
            return Ok(());
        };
        let arrangement = &canonical.session.arrangement;
        let Some(owner) = arrangement.device_owner(&target_device_id) else {
            return Ok(());
        };
        let Ok(riffra_core::DeviceRef::Effect(effect)) =
            arrangement.device(&owner, &target_device_id)
        else {
            return Ok(());
        };
        if effect.plugin.disabled_placeholder {
            return Err(DispatchError::invalid_request(format!(
                "sidechain target '{target_device_id}' is not an available effect"
            )));
        }
        #[cfg(test)]
        let capability = match self.test_sidechain_capability.clone() {
            Some(capability) => capability,
            None => self.sidechain_capability(&owner, &target_device_id, &effect.plugin.path)?,
        };
        #[cfg(not(test))]
        let capability =
            self.sidechain_capability(&owner, &target_device_id, &effect.plugin.path)?;
        if capability.mono || capability.stereo {
            return Ok(());
        }
        Err(DispatchError::invalid_request(format!(
            "sidechain target '{target_device_id}' does not support mono or stereo auxiliary input"
        )))
    }

    pub(super) fn run_canonical<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        command: CanonicalCommand,
        canonical: CanonicalState,
    ) -> Result<ControlOutput, DispatchError> {
        self.validate_sidechain_activation(&command, &canonical)?;
        let timebase = canonical.session.arrangement.timebase.clone();
        let tick = |position| {
            timebase
                .musical_position_to_tick(position)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))
        };
        match command {
            CanonicalCommand::MixerGet(_) => {
                Ok(ControlOutput::Mixer(canonical.session.arrangement.mixer))
            }
            CanonicalCommand::MixBusList(_) => Ok(ControlOutput::MixBuses(
                canonical.session.arrangement.mixer.buses,
            )),
            CanonicalCommand::SendList(_) => Ok(ControlOutput::Sends(
                canonical.session.arrangement.mixer.sends,
            )),
            CanonicalCommand::MixBusAdd(params) => self.created(
                application.add_mix_bus_with_created_ids(params.name, params.kind)?,
                application,
            ),
            CanonicalCommand::MixBusUpdate(params) => self.edited(
                application.update_mix_bus(&params.bus_id, params.patch)?,
                application,
            ),
            CanonicalCommand::MixBusRemove(params) => {
                self.edited(application.remove_mix_bus(&params.bus_id)?, application)
            }
            CanonicalCommand::TrackOutputSet(params) => self.edited(
                application.set_track_output(&params.track_id, params.bus_id)?,
                application,
            ),
            CanonicalCommand::SendAdd(params) => self.created(
                application.add_send_with_created_ids(
                    params.track_id,
                    params.return_bus_id,
                    params.level_db,
                    params.position,
                )?,
                application,
            ),
            CanonicalCommand::SendUpdate(params) => self.edited(
                application.update_send(&params.send_id, params.patch)?,
                application,
            ),
            CanonicalCommand::SendRemove(params) => {
                self.edited(application.remove_send(&params.send_id)?, application)
            }
            CanonicalCommand::SidechainSendList(_) => Ok(ControlOutput::SidechainSends(
                canonical.session.arrangement.mixer.sidechain_sends,
            )),
            CanonicalCommand::SidechainSendAdd(params) => self.created(
                application.add_sidechain_send_with_created_ids(
                    params.source_track_id,
                    params.target_device_id,
                    params.level_db,
                    params.position,
                )?,
                application,
            ),
            CanonicalCommand::SidechainSendUpdate(params) => self.edited(
                application.update_sidechain_send(&params.sidechain_send_id, params.patch)?,
                application,
            ),
            CanonicalCommand::SidechainSendRemove(params) => self.edited(
                application.remove_sidechain_send(&params.sidechain_send_id)?,
                application,
            ),
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
            CanonicalCommand::AutomationGet(params) => Ok(ControlOutput::AutomationLanes(
                automation_lane(&canonical.session, params.target)?,
            )),
            CanonicalCommand::AutomationList(params) => Ok(ControlOutput::AutomationLanes(
                automation_lanes(&canonical.session, params.owner)?,
            )),
            CanonicalCommand::AutomationSet(params) => self.created(
                application.set_automation_with_created_ids(
                    params.target.clone(),
                    params.interpolation,
                    params
                        .points
                        .into_iter()
                        .map(|point| {
                            let value = match point.value {
                                crate::api::params::AutomationPointValue::Number(value) => value,
                                crate::api::params::AutomationPointValue::Display(display) => {
                                    let AutomationTarget::DeviceParameter {
                                        device_id,
                                        parameter_id,
                                    } = &params.target
                                    else {
                                        return Err(DispatchError::invalid_request(
                                            "display values require a device parameter target",
                                        ));
                                    };
                                    let owner = canonical
                                        .session
                                        .arrangement
                                        .device_owner(device_id)
                                        .ok_or_else(|| {
                                            DispatchError::invalid_request(
                                                "unknown automation device",
                                            )
                                        })?;
                                    self.audio
                                        .ok_or_else(DispatchError::requires_live_host)?
                                        .device_parameter_value_for_text(
                                            &owner,
                                            device_id,
                                            parameter_id,
                                            &display,
                                        )
                                        .map_err(|error| {
                                            DispatchError::CommandFailed(error.to_string())
                                        })?
                                }
                            };
                            Ok(riffra_core::AutomationPoint {
                                id: point.id,
                                tick: TimelineTick(point.tick),
                                value,
                            })
                        })
                        .collect::<Result<Vec<_>, DispatchError>>()?,
                )?,
                application,
            ),
            CanonicalCommand::AutomationClear(params) => self.created(
                application.set_automation_with_created_ids(
                    params.target,
                    AutomationInterpolation::Linear,
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
                application.remove_effect(&params.owner, &params.device_id)?,
                application,
            ),
            CanonicalCommand::EffectReorder(params) => self.edited(
                application.reorder_effects(&params.owner, params.device_ids)?,
                application,
            ),
            CanonicalCommand::DeviceBypass(params) => self.edited(
                application.set_device_bypassed(
                    &params.owner,
                    &params.device_id,
                    params.bypassed,
                )?,
                application,
            ),
            CanonicalCommand::DeviceParameterSet(params) => self.edited(
                application.set_device_parameter(
                    &params.owner,
                    &params.device_id,
                    params.parameter_index as usize,
                    match (params.value, params.display) {
                        (Some(value), None) => value,
                        (None, Some(_)) => {
                            return Err(DispatchError::CommandFailed(
                                "display parameter input requires a live Host".into(),
                            ));
                        }
                        _ => {
                            return Err(DispatchError::CommandFailed(
                                "select exactly one of value or display".into(),
                            ));
                        }
                    },
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

fn sidechain_target_to_activate(
    command: &CanonicalCommand,
    canonical: &CanonicalState,
) -> Option<String> {
    match command {
        CanonicalCommand::SidechainSendAdd(params) => Some(params.target_device_id.clone()),
        CanonicalCommand::SidechainSendUpdate(params) => canonical
            .session
            .arrangement
            .mixer
            .sidechain_sends
            .iter()
            .find(|send| send.id == params.sidechain_send_id)
            .and_then(|send| {
                let target = params
                    .patch
                    .target_device_id
                    .as_deref()
                    .unwrap_or(&send.target_device_id);
                let enabled = params.patch.enabled.unwrap_or(send.enabled);
                (enabled && (!send.enabled || target != send.target_device_id))
                    .then(|| target.to_owned())
            }),
        _ => None,
    }
}

fn automation_lane(
    session: &CreativeSession,
    target: AutomationTarget,
) -> Result<Vec<AutomationLane>, DispatchError> {
    if !session.arrangement.has_automation_target(&target) {
        return Err(DispatchError::invalid_request("unknown automation target"));
    }
    Ok(session
        .arrangement
        .automation_lanes
        .iter()
        .filter(|lane| lane.target == target)
        .cloned()
        .collect())
}

fn automation_lanes(
    session: &CreativeSession,
    owner: Option<riffra_core::EffectOwner>,
) -> Result<Vec<AutomationLane>, DispatchError> {
    use riffra_core::EffectOwner;
    let arrangement = &session.arrangement;
    match &owner {
        Some(EffectOwner::Track { id })
            if !arrangement.tracks.iter().any(|track| track.id == *id) =>
        {
            return Err(DispatchError::invalid_request("unknown track"));
        }
        Some(EffectOwner::Bus { id })
            if !arrangement.mixer.buses.iter().any(|bus| bus.id == *id) =>
        {
            return Err(DispatchError::invalid_request("unknown bus"));
        }
        _ => {}
    }
    let mut lanes = arrangement
        .automation_lanes
        .iter()
        .filter(|lane| match (&owner, &lane.target) {
            (None, _) => true,
            (Some(EffectOwner::Track { id }), AutomationTarget::TrackMix { track_id, .. }) => {
                id == track_id
            }
            (Some(EffectOwner::Bus { id }), AutomationTarget::BusMix { bus_id, .. }) => {
                id == bus_id
            }
            (Some(EffectOwner::Master), AutomationTarget::MasterGain) => true,
            (Some(EffectOwner::Track { id }), AutomationTarget::SendLevel { send_id }) => {
                arrangement
                    .mixer
                    .sends
                    .iter()
                    .any(|send| send.id == *send_id && send.track_id == *id)
            }
            (
                Some(EffectOwner::Track { id }),
                AutomationTarget::SidechainSendLevel { sidechain_send_id },
            ) => arrangement
                .mixer
                .sidechain_sends
                .iter()
                .any(|send| send.id == *sidechain_send_id && send.source_track_id == *id),
            (Some(owner), AutomationTarget::DeviceParameter { device_id, .. }) => {
                arrangement.device(owner, device_id).is_ok()
            }
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    lanes.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(lanes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        AutomationPoint, EffectOwner, SendPosition, SidechainSend, SidechainSendPatch,
        TimelineTick, Track, TrackMixParameter,
    };

    #[test]
    fn capability_validation_only_runs_when_a_sidechain_route_becomes_active() {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:source".into(), "Source".into()));
        session
            .arrangement
            .mixer
            .sidechain_sends
            .push(SidechainSend {
                id: "sidechain:1".into(),
                source_track_id: "track:source".into(),
                target_device_id: "device:first".into(),
                level_db: -6.0,
                position: SendPosition::PostFader,
                enabled: false,
            });
        let canonical =
            riffra_core::AppCore::new("project:test".into(), session, 0).canonical_state();

        let add = CanonicalCommand::SidechainSendAdd(crate::api::params::SidechainSendAddParams {
            source_track_id: "track:source".into(),
            target_device_id: "device:first".into(),
            level_db: -6.0,
            position: SendPosition::PostFader,
        });
        let activate =
            CanonicalCommand::SidechainSendUpdate(crate::api::params::SidechainSendUpdateParams {
                sidechain_send_id: "sidechain:1".into(),
                patch: SidechainSendPatch {
                    enabled: Some(true),
                    ..Default::default()
                },
            });
        let level_change =
            CanonicalCommand::SidechainSendUpdate(crate::api::params::SidechainSendUpdateParams {
                sidechain_send_id: "sidechain:1".into(),
                patch: SidechainSendPatch {
                    level_db: Some(-12.0),
                    ..Default::default()
                },
            });
        let retarget =
            CanonicalCommand::SidechainSendUpdate(crate::api::params::SidechainSendUpdateParams {
                sidechain_send_id: "sidechain:1".into(),
                patch: SidechainSendPatch {
                    target_device_id: Some("device:second".into()),
                    enabled: Some(true),
                    ..Default::default()
                },
            });

        assert_eq!(
            sidechain_target_to_activate(&add, &canonical).as_deref(),
            Some("device:first")
        );
        assert_eq!(
            sidechain_target_to_activate(&activate, &canonical).as_deref(),
            Some("device:first")
        );
        assert_eq!(
            sidechain_target_to_activate(&level_change, &canonical),
            None
        );
        assert_eq!(
            sidechain_target_to_activate(&retarget, &canonical).as_deref(),
            Some("device:second")
        );
    }

    #[test]
    fn automation_read_resolves_targets_and_lists_channel_ownership() {
        // Arrange
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:1".into(), "Audio".into()));
        session
            .arrangement
            .mixer
            .sidechain_sends
            .push(SidechainSend {
                id: "sidechain:1".into(),
                source_track_id: "track:1".into(),
                target_device_id: "fx:target".into(),
                level_db: -6.0,
                position: SendPosition::PostFader,
                enabled: true,
            });
        let pan = AutomationTarget::TrackMix {
            track_id: "track:1".into(),
            parameter: TrackMixParameter::Pan,
        };
        session.arrangement.automation_lanes = vec![
            AutomationLane {
                id: "pan".into(),
                target: pan.clone(),
                interpolation: AutomationInterpolation::Step,
                points: vec![AutomationPoint {
                    id: "pan-point".into(),
                    tick: TimelineTick(20),
                    value: 0.5,
                }],
            },
            AutomationLane {
                id: "master".into(),
                target: AutomationTarget::MasterGain,
                interpolation: AutomationInterpolation::Linear,
                points: vec![AutomationPoint {
                    id: "master-point".into(),
                    tick: TimelineTick(0),
                    value: -6.0,
                }],
            },
            AutomationLane {
                id: "sidechain".into(),
                target: AutomationTarget::SidechainSendLevel {
                    sidechain_send_id: "sidechain:1".into(),
                },
                interpolation: AutomationInterpolation::Linear,
                points: vec![AutomationPoint {
                    id: "sidechain-point".into(),
                    tick: TimelineTick(0),
                    value: 0.0,
                }],
            },
        ];

        // Act
        let all = automation_lanes(&session, None).unwrap();
        let track = automation_lanes(
            &session,
            Some(EffectOwner::Track {
                id: "track:1".into(),
            }),
        )
        .unwrap();
        let lane = automation_lane(&session, pan).unwrap();

        // Assert
        assert_eq!(
            all.iter().map(|lane| lane.id.as_str()).collect::<Vec<_>>(),
            ["master", "pan", "sidechain"]
        );
        assert_eq!(lane.len(), 1);
        assert_eq!(lane[0].id, "pan");
        assert_eq!(track.len(), 2);
        assert_eq!(track[0].id, "pan");
        assert_eq!(track[1].id, "sidechain");
        assert!(
            automation_lane(
                &session,
                AutomationTarget::TrackMix {
                    track_id: "missing".into(),
                    parameter: TrackMixParameter::Volume
                }
            )
            .is_err()
        );
        assert!(
            automation_lanes(
                &session,
                Some(EffectOwner::Bus {
                    id: "missing".into()
                })
            )
            .is_err()
        );
    }
}
