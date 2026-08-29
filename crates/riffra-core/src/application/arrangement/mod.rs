//! Track, Clip, MIDI, and timeline application operations.

use super::*;

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Lists Tracks from the canonical production state.
    pub fn list_tracks(&self) -> Result<Vec<Track>, ApplicationError> {
        Ok(self.get_session()?.arrangement.tracks)
    }

    /// Lists all Timeline Clips from the canonical production state.
    pub fn list_audio_clips(&self) -> Result<Vec<AudioClip>, ApplicationError> {
        Ok(self.get_session()?.arrangement.audio_clips)
    }

    /// Lists all MIDI Clips from the canonical production state.
    pub fn list_midi_clips(&self) -> Result<Vec<MidiClip>, ApplicationError> {
        Ok(self.get_session()?.arrangement.midi_clips)
    }

    /// Adds a Track to the Arrangement.
    pub fn add_track(
        &self,
        name: impl Into<String>,
        kind: TrackKind,
    ) -> Result<CreativeSession, ApplicationError> {
        let name = normalize_track_name(name.into())?;
        self.commit_arrangement(|arrangement| {
            let id = next_id("track");
            let track = match kind {
                TrackKind::Audio => Track::audio(id, name),
                TrackKind::Instrument => Track::instrument(id, name),
            };
            arrangement.tracks.push(track);
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes a Track and its owned Timeline objects.
    pub fn remove_track(&self, track_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.remove_track(track_id).map_err(Into::into)
        })
    }

    /// Duplicates a Track and its owned timeline and automation objects.
    pub fn duplicate_track(&self, track_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let source_index = arrangement
                .tracks
                .iter()
                .position(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let operation_id = next_id("duplicate");
            let mut duplicate = arrangement.tracks[source_index].clone();
            duplicate.id = format!("track:{operation_id}");
            duplicate.name = format!("{} copy", duplicate.name);
            let duplicate_id = duplicate.id.clone();
            arrangement
                .tracks
                .insert(source_index.saturating_add(1), duplicate);

            let audio_clips = arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut clip)| {
                    clip.id = format!("clip:{operation_id}:{index}");
                    clip.track_id = duplicate_id.clone();
                    clip
                })
                .collect::<Vec<_>>();
            arrangement.audio_clips.extend(audio_clips);

            let midi_clips = arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut clip)| {
                    clip.id = format!("midi-clip:{operation_id}:{index}");
                    clip.track_id = duplicate_id.clone();
                    clip
                })
                .collect::<Vec<_>>();
            arrangement.midi_clips.extend(midi_clips);

            let automation_lanes = arrangement
                .automation_lanes
                .iter()
                .filter(|lane| lane.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut lane)| {
                    lane.id = format!("automation:{duplicate_id}:{index}");
                    lane.track_id = duplicate_id.clone();
                    for (point_index, point) in lane.points.iter_mut().enumerate() {
                        point.id = format!("automation-point:{operation_id}:{index}:{point_index}");
                    }
                    lane
                })
                .collect::<Vec<_>>();
            arrangement.automation_lanes.extend(automation_lanes);
            Ok(())
        })
    }

    /// Applies a validated Track mix and routing patch.
    pub fn update_track(
        &self,
        track_id: &str,
        patch: TrackPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if let Some(name) = patch.name {
                let name = name.trim().chars().take(80).collect::<String>();
                if name.is_empty() {
                    return Err(crate::DomainError::InvalidClip(
                        "track name must not be empty".into(),
                    )
                    .into());
                }
                track.name = name;
            }
            if let Some(gain_db) = patch.gain_db {
                track.gain_db = if gain_db.is_finite() {
                    gain_db.clamp(-90.0, 24.0)
                } else {
                    0.0
                };
            }
            if let Some(pan) = patch.pan {
                track.pan = if pan.is_finite() {
                    pan.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
            if let Some(muted) = patch.muted {
                track.muted = muted;
            }
            if let Some(solo) = patch.solo {
                track.solo = solo;
            }
            if let Some(armed) = patch.armed {
                track.armed = armed;
            }
            if let Some(monitoring) = patch.monitoring {
                track.monitoring = monitoring;
            }
            if let Some(color) = patch.color {
                let trimmed = color.trim();
                if trimmed.is_empty() {
                    track.color = None;
                } else {
                    if !is_valid_track_color(trimmed) {
                        return Err(crate::DomainError::InvalidClip(
                            "track color must be #rrggbb".into(),
                        )
                        .into());
                    }
                    track.color = Some(trimmed.to_ascii_lowercase());
                }
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Reorders a Track without changing its owned timeline objects.
    pub fn reorder_track(
        &self,
        track_id: &str,
        target_index: usize,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .reorder_track(track_id, target_index)
                .map_err(Into::into)
        })
    }

    /// Adds an already-validated Audio Clip after checking the host asset index.
    pub fn add_audio_clip(
        &self,
        clip: AudioClip,
        asset_exists: impl Fn(&crate::domain::asset::AssetId) -> bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .add_audio_clip(clip, asset_exists)
                .map_err(Into::into)
        })
    }

    /// Adds an Audio Asset to the timeline, selecting an existing Audio Track
    /// or creating one when no target was supplied or available.
    ///
    /// # Errors
    /// Returns an error when the target Track or Asset is invalid, or when the
    /// resulting session cannot be persisted.
    pub fn add_audio_asset_clip(
        &self,
        placement: AudioAssetClipPlacement,
        asset_exists: impl Fn(&crate::domain::asset::AssetId) -> bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let requested_track_id = placement.track_id.filter(|id| !id.trim().is_empty());
            let track_id = if let Some(track_id) = requested_track_id {
                let track = arrangement
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.clone()))?;
                if track.kind != TrackKind::Audio {
                    return Err(ApplicationError::InvalidCommand(
                        "audio clips can only be added to audio tracks".into(),
                    ));
                }
                track_id
            } else if let Some(track) = arrangement
                .tracks
                .iter()
                .find(|track| track.kind == TrackKind::Audio)
            {
                track.id.clone()
            } else {
                let track_id = next_id("track");
                arrangement
                    .tracks
                    .push(Track::audio(track_id.clone(), "Audio 1".into()));
                track_id
            };
            let append_tick = arrangement
                .audio_clips
                .iter()
                .map(|clip| {
                    let duration = arrangement.timebase.milliseconds_to_ticks(
                        clip.timeline_duration.frames as f64 * 1000.0
                            / f64::from(clip.timeline_duration.sample_rate),
                    );
                    clip.start_tick.0.saturating_add(duration.0)
                })
                .max()
                .unwrap_or(0);
            let clip = AudioClip::full_source(
                next_id("clip"),
                placement.name,
                track_id,
                placement.asset_id,
                placement.start_tick.unwrap_or(TimelineTick(append_tick)),
                placement.sample_rate,
                placement.source_frames,
            );
            arrangement
                .add_audio_clip(clip, asset_exists)
                .map_err(Into::into)
        })
    }

    /// Adds an already-validated MIDI Clip.
    pub fn add_midi_clip(&self, clip: MidiClip) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| arrangement.add_midi_clip(clip).map_err(Into::into))
    }

    /// Creates an empty MIDI Clip on an existing Instrument Track.
    ///
    /// The Core owns the Clip identity, default name, empty content, and
    /// duration normalization so hosts only submit user intent.
    ///
    /// # Errors
    ///
    /// Returns an error when the track is missing, is not an Instrument Track,
    /// or the resulting Clip cannot be validated or persisted.
    pub fn create_midi_clip(
        &self,
        track_id: &str,
        start_tick: TimelineTick,
        duration_ticks: u64,
        name: Option<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            create_midi_clip_in_arrangement(arrangement, track_id, start_tick, duration_ticks, name)
        })
    }

    /// Adds parsed MIDI Asset content to the timeline with Core-owned
    /// identities, creating an Instrument Track when necessary.
    ///
    /// # Errors
    /// Returns an error when the target Track or MIDI content is invalid, or
    /// when the resulting session cannot be persisted.
    pub fn add_midi_asset_clip(
        &self,
        placement: MidiAssetClipPlacement,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let requested_track_id = placement.track_id.filter(|id| !id.trim().is_empty());
            let track_id = if let Some(track_id) = requested_track_id {
                let track = arrangement
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.clone()))?;
                if track.kind != TrackKind::Instrument {
                    return Err(ApplicationError::InvalidCommand(
                        "midi clips can only be added to instrument tracks".into(),
                    ));
                }
                track_id
            } else if let Some(track) = arrangement
                .tracks
                .iter()
                .find(|track| track.kind == TrackKind::Instrument)
            {
                track.id.clone()
            } else {
                let track_id = next_id("track");
                arrangement
                    .tracks
                    .push(Track::instrument(track_id.clone(), "Instrument 1".into()));
                track_id
            };
            let mut notes = placement.notes;
            for note in &mut notes {
                note.id = next_id("note");
            }
            let mut events = placement.events;
            for event in &mut events {
                event.id = next_id("event");
            }
            let clip = MidiClip {
                id: next_id("midi-clip"),
                name: placement.name,
                track_id,
                asset_id: Some(placement.asset_id),
                start_tick: placement.start_tick.unwrap_or(TimelineTick(0)),
                duration_ticks: placement.duration_ticks,
                notes,
                events,
                muted: false,
                loop_enabled: false,
                recording_take_id: None,
            };
            arrangement.add_midi_clip(clip).map_err(Into::into)
        })
    }

    /// Replaces the project timebase through the canonical domain operation.
    pub fn update_timebase(
        &self,
        timebase: ProjectTimebase,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.update_timebase(timebase).map_err(Into::into)
        })
    }

    /// Updates the transport loop range through the canonical domain operation.
    pub fn update_loop_range(
        &self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_loop_range(enabled, start_tick, end_tick)
                .map_err(Into::into)
        })
    }

    /// Updates the transport punch range through the canonical domain operation.
    pub fn update_punch_range(
        &self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_punch_range(enabled, start_tick, end_tick)
                .map_err(Into::into)
        })
    }

    /// Applies a validated Audio Clip patch and commits the result.
    pub fn update_audio_clip(
        &self,
        clip_id: &str,
        patch: AudioClipPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_audio_clip(clip_id, patch)
                .map_err(Into::into)
        })
    }

    /// Applies a validated MIDI Clip patch and commits the result.
    pub fn update_midi_clip(
        &self,
        clip_id: &str,
        patch: MidiClipPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_midi_clip(clip_id, patch)
                .map_err(Into::into)
        })
    }

    /// Moves Audio Clips as one atomic arrangement edit.
    pub fn move_audio_clips(
        &self,
        moves: Vec<AudioClipMove>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.move_audio_clips(moves).map_err(Into::into)
        })
    }

    /// Moves MIDI Clips as one atomic arrangement edit.
    pub fn move_midi_clips(
        &self,
        moves: Vec<MidiClipMove>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.move_midi_clips(moves).map_err(Into::into)
        })
    }

    /// Removes selected Audio and MIDI Clips in one atomic edit.
    pub fn remove_timeline_clips(
        &self,
        audio_clip_ids: Vec<String>,
        midi_clip_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .remove_timeline_clips(&audio_clip_ids, &midi_clip_ids)
                .map_err(Into::into)
        })
    }

    /// Duplicates selected Clips at one timeline anchor.
    pub fn paste_timeline_clips(
        &self,
        audio_clip_ids: Vec<String>,
        midi_clip_ids: Vec<String>,
        start_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        let operation_id = next_id("paste");
        let audio_ids = (0..audio_clip_ids.len())
            .map(|index| format!("clip:{operation_id}:{index}"))
            .collect::<Vec<_>>();
        let midi_ids = (0..midi_clip_ids.len())
            .map(|index| format!("midi-clip:{operation_id}:{index}"))
            .collect::<Vec<_>>();
        self.commit_arrangement(|arrangement| {
            arrangement
                .paste_timeline_clips(
                    &audio_clip_ids,
                    &midi_clip_ids,
                    &audio_ids,
                    &midi_ids,
                    start_tick,
                )
                .map_err(Into::into)
        })
    }

    /// Trims an Audio Clip after the host validates its source length.
    pub fn trim_audio_clip(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        source_range: FrameRange,
        source_frames: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .trim_audio_clip(clip_id, start_tick, source_range, source_frames)
                .map_err(Into::into)
        })
    }

    /// Splits an Audio Clip at a musical position.
    pub fn split_audio_clip(
        &self,
        clip_id: &str,
        split_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        let right_id = next_id("clip:split");
        self.commit_arrangement(|arrangement| {
            arrangement
                .split_audio_clip(clip_id, split_tick, right_id)
                .map_err(Into::into)
        })
    }

    /// Duplicates an Audio Clip with a Core-owned identity.
    pub fn duplicate_audio_clip(&self, clip_id: &str) -> Result<CreativeSession, ApplicationError> {
        let duplicate_id = next_id("clip:duplicate");
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_audio_clip(clip_id, duplicate_id)
                .map_err(Into::into)
        })
    }

    /// Trims a MIDI Clip and its contained notes/events.
    pub fn trim_midi_clip(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        duration_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .trim_midi_clip(clip_id, start_tick, duration_ticks)
                .map_err(Into::into)
        })
    }

    /// Splits a MIDI Clip at a musical position.
    pub fn split_midi_clip(
        &self,
        clip_id: &str,
        split_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        let right_id = next_id("midi-clip:split");
        self.commit_arrangement(|arrangement| {
            arrangement
                .split_midi_clip(clip_id, split_tick, right_id)
                .map_err(Into::into)
        })
    }

    /// Duplicates a MIDI Clip with a Core-owned identity.
    pub fn duplicate_midi_clip(&self, clip_id: &str) -> Result<CreativeSession, ApplicationError> {
        let duplicate_id = next_id("midi-clip:duplicate");
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_midi_clip(clip_id, duplicate_id)
                .map_err(Into::into)
        })
    }

    /// Adds one MIDI note to an existing MIDI clip.
    pub fn add_midi_note(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        pitch: u8,
        duration_ticks: u64,
        velocity: u8,
        channel: u8,
    ) -> Result<CreativeSession, ApplicationError> {
        if pitch > 127 {
            return Err(ApplicationError::InvalidCommand(
                "midi pitch must be between 0 and 127".into(),
            ));
        }
        if velocity > 127 {
            return Err(ApplicationError::InvalidCommand(
                "midi velocity must be between 0 and 127".into(),
            ));
        }
        if !(1..=16).contains(&channel) {
            return Err(ApplicationError::InvalidCommand(
                "midi channel must be between 1 and 16".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            clip.notes.push(MidiNote {
                id: next_id("note"),
                note: pitch,
                start_tick,
                duration_ticks: duration_ticks.max(1),
                velocity,
                channel,
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Inserts multiple identity-free MIDI notes as one atomic edit.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty input, invalid MIDI values, an unknown
    /// Clip, or a note that would make the Clip invalid.
    pub fn insert_midi_notes(
        &self,
        clip_id: &str,
        inputs: Vec<MidiNoteInput>,
    ) -> Result<CreativeSession, ApplicationError> {
        if inputs.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one midi note is required".into(),
            ));
        }
        for input in &inputs {
            if input.pitch > 127 {
                return Err(ApplicationError::InvalidCommand(
                    "midi pitch must be between 0 and 127".into(),
                ));
            }
            if input.velocity > 127 {
                return Err(ApplicationError::InvalidCommand(
                    "midi velocity must be between 0 and 127".into(),
                ));
            }
            if !(1..=16).contains(&input.channel) {
                return Err(ApplicationError::InvalidCommand(
                    "midi channel must be between 1 and 16".into(),
                ));
            }
        }
        let notes = inputs
            .into_iter()
            .map(|input| MidiNote {
                id: next_id("note"),
                note: input.pitch,
                start_tick: input.start_tick,
                duration_ticks: input.duration_ticks.max(1),
                velocity: input.velocity,
                channel: input.channel,
            })
            .collect();
        self.commit_arrangement(|arrangement| {
            arrangement
                .insert_midi_notes(clip_id, notes)
                .map_err(Into::into)
        })
    }

    /// Applies one atomic set of updates to notes in a MIDI clip.
    pub fn update_midi_notes(
        &self,
        clip_id: &str,
        updates: Vec<MidiNoteUpdate>,
    ) -> Result<CreativeSession, ApplicationError> {
        if updates.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one midi note update is required".into(),
            ));
        }
        let unique_ids = updates
            .iter()
            .map(|update| update.note_id.as_str())
            .collect::<HashSet<_>>();
        if unique_ids.len() != updates.len() {
            return Err(ApplicationError::InvalidCommand(
                "each midi note may be updated only once per operation".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            for update in updates {
                let note = clip
                    .notes
                    .iter_mut()
                    .find(|note| note.id == update.note_id)
                    .ok_or_else(|| {
                        crate::DomainError::InvalidClip(format!(
                            "midi note '{}' is not registered",
                            update.note_id
                        ))
                    })?;
                if let Some(pitch) = update.patch.note {
                    note.note = pitch.min(127);
                }
                if let Some(start_tick) = update.patch.start_tick {
                    note.start_tick = start_tick;
                }
                if let Some(duration_ticks) = update.patch.duration_ticks {
                    note.duration_ticks = duration_ticks.max(1);
                }
                if let Some(velocity) = update.patch.velocity {
                    note.velocity = velocity.min(127);
                }
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes one MIDI note from an existing MIDI clip.
    pub fn remove_midi_note(
        &self,
        clip_id: &str,
        note_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            let before = clip.notes.len();
            clip.notes.retain(|note| note.id != note_id);
            if clip.notes.len() == before {
                return Err(crate::DomainError::InvalidClip(format!(
                    "midi note '{note_id}' is not registered"
                ))
                .into());
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes multiple MIDI notes as one atomic edit.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty selection, an unknown Clip, or a missing
    /// Note ID.
    pub fn remove_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .remove_midi_notes(clip_id, &note_ids)
                .map_err(Into::into)
        })
    }

    /// Clears all notes from a MIDI Clip without changing its placement,
    /// duration, or other MIDI events.
    ///
    /// # Errors
    ///
    /// Returns an error when the Clip is unknown or persistence fails.
    pub fn clear_midi_notes(&self, clip_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.clear_midi_notes(clip_id).map_err(Into::into)
        })
    }

    /// Transposes and velocity-offsets MIDI notes in a clip.
    ///
    /// When `note_ids` is non-empty only those notes are touched. When empty
    /// all notes in the clip are transformed. Pitch and velocity are clamped
    /// to `0..=127`.
    pub fn transform_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
        transpose_semitones: i16,
        velocity_offset: i16,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            if clip.notes.is_empty() {
                return Err(crate::DomainError::InvalidClip(
                    "midi clip has no notes to transform".into(),
                )
                .into());
            }
            if transpose_semitones == 0 && velocity_offset == 0 {
                return Err(crate::ApplicationError::InvalidCommand(
                    "transform requires a non-zero transpose or velocity offset".into(),
                ));
            }
            use std::collections::HashSet;
            let targets: HashSet<&str> = note_ids.iter().map(String::as_str).collect();
            if !targets.is_empty() {
                let known: HashSet<&str> = clip.notes.iter().map(|note| note.id.as_str()).collect();
                for id in &note_ids {
                    if !known.contains(id.as_str()) {
                        return Err(crate::DomainError::InvalidClip(format!(
                            "midi note '{id}' is not registered"
                        ))
                        .into());
                    }
                }
            }
            for note in clip.notes.iter_mut() {
                if !targets.is_empty() && !targets.contains(note.id.as_str()) {
                    continue;
                }
                let next_pitch = (note.note as i16 + transpose_semitones).clamp(0, 127) as u8;
                let next_velocity = (note.velocity as i16 + velocity_offset).clamp(0, 127) as u8;
                note.note = next_pitch;
                note.velocity = next_velocity;
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Quantizes selected MIDI notes to a positive grid.
    pub fn quantize_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
        grid_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .quantize_midi_notes(clip_id, &note_ids, grid_ticks)
                .map_err(Into::into)
        })
    }

    /// Duplicates selected MIDI notes within one clip.
    pub fn duplicate_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
        offset_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_midi_notes(clip_id, &note_ids, offset_ticks)
                .map_err(Into::into)
        })
    }

    /// Adds a named timeline marker.
    pub fn add_marker(
        &self,
        tick: TimelineTick,
        name: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let name = name.trim().chars().take(80).collect::<String>();
            arrangement.markers.push(Marker {
                id: next_id("marker"),
                name: if name.is_empty() {
                    "Marker".into()
                } else {
                    name
                },
                tick: tick.0,
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Updates one timeline marker.
    pub fn update_marker(
        &self,
        marker_id: &str,
        patch: MarkerPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let marker = arrangement
                .markers
                .iter_mut()
                .find(|marker| marker.id == marker_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "marker '{marker_id}' is not registered"
                    ))
                })?;
            if let Some(name) = patch.name {
                let name = name.trim().chars().take(80).collect::<String>();
                if !name.is_empty() {
                    marker.name = name;
                }
            }
            if let Some(tick) = patch.tick {
                marker.tick = tick.0;
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes one timeline marker.
    pub fn remove_marker(&self, marker_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let before = arrangement.markers.len();
            arrangement.markers.retain(|marker| marker.id != marker_id);
            if arrangement.markers.len() == before {
                return Err(crate::DomainError::InvalidClip(format!(
                    "marker '{marker_id}' is not registered"
                ))
                .into());
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Applies a crossfade between two neighboring Audio Clips.
    pub fn crossfade_audio_clips(
        &self,
        first_id: &str,
        second_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .crossfade_audio_clips(first_id, second_id)
                .map_err(Into::into)
        })
    }
}

pub(super) fn create_midi_clip_in_arrangement(
    arrangement: &mut Arrangement,
    track_id: &str,
    start_tick: TimelineTick,
    duration_ticks: u64,
    name: Option<String>,
) -> Result<(), ApplicationError> {
    arrangement
        .add_midi_clip(MidiClip {
            id: next_id("midi-clip"),
            name: normalize_midi_clip_name(name),
            track_id: track_id.to_owned(),
            asset_id: None,
            start_tick,
            duration_ticks: duration_ticks.max(1),
            notes: Vec::new(),
            events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        })
        .map_err(Into::into)
}
