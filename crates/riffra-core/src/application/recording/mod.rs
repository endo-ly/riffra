//! Recording completion and take application operations.

use super::*;

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Merges the production fields owned by a completed recording onto the
    /// latest canonical snapshot.
    pub fn commit_recording(
        &self,
        base: &CreativeSession,
        candidate: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core
            .commit_merged(self.storage, base, candidate, merge_recording_session)
    }

    /// Selects the raw or processed source for an Audio Clip backed by a Take.
    pub fn set_audio_clip_take_variant(
        &self,
        clip_id: &str,
        variant: AudioTakeVariant,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            apply_audio_clip_take_variant(session, clip_id, variant)
                .map_err(ApplicationError::InvalidCommand)
        })
    }

    /// Activates a recorded Take in its recording Session slot.
    ///
    /// MIDI take decoding remains host-specific; a decoded clip is supplied
    /// when the Take is MIDI-backed. Audio source selection and all canonical
    /// slot/clip updates stay in Core.
    pub fn activate_take(
        &self,
        session_id: &str,
        take_id: &str,
        midi_clip: Option<MidiClip>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let target_take = arrangement
                .takes
                .iter()
                .find(|take| take.session_id == session_id && take.id == take_id)
                .cloned()
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "recording take is not registered: {take_id}"
                    ))
                })?;
            let timeline_clip_id = {
                let slot = arrangement
                    .recording_sessions
                    .iter_mut()
                    .find(|recording| recording.id == session_id)
                    .and_then(|recording| {
                        recording
                            .track_slots
                            .iter_mut()
                            .find(|slot| slot.track_id == target_take.track_id)
                    })
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(format!(
                            "recording session has no track slot for {}",
                            target_take.track_id
                        ))
                    })?;
                slot.active_take_id = take_id.to_owned();
                slot.timeline_clip_id.clone()
            };
            if let Some(clip) = arrangement
                .audio_clips
                .iter_mut()
                .find(|clip| clip.id == timeline_clip_id)
            {
                let source = target_take
                    .preferred_audio_source(clip.take_variant)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "the selected take has no audio asset".into(),
                        )
                    })?;
                apply_audio_source_to_clip(clip, &source);
                clip.recording_take_id = Some(take_id.to_owned());
            } else if target_take.midi_asset_id.is_some() {
                let source = midi_clip.ok_or_else(|| {
                    ApplicationError::InvalidCommand(
                        "the selected MIDI take has no decoded clip".into(),
                    )
                })?;
                let clip = arrangement
                    .midi_clips
                    .iter_mut()
                    .find(|clip| clip.id == timeline_clip_id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take slot has no MIDI clip".into(),
                        )
                    })?;
                clip.asset_id = target_take.midi_asset_id.clone();
                clip.notes = source.notes;
                clip.events = source.events;
                clip.duration_ticks = target_take.duration_ticks;
                clip.recording_take_id = Some(take_id.to_owned());
            } else {
                return Err(ApplicationError::InvalidCommand(
                    "recording take has no timeline source".into(),
                ));
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Places a recorded Take as a new timeline clip.
    ///
    /// The host may provide a decoded MIDI clip because reading the source
    /// asset is an infrastructure concern. Core assigns the new clip identity
    /// and owns the arrangement mutation.
    pub fn place_take_as_separate_clip(
        &self,
        take_id: &str,
        mut midi_clip: Option<MidiClip>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let take = arrangement
                .takes
                .iter()
                .find(|take| take.id == take_id)
                .cloned()
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "recording take is not registered: {take_id}"
                    ))
                })?;
            if let Some(source) = arrangement
                .audio_clips
                .iter()
                .find(|clip| clip.recording_take_id.as_deref() == Some(take_id))
                .cloned()
            {
                let mut clip = source;
                clip.id = next_id("clip:take-place");
                clip.muted = false;
                arrangement.audio_clips.push(clip);
            } else if take.raw_audio.is_some() || take.processed_audio.is_some() {
                let slot_clip_id = arrangement
                    .recording_sessions
                    .iter()
                    .find(|recording| recording.id == take.session_id)
                    .and_then(|recording| {
                        recording
                            .track_slots
                            .iter()
                            .find(|slot| slot.track_id == take.track_id)
                    })
                    .map(|slot| slot.timeline_clip_id.clone())
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take track slot is unavailable".into(),
                        )
                    })?;
                let mut clip = arrangement
                    .audio_clips
                    .iter()
                    .find(|clip| clip.id == slot_clip_id)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take slot has no audio clip".into(),
                        )
                    })?;
                clip.id = next_id("clip:take-place");
                clip.start_tick = take.start_tick;
                let source = take
                    .preferred_audio_source(clip.take_variant)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take has no usable audio asset".into(),
                        )
                    })?;
                apply_audio_source_to_clip(&mut clip, &source);
                clip.recording_take_id = Some(take.id);
                clip.muted = false;
                arrangement.audio_clips.push(clip);
            } else if take.midi_asset_id.is_some() {
                let mut clip = midi_clip.take().ok_or_else(|| {
                    ApplicationError::InvalidCommand(
                        "the selected MIDI take has no decoded clip".into(),
                    )
                })?;
                clip.id = next_id("midi-clip:take-place");
                clip.recording_take_id = Some(take.id);
                arrangement.midi_clips.push(clip);
            } else {
                return Err(ApplicationError::InvalidCommand(format!(
                    "recording take has no timeline clip: {take_id}"
                )));
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Repoints every production reference from one Asset id to another.
    pub fn replace_asset_references(
        &self,
        old_asset_id: &AssetId,
        new_asset_id: AssetId,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let mut arrangement_changed = false;
            for clip in &mut session.arrangement.audio_clips {
                if clip.asset_id == *old_asset_id {
                    clip.asset_id = new_asset_id.clone();
                    arrangement_changed = true;
                }
            }
            let mut play_state_changed = false;
            for pad in &mut session.play_state.sample_instrument.pads {
                if pad.asset_id == *old_asset_id {
                    pad.asset_id = new_asset_id.clone();
                    play_state_changed = true;
                }
            }
            if !arrangement_changed && !play_state_changed {
                return Err(ApplicationError::InvalidCommand(format!(
                    "asset is not referenced by the project: {old_asset_id}"
                )));
            }
            if arrangement_changed {
                session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            }
            Ok(())
        })
    }
}
