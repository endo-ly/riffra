//! Music-oriented application operations over the canonical arrangement.

mod harmony;
mod phrase;

pub use harmony::{
    ChordVoicingInput, HarmonyEventInput, HarmonyEventPatch, HarmonyRealizeSelection,
    MusicalHarmonyEventView,
};

use super::*;
use crate::domain::{
    MidiNote, MusicalDuration, MusicalOffset, MusicalPitch, MusicalPosition, MusicalTimeDelta,
    ProjectTimebase, TimelineRegion, TimelineTick,
};
use crate::{DomainError, InputLocation};
use serde::{Deserialize, Serialize};

/// A MIDI note described using musical position, duration, and pitch values.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiNoteInput {
    pub pitch: MusicalPitch,
    pub position: MusicalPosition,
    pub duration: MusicalDuration,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub velocity: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub channel: Option<u8>,
}

/// A MIDI note exposed in musical coordinates rather than timeline ticks.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiNoteView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub pitch: MusicalPitch,
    pub position: MusicalPosition,
    pub duration: MusicalDuration,
    pub velocity: u8,
    pub channel: u8,
}

/// Scope for a music-level Note query or transform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicalNoteScope {
    /// One Clip to inspect, when supplied.
    pub clip_id: Option<String>,
    /// One Track whose MIDI Clips should be inspected, when supplied.
    pub track_id: Option<String>,
}

/// Parameters for the lightweight Note query.
#[derive(Clone, Debug)]
pub struct MusicalNoteListRequest {
    /// Clip or Track scope.
    pub scope: MusicalNoteScope,
    /// Optional half-open musical range.
    pub start: Option<MusicalPosition>,
    /// Optional half-open musical range end.
    pub end: Option<MusicalPosition>,
    /// Whether stable Note IDs should be included.
    pub include_ids: bool,
    /// Whether to return raw Clip-relative MIDI values instead of musical values.
    pub raw: bool,
}

/// A grouped Note query response.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteListView {
    /// Number of Notes across all returned Clips.
    pub count: usize,
    /// Raw timebase metadata, included only for raw responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timebase: Option<MusicalNoteTimebaseView>,
    /// Notes grouped by Clip.
    pub clips: Vec<MusicalNoteClipView>,
}

/// The project timebase needed to interpret raw Note ticks.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteTimebaseView {
    pub ppq: u32,
    pub time_signature_numerator: u8,
    pub time_signature_denominator: u8,
}

/// A Clip group in a Note query response.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalNoteClipView {
    pub clip_id: String,
    /// Timeline Clip start, present only for raw responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<u64>,
    pub notes: Vec<MusicalNoteListNoteView>,
}

/// One Note in either the musical or raw response representation.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum MusicalNoteListNoteView {
    /// A Note represented by musical coordinates.
    Musical(MusicalMidiNoteView),
    /// A Note represented by Clip-relative MIDI values.
    Raw(RawMidiNoteView),
}

/// A raw MIDI Note with Clip-relative timing.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMidiNoteView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub pitch: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// Parameters for an atomic music-level Note transform.
#[derive(Clone, Debug)]
pub struct MusicalNoteTransformRequest {
    /// Clip or Track scope.
    pub scope: MusicalNoteScope,
    /// Half-open range used to select Note start positions.
    pub start: Option<MusicalPosition>,
    /// Half-open range end used to select Note start positions.
    pub end: Option<MusicalPosition>,
    /// Optional pre-transform pitch filter.
    pub pitch: Option<MusicalPitch>,
    /// Optional pre-transform MIDI channel filter.
    pub channel: Option<u8>,
    /// Signed musical displacement.
    pub timing_offset: Option<MusicalTimeDelta>,
    /// Velocity displacement, clamped to the MIDI range.
    pub velocity_offset: Option<i32>,
    /// Pitch displacement in semitones.
    pub transpose_semitones: Option<i16>,
}

fn deserialize_optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom("expected a value, found null"));
    }
    T::deserialize(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// Partial update for a MIDI note expressed in musical coordinates.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiNotePatch {
    pub pitch: Option<MusicalPitch>,
    pub position: Option<MusicalPosition>,
    pub duration: Option<MusicalDuration>,
    pub velocity: Option<u8>,
    pub channel: Option<u8>,
}

/// A music-level view of a named timeline range.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRegionView {
    pub id: String,
    pub name: String,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
}

#[derive(Clone, Copy)]
struct ResolvedMidiNoteInput {
    pitch: u8,
    absolute_start_tick: TimelineTick,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
}

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Creates a MIDI Clip from absolute musical positions and returns its
    /// Core-allocated identity.
    ///
    /// # Errors
    ///
    /// Returns an error when a musical position is invalid, the range is not
    /// positive, the Track is missing or not an Instrument Track, or the
    /// canonical commit cannot be persisted.
    pub fn create_musical_midi_clip_with_created_ids(
        &self,
        track_id: &str,
        start: MusicalPosition,
        end: MusicalPosition,
        name: Option<String>,
    ) -> Result<ApplicationMutation, ApplicationError> {
        let mut created_entity_ids = CreatedEntityIds::new();
        let session = self.commit_arrangement(|arrangement| {
            let start_tick = arrangement.timebase.musical_position_to_tick(start)?;
            let end_tick = arrangement.timebase.musical_position_to_tick(end)?;
            let duration_ticks = end_tick.0.checked_sub(start_tick.0).ok_or_else(|| {
                crate::DomainError::InvalidMusicalValue(
                    "musical clip end must be after its start".into(),
                )
            })?;
            if duration_ticks == 0 {
                return Err(crate::DomainError::InvalidMusicalValue(
                    "musical clip end must be after its start".into(),
                )
                .into());
            }
            let id = super::arrangement::create_midi_clip_in_arrangement(
                arrangement,
                track_id,
                start_tick,
                duration_ticks,
                name,
            )?;
            record_created(&mut created_entity_ids, "midiClips", id);
            Ok(())
        })?;
        Ok(ApplicationMutation::new(session, created_entity_ids))
    }

    /// Inserts MIDI notes at absolute musical positions and returns their
    /// Core-allocated identities.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is empty, a musical value is invalid,
    /// a note precedes the Clip, the Clip is missing, or the canonical commit
    /// cannot be persisted.
    pub fn insert_musical_notes_with_created_ids(
        &self,
        clip_id: &str,
        inputs: Vec<MusicalMidiNoteInput>,
    ) -> Result<ApplicationMutation, ApplicationError> {
        if inputs.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one musical midi note is required".into(),
            ));
        }
        let mut created_entity_ids = CreatedEntityIds::new();
        let session = self.commit_arrangement(|arrangement| {
            let available_notes = available_midi_note_capacity(arrangement, clip_id)?;
            if inputs.len() > available_notes {
                return Err(too_many_midi_notes().into());
            }
            let timebase = arrangement.timebase;
            let notes = inputs
                .into_iter()
                .enumerate()
                .map(|(index, input)| {
                    resolve_musical_note(timebase, input).map_err(|error| {
                        ApplicationError::InvalidInput {
                            location: InputLocation {
                                collection: "notes".into(),
                                index,
                                field: None,
                            },
                            message: error.to_string(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let ids = insert_resolved_midi_notes_in_arrangement(arrangement, clip_id, notes)?;
            for id in ids {
                record_created(&mut created_entity_ids, "midiNotes", id);
            }
            Ok(())
        })?;
        Ok(ApplicationMutation::new(session, created_entity_ids))
    }

    /// Lists MIDI notes using absolute musical positions.
    pub fn list_musical_notes(
        &self,
        request: MusicalNoteListRequest,
    ) -> Result<MusicalNoteListView, ApplicationError> {
        let session = self.get_session()?;
        let timebase = session.arrangement.timebase;
        if let Some(track_id) = request.scope.track_id.as_deref()
            && !session
                .arrangement
                .tracks
                .iter()
                .any(|track| track.id == track_id)
        {
            return Err(ApplicationError::InvalidCommand(format!(
                "track '{track_id}' is not registered"
            )));
        }
        let query = resolve_note_range(timebase, request.start, request.end)?;
        let clips = select_note_clips(&session.arrangement.midi_clips, &request.scope)?;
        if request.scope.track_id.is_some() && query.is_none() {
            return Err(ApplicationError::InvalidCommand(
                "track note list requires both start and end".into(),
            ));
        }
        let mut clip_views = clips
            .into_iter()
            .filter_map(|clip| {
                let mut notes = clip
                    .notes
                    .iter()
                    .filter(|note| note_overlaps_range(clip, note, query))
                    .collect::<Vec<_>>();
                notes.sort_by_key(|note| (note.start_tick, note.note, note.id.clone()));
                if request.scope.track_id.is_some() && notes.is_empty() {
                    return None;
                }
                let notes = notes
                    .into_iter()
                    .map(
                        |note| -> Result<MusicalNoteListNoteView, ApplicationError> {
                            if request.raw {
                                Ok(MusicalNoteListNoteView::Raw(RawMidiNoteView {
                                    id: request.include_ids.then(|| note.id.clone()),
                                    pitch: note.note,
                                    start_tick: note.start_tick.0,
                                    duration_ticks: note.duration_ticks,
                                    velocity: note.velocity,
                                    channel: note.channel,
                                }))
                            } else {
                                Ok(MusicalNoteListNoteView::Musical(musical_note_view(
                                    timebase,
                                    clip.start_tick,
                                    note,
                                    request.include_ids,
                                )?))
                            }
                        },
                    )
                    .collect::<Result<Vec<_>, ApplicationError>>();
                Some(notes.map(|notes| MusicalNoteClipView {
                    clip_id: clip.id.clone(),
                    start_tick: request.raw.then_some(clip.start_tick.0),
                    notes,
                }))
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        clip_views.sort_by(|left, right| {
            let left_clip = session
                .arrangement
                .midi_clips
                .iter()
                .find(|clip| clip.id == left.clip_id)
                .expect("selected clip remains in session");
            let right_clip = session
                .arrangement
                .midi_clips
                .iter()
                .find(|clip| clip.id == right.clip_id)
                .expect("selected clip remains in session");
            left_clip
                .start_tick
                .cmp(&right_clip.start_tick)
                .then_with(|| left.clip_id.cmp(&right.clip_id))
        });
        let count = clip_views.iter().map(|clip| clip.notes.len()).sum();
        Ok(MusicalNoteListView {
            count,
            timebase: request.raw.then_some(MusicalNoteTimebaseView {
                ppq: timebase.ppq,
                time_signature_numerator: timebase.time_signature_numerator,
                time_signature_denominator: timebase.time_signature_denominator,
            }),
            clips: clip_views,
        })
    }

    /// Applies one atomic music-level transform to Notes selected by scope and
    /// pre-transform musical conditions.
    pub fn transform_musical_notes(
        &self,
        request: MusicalNoteTransformRequest,
    ) -> Result<ApplicationMutation, ApplicationError> {
        if request.timing_offset.is_none()
            && request.velocity_offset.is_none()
            && request.transpose_semitones.is_none()
        {
            return Err(ApplicationError::InvalidCommand(
                "at least one note transform is required".into(),
            ));
        }
        if request
            .channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(ApplicationError::InvalidCommand(
                "note transform channel must be between 1 and 16".into(),
            ));
        }
        if request
            .timing_offset
            .is_some_and(|offset| offset.numerator == 0)
            && request.velocity_offset.is_none_or(|offset| offset == 0)
            && request
                .transpose_semitones
                .is_none_or(|semitones| semitones == 0)
        {
            return Err(ApplicationError::InvalidCommand(
                "note transform must change at least one value".into(),
            ));
        }
        let mut matched = false;
        let mutation = self.commit_arrangement_with_created_ids(|arrangement, _| {
            let timebase = arrangement.timebase;
            if let Some(track_id) = request.scope.track_id.as_deref()
                && !arrangement.tracks.iter().any(|track| track.id == track_id)
            {
                return Err(ApplicationError::InvalidCommand(format!(
                    "track '{track_id}' is not registered"
                )));
            }
            let query = resolve_note_range(timebase, request.start, request.end)?;
            if request.scope.track_id.is_some() && query.is_none() {
                return Err(ApplicationError::InvalidCommand(
                    "track note transform requires both start and end".into(),
                ));
            }
            let timing_ticks = request
                .timing_offset
                .map(|offset| timebase.musical_time_delta_to_ticks(offset))
                .transpose()?;
            let clip_ids = select_note_clips(&arrangement.midi_clips, &request.scope)?
                .into_iter()
                .map(|clip| clip.id.clone())
                .collect::<Vec<_>>();
            for clip_id in clip_ids {
                let track_id = {
                    let clip = arrangement
                        .midi_clips
                        .iter_mut()
                        .find(|clip| clip.id == clip_id)
                        .expect("selected clip remains in arrangement");
                    for note in &mut clip.notes {
                        let absolute_start = clip
                            .start_tick
                            .0
                            .checked_add(note.start_tick.0)
                            .ok_or_else(|| {
                                ApplicationError::InvalidCommand(
                                    "MIDI note position is too large".into(),
                                )
                            })?;
                        if !note_starts_in_range(absolute_start, query)
                            || request
                                .pitch
                                .is_some_and(|pitch| pitch.midi_pitch() != note.note)
                            || request
                                .channel
                                .is_some_and(|channel| channel != note.channel)
                        {
                            continue;
                        }
                        matched = true;
                        let next_start = timing_ticks
                            .map(|offset| i128::from(note.start_tick.0) + i128::from(offset))
                            .unwrap_or_else(|| i128::from(note.start_tick.0));
                        let next_end = next_start + i128::from(note.duration_ticks);
                        if next_start < 0 || next_end > i128::from(clip.duration_ticks) {
                            return Err(ApplicationError::InvalidCommand(
                                "note transform would move a note outside its MIDI clip".into(),
                            ));
                        }
                        if let Some(transpose) = request.transpose_semitones {
                            let next_pitch = i16::from(note.note) + transpose;
                            if !(0..=127).contains(&next_pitch) {
                                return Err(ApplicationError::InvalidCommand(
                                    "note transform would move a pitch outside the MIDI range"
                                        .into(),
                                ));
                            }
                            note.note = u8::try_from(next_pitch).expect("pitch range was checked");
                        }
                        if let Some(offset) = request.velocity_offset {
                            let next_velocity = i32::from(note.velocity)
                                .saturating_add(offset)
                                .clamp(0, 127);
                            note.velocity =
                                u8::try_from(next_velocity).expect("velocity was clamped");
                        }
                        note.start_tick = TimelineTick(
                            u64::try_from(next_start).expect("note start was checked non-negative"),
                        );
                    }
                    clip.track_id.clone()
                };
                let track = arrangement.tracks.iter().find(|track| track.id == track_id);
                let clip = arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .expect("selected clip remains in arrangement");
                clip.validate_and_normalize(track)
                    .map_err(DomainError::InvalidClip)?;
            }
            if !matched {
                return Err(ApplicationError::InvalidCommand(
                    "note selection matched no notes".into(),
                ));
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(mutation)
    }

    /// Gets one MIDI note using musical coordinates.
    pub fn get_musical_note(
        &self,
        clip_id: &str,
        note_id: &str,
    ) -> Result<MusicalMidiNoteView, ApplicationError> {
        let session = self.get_session()?;
        let timebase = session.arrangement.timebase;
        let clip = session
            .arrangement
            .midi_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                ApplicationError::InvalidCommand(format!("midi clip '{clip_id}' is not registered"))
            })?;
        let note = clip
            .notes
            .iter()
            .find(|note| note.id == note_id)
            .ok_or_else(|| {
                ApplicationError::InvalidCommand(format!("midi note '{note_id}' is not registered"))
            })?;
        musical_note_view(timebase, clip.start_tick, note, true)
    }

    /// Updates one MIDI note using only the supplied musical fields.
    pub fn update_musical_note(
        &self,
        clip_id: &str,
        note_id: &str,
        patch: MusicalMidiNotePatch,
    ) -> Result<CreativeSession, ApplicationError> {
        if patch.pitch.is_none()
            && patch.position.is_none()
            && patch.duration.is_none()
            && patch.velocity.is_none()
            && patch.channel.is_none()
        {
            return Err(ApplicationError::InvalidCommand(
                "at least one musical note field is required".into(),
            ));
        }
        let session = self.get_session()?;
        let timebase = session.arrangement.timebase;
        let clip_start = session
            .arrangement
            .midi_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                ApplicationError::InvalidCommand(format!("midi clip '{clip_id}' is not registered"))
            })?
            .start_tick;
        let raw_patch = MidiNotePatch {
            note: patch.pitch.map(MusicalPitch::midi_pitch),
            start_tick: patch
                .position
                .map(|position| timebase.musical_position_to_tick(position))
                .transpose()?
                .map(|absolute| {
                    absolute.0.checked_sub(clip_start.0).ok_or_else(|| {
                        crate::DomainError::InvalidMusicalValue(
                            "note position must not precede the MIDI clip".into(),
                        )
                    })
                })
                .transpose()?
                .map(TimelineTick),
            duration_ticks: patch
                .duration
                .map(|duration| timebase.musical_duration_to_ticks(duration))
                .transpose()?,
            velocity: patch.velocity,
            channel: patch.channel,
        };
        self.update_midi_notes(
            clip_id,
            vec![MidiNoteUpdate {
                note_id: note_id.to_owned(),
                patch: raw_patch,
            }],
        )
    }

    /// Removes one MIDI note by its stable identity.
    pub fn remove_musical_note(
        &self,
        clip_id: &str,
        note_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.remove_midi_note(clip_id, note_id)
    }

    /// Resizes a MIDI Clip using absolute musical positions.
    pub fn resize_musical_midi_clip(
        &self,
        clip_id: &str,
        start: Option<MusicalPosition>,
        end: Option<MusicalPosition>,
    ) -> Result<CreativeSession, ApplicationError> {
        if start.is_none() && end.is_none() {
            return Err(ApplicationError::InvalidCommand(
                "MIDI clip resize requires a start or end".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let timebase = arrangement.timebase;
            arrangement
                .resize_midi_clip(
                    clip_id,
                    start
                        .map(|value| timebase.musical_position_to_tick(value))
                        .transpose()?,
                    end.map(|value| timebase.musical_position_to_tick(value))
                        .transpose()?,
                )
                .map_err(Into::into)
        })
    }

    /// Lists all named timeline ranges.
    ///
    /// # Errors
    ///
    /// Returns an error when the canonical session cannot be read.
    pub fn list_regions(&self) -> Result<Vec<MusicalRegionView>, ApplicationError> {
        let session = self.get_session()?;
        let timebase = session.arrangement.timebase;
        Ok(session
            .arrangement
            .regions
            .into_iter()
            .map(|region| MusicalRegionView {
                id: region.id,
                name: region.name,
                start: timebase.tick_to_musical_position(region.start_tick),
                end: timebase.tick_to_musical_position(region.end_tick),
            })
            .collect())
    }

    /// Adds a named timeline range from absolute musical positions and returns
    /// its Core-allocated identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the name, positions, or range is invalid, or the
    /// canonical commit cannot be persisted.
    pub fn add_region_with_created_ids(
        &self,
        name: String,
        start: MusicalPosition,
        end: MusicalPosition,
    ) -> Result<ApplicationMutation, ApplicationError> {
        let name = normalize_region_name(name)?;
        let id = next_id("region");
        let session = self.commit_arrangement(|arrangement| {
            let start_tick = arrangement.timebase.musical_position_to_tick(start)?;
            let end_tick = arrangement.timebase.musical_position_to_tick(end)?;
            arrangement
                .add_region(TimelineRegion {
                    id: id.clone(),
                    name,
                    start_tick,
                    end_tick,
                })
                .map_err(Into::into)
        })?;
        Ok(ApplicationMutation::one(session, "regions", id))
    }

    /// Updates a named timeline range using only the supplied fields.
    ///
    /// # Errors
    ///
    /// Returns an error when the region is missing, an updated field is
    /// invalid, or the canonical commit cannot be persisted.
    pub fn update_region(
        &self,
        region_id: &str,
        name: Option<String>,
        start: Option<MusicalPosition>,
        end: Option<MusicalPosition>,
    ) -> Result<CreativeSession, ApplicationError> {
        let name = name.map(normalize_region_name).transpose()?;
        self.commit_arrangement(|arrangement| {
            let timebase = arrangement.timebase;
            arrangement
                .update_region(
                    region_id,
                    name,
                    start
                        .map(|position| timebase.musical_position_to_tick(position))
                        .transpose()?,
                    end.map(|position| timebase.musical_position_to_tick(position))
                        .transpose()?,
                )
                .map_err(Into::into)
        })
    }

    /// Removes a named timeline range.
    ///
    /// # Errors
    ///
    /// Returns an error when the region is missing or the canonical commit
    /// cannot be persisted.
    pub fn remove_region(&self, region_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.remove_region(region_id).map_err(Into::into)
        })
    }
}

fn resolve_musical_note(
    timebase: crate::domain::ProjectTimebase,
    input: MusicalMidiNoteInput,
) -> Result<ResolvedMidiNoteInput, DomainError> {
    let velocity = input.velocity.unwrap_or(100);
    let channel = input.channel.unwrap_or(1);
    if velocity > 127 {
        return Err(DomainError::InvalidMusicalValue(
            "midi velocity must be between 0 and 127".into(),
        ));
    }
    if !(1..=16).contains(&channel) {
        return Err(DomainError::InvalidMusicalValue(
            "midi channel must be between 1 and 16".into(),
        ));
    }
    Ok(ResolvedMidiNoteInput {
        pitch: input.pitch.midi_pitch(),
        absolute_start_tick: timebase.musical_position_to_tick(input.position)?,
        duration_ticks: timebase.musical_duration_to_ticks(input.duration)?,
        velocity,
        channel,
    })
}

fn musical_note_view(
    timebase: ProjectTimebase,
    clip_start: TimelineTick,
    note: &MidiNote,
    include_id: bool,
) -> Result<MusicalMidiNoteView, ApplicationError> {
    let absolute_start = clip_start.0.checked_add(note.start_tick.0).ok_or_else(|| {
        ApplicationError::InvalidCommand("MIDI note position is too large".into())
    })?;
    Ok(MusicalMidiNoteView {
        id: include_id.then(|| note.id.clone()),
        pitch: MusicalPitch::from_midi_pitch(note.note)?,
        position: timebase.tick_to_musical_position(TimelineTick(absolute_start)),
        duration: timebase.ticks_to_musical_duration(note.duration_ticks)?,
        velocity: note.velocity,
        channel: note.channel,
    })
}

fn resolve_note_range(
    timebase: ProjectTimebase,
    start: Option<MusicalPosition>,
    end: Option<MusicalPosition>,
) -> Result<Option<(TimelineTick, TimelineTick)>, ApplicationError> {
    if start.is_some() != end.is_some() {
        return Err(ApplicationError::InvalidCommand(
            "note range requires both start and end".into(),
        ));
    }
    let range = start
        .zip(end)
        .map(|(start, end)| {
            let start = timebase.musical_position_to_tick(start)?;
            let end = timebase.musical_position_to_tick(end)?;
            if end <= start {
                return Err(ApplicationError::InvalidCommand(
                    "note range must have a positive duration".into(),
                ));
            }
            Ok((start, end))
        })
        .transpose()?;
    Ok(range)
}

fn select_note_clips<'a>(
    clips: &'a [crate::domain::MidiClip],
    scope: &MusicalNoteScope,
) -> Result<Vec<&'a crate::domain::MidiClip>, ApplicationError> {
    match (&scope.clip_id, &scope.track_id) {
        (Some(_), Some(_)) | (None, None) => Err(ApplicationError::InvalidCommand(
            "note scope requires exactly one clipId or trackId".into(),
        )),
        (Some(clip_id), None) => clips
            .iter()
            .find(|clip| clip.id == *clip_id)
            .map(|clip| vec![clip])
            .ok_or_else(|| {
                ApplicationError::InvalidCommand(format!("midi clip '{clip_id}' is not registered"))
            }),
        (None, Some(track_id)) => Ok(clips
            .iter()
            .filter(|clip| clip.track_id == *track_id)
            .collect()),
    }
}

fn note_overlaps_range(
    clip: &crate::domain::MidiClip,
    note: &MidiNote,
    range: Option<(TimelineTick, TimelineTick)>,
) -> bool {
    let Some((start, end)) = range else {
        return true;
    };
    let Some(note_start) = clip.start_tick.0.checked_add(note.start_tick.0) else {
        return false;
    };
    let Some(note_end) = note_start.checked_add(note.duration_ticks) else {
        return false;
    };
    note_start < end.0 && note_end > start.0
}

fn note_starts_in_range(note_start: u64, range: Option<(TimelineTick, TimelineTick)>) -> bool {
    range.is_none_or(|(start, end)| start.0 <= note_start && note_start < end.0)
}

fn insert_resolved_midi_notes_in_arrangement(
    arrangement: &mut crate::domain::Arrangement,
    clip_id: &str,
    inputs: Vec<ResolvedMidiNoteInput>,
) -> Result<Vec<String>, DomainError> {
    if inputs.is_empty() {
        return Err(DomainError::InvalidMusicalValue(
            "at least one musical midi note is required".into(),
        ));
    }
    let (clip_start, available_notes) = arrangement
        .midi_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .map(|clip| {
            (
                clip.start_tick,
                crate::domain::arrangement::MAX_MIDI_NOTES_PER_CLIP.checked_sub(clip.notes.len()),
            )
        })
        .ok_or_else(|| {
            DomainError::InvalidClip(format!("midi clip '{clip_id}' is not registered"))
        })?;
    let available_notes = available_notes.ok_or_else(too_many_midi_notes)?;
    if inputs.len() > available_notes {
        return Err(too_many_midi_notes());
    }
    let notes = inputs
        .into_iter()
        .map(|input| {
            let start_tick = input
                .absolute_start_tick
                .0
                .checked_sub(clip_start.0)
                .ok_or_else(|| {
                    DomainError::InvalidMusicalValue(
                        "note position must not precede the MIDI clip".into(),
                    )
                })?;
            Ok(MidiNote {
                id: next_id("note"),
                note: input.pitch,
                start_tick: TimelineTick(start_tick),
                duration_ticks: input.duration_ticks,
                velocity: input.velocity,
                channel: input.channel,
            })
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    let ids = notes.iter().map(|note| note.id.clone()).collect::<Vec<_>>();
    arrangement.insert_midi_notes(clip_id, notes)?;
    Ok(ids)
}

fn available_midi_note_capacity(
    arrangement: &crate::domain::Arrangement,
    clip_id: &str,
) -> Result<usize, DomainError> {
    arrangement
        .midi_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .map(|clip| {
            crate::domain::arrangement::MAX_MIDI_NOTES_PER_CLIP
                .checked_sub(clip.notes.len())
                .ok_or_else(too_many_midi_notes)
        })
        .ok_or_else(|| {
            DomainError::InvalidClip(format!("midi clip '{clip_id}' is not registered"))
        })?
}

fn push_resolved_midi_note(
    notes: &mut Vec<ResolvedMidiNoteInput>,
    available_notes: usize,
    note: ResolvedMidiNoteInput,
) -> Result<(), DomainError> {
    if notes.len() >= available_notes {
        return Err(too_many_midi_notes());
    }
    notes.push(note);
    Ok(())
}

fn too_many_midi_notes() -> DomainError {
    DomainError::InvalidClip(format!(
        "a MIDI clip cannot contain more than {} notes",
        crate::domain::arrangement::MAX_MIDI_NOTES_PER_CLIP
    ))
}

fn repeated_offset_to_ticks(
    timebase: ProjectTimebase,
    pattern_length: MusicalDuration,
    repeat: u64,
    step_offset: MusicalOffset,
) -> Result<u64, DomainError> {
    let length_numerator = u128::from(pattern_length.numerator)
        .checked_mul(u128::from(repeat))
        .ok_or_else(|| DomainError::InvalidMusicalValue("pattern offset is too large".into()))?;
    let length_denominator = u128::from(pattern_length.denominator);
    let offset_numerator = u128::from(step_offset.numerator);
    let offset_denominator = u128::from(step_offset.denominator);
    let denominator_gcd = gcd_u128(length_denominator, offset_denominator);
    let left_multiplier = offset_denominator / denominator_gcd;
    let right_multiplier = length_denominator / denominator_gcd;
    let numerator = length_numerator
        .checked_mul(left_multiplier)
        .and_then(|value| {
            offset_numerator
                .checked_mul(right_multiplier)
                .and_then(|offset| value.checked_add(offset))
        })
        .ok_or_else(|| DomainError::InvalidMusicalValue("pattern offset is too large".into()))?;
    let denominator = length_denominator
        .checked_mul(left_multiplier)
        .ok_or_else(|| DomainError::InvalidMusicalValue("pattern offset is too large".into()))?;
    rational_to_ticks(timebase, numerator, denominator)
}

fn rational_to_ticks(
    timebase: ProjectTimebase,
    mut numerator: u128,
    mut denominator: u128,
) -> Result<u64, DomainError> {
    if denominator == 0 || timebase.ppq == 0 {
        return Err(DomainError::InvalidMusicalValue(
            "musical timebase is invalid".into(),
        ));
    }
    let fraction_gcd = gcd_u128(numerator, denominator);
    numerator /= fraction_gcd;
    denominator /= fraction_gcd;
    let whole_note_ticks = u128::from(timebase.ppq)
        .checked_mul(4)
        .ok_or_else(|| DomainError::InvalidMusicalValue("pattern offset is too large".into()))?;
    let tick_gcd = gcd_u128(whole_note_ticks, denominator);
    let scaled_numerator = numerator
        .checked_mul(whole_note_ticks / tick_gcd)
        .ok_or_else(|| DomainError::InvalidMusicalValue("pattern offset is too large".into()))?;
    let scaled_denominator = denominator / tick_gcd;
    let whole = scaled_numerator / scaled_denominator;
    let remainder = scaled_numerator % scaled_denominator;
    let rounded = whole + u128::from(remainder >= scaled_denominator - remainder);
    u64::try_from(rounded)
        .map_err(|_| DomainError::InvalidMusicalValue("pattern offset is too large".into()))
}

fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn normalize_region_name(name: String) -> Result<String, ApplicationError> {
    let name = name.trim().chars().take(80).collect::<String>();
    if name.is_empty() {
        return Err(ApplicationError::InvalidCommand(
            "region name must not be empty".into(),
        ));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PortError;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStorage(Mutex<Vec<CreativeSession>>);

    impl SessionStorage for MemoryStorage {
        fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
            self.0.lock().unwrap().push(session.clone());
            Ok(())
        }
    }

    #[test]
    fn musical_notes_use_absolute_positions_and_one_commit() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        let track = application
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        let clip = application
            .create_musical_midi_clip_with_created_ids(
                &track_id,
                "5:1".parse().unwrap(),
                "13:1".parse().unwrap(),
                Some("Piano".into()),
            )
            .unwrap();
        let clip_id = clip.session.arrangement.midi_clips[0].id.clone();
        let inserted = application
            .insert_musical_notes_with_created_ids(
                &clip_id,
                vec![
                    MusicalMidiNoteInput {
                        pitch: "C4".parse().unwrap(),
                        position: "5:1".parse().unwrap(),
                        duration: "1/8".parse().unwrap(),
                        velocity: None,
                        channel: None,
                    },
                    MusicalMidiNoteInput {
                        pitch: "Bb4".parse().unwrap(),
                        position: "6:3+1/3".parse().unwrap(),
                        duration: "1/12".parse().unwrap(),
                        velocity: Some(92),
                        channel: Some(2),
                    },
                    MusicalMidiNoteInput {
                        pitch: "A4".parse().unwrap(),
                        position: "12:4".parse().unwrap(),
                        duration: "1/8".parse().unwrap(),
                        velocity: None,
                        channel: None,
                    },
                ],
            )
            .unwrap();

        let notes = &inserted.session.arrangement.midi_clips[0].notes;
        assert_eq!(notes[0].start_tick, TimelineTick(0));
        assert_eq!(notes[0].duration_ticks, 480);
        assert_eq!(notes[0].note, 60);
        assert_eq!(notes[1].start_tick, TimelineTick(6_080));
        assert_eq!(notes[1].duration_ticks, 320);
        assert_eq!(notes[1].note, 70);
        assert_eq!(notes[2].start_tick, TimelineTick(29_760));
        assert_eq!(notes[2].duration_ticks, 480);
        assert_eq!(notes[2].note, 69);
        assert_eq!(
            inserted.session.arrangement.midi_clips[0].duration_ticks,
            30_720
        );
        assert!(
            application
                .insert_musical_notes_with_created_ids(
                    &clip_id,
                    vec![MusicalMidiNoteInput {
                        pitch: "C4".parse().unwrap(),
                        position: "4:4".parse().unwrap(),
                        duration: "1/8".parse().unwrap(),
                        velocity: None,
                        channel: None,
                    }],
                )
                .is_err()
        );
        assert_eq!(storage.0.lock().unwrap().len(), 3);
    }

    #[test]
    fn musical_note_crud_and_clip_resize_keep_absolute_positions() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        let track = application
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let clip = application
            .create_musical_midi_clip_with_created_ids(
                &track.session.arrangement.tracks[0].id,
                "1:1".parse().unwrap(),
                "5:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.session.arrangement.midi_clips[0].id.clone();
        let inserted = application
            .insert_musical_notes_with_created_ids(
                &clip_id,
                vec![MusicalMidiNoteInput {
                    pitch: "C4".parse().unwrap(),
                    position: "2:1".parse().unwrap(),
                    duration: "1/4".parse().unwrap(),
                    velocity: Some(90),
                    channel: Some(2),
                }],
            )
            .unwrap();
        let note_id = inserted.session.arrangement.midi_clips[0].notes[0]
            .id
            .clone();

        let listed = application
            .list_musical_notes(MusicalNoteListRequest {
                scope: MusicalNoteScope {
                    clip_id: Some(clip_id.clone()),
                    track_id: None,
                },
                start: Some("2:1+1/8".parse().unwrap()),
                end: Some("2:1+1/4".parse().unwrap()),
                include_ids: true,
                raw: false,
            })
            .unwrap();
        assert_eq!(listed.count, 1);
        let MusicalNoteListNoteView::Musical(note) = &listed.clips[0].notes[0] else {
            panic!("expected musical note view");
        };
        assert_eq!(note.position.to_string(), "2:1");
        assert_eq!(note.duration.to_string(), "1/4");
        assert_eq!(note.channel, 2);

        application
            .update_musical_note(
                &clip_id,
                &note_id,
                MusicalMidiNotePatch {
                    position: Some("3:1".parse().unwrap()),
                    duration: Some("1/2".parse().unwrap()),
                    ..MusicalMidiNotePatch::default()
                },
            )
            .unwrap();
        application
            .resize_musical_midi_clip(&clip_id, Some("1:2".parse().unwrap()), None)
            .unwrap();

        let fetched = application.get_musical_note(&clip_id, &note_id).unwrap();
        assert_eq!(fetched.position.to_string(), "3:1");
        assert_eq!(fetched.duration.to_string(), "1/2");
        application.remove_musical_note(&clip_id, &note_id).unwrap();
        assert!(application.get_musical_note(&clip_id, &note_id).is_err());
    }

    #[test]
    fn track_note_query_and_transform_are_grouped_deterministic_and_atomic() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        let track_id = application
            .add_track_with_created_ids("Drums", TrackKind::Instrument)
            .unwrap()
            .session
            .arrangement
            .tracks[0]
            .id
            .clone();
        let first_clip = application
            .create_musical_midi_clip_with_created_ids(
                &track_id,
                "1:1".parse().unwrap(),
                "5:1".parse().unwrap(),
                None,
            )
            .unwrap()
            .session
            .arrangement
            .midi_clips[0]
            .id
            .clone();
        let second_clip = application
            .create_musical_midi_clip_with_created_ids(
                &track_id,
                "5:1".parse().unwrap(),
                "9:1".parse().unwrap(),
                None,
            )
            .unwrap()
            .session
            .arrangement
            .midi_clips
            .iter()
            .find(|clip| clip.id != first_clip)
            .unwrap()
            .id
            .clone();
        application
            .insert_musical_notes_with_created_ids(
                &first_clip,
                vec![
                    MusicalMidiNoteInput {
                        pitch: "C4".parse().unwrap(),
                        position: "2:1".parse().unwrap(),
                        duration: "1/4".parse().unwrap(),
                        velocity: Some(20),
                        channel: Some(1),
                    },
                    MusicalMidiNoteInput {
                        pitch: "C4".parse().unwrap(),
                        position: "4:4".parse().unwrap(),
                        duration: "1/4".parse().unwrap(),
                        velocity: Some(30),
                        channel: Some(1),
                    },
                ],
            )
            .unwrap();
        application
            .insert_musical_notes_with_created_ids(
                &second_clip,
                vec![MusicalMidiNoteInput {
                    pitch: "D2".parse().unwrap(),
                    position: "6:1".parse().unwrap(),
                    duration: "1/4".parse().unwrap(),
                    velocity: Some(126),
                    channel: Some(1),
                }],
            )
            .unwrap();

        let listed = application
            .list_musical_notes(MusicalNoteListRequest {
                scope: MusicalNoteScope {
                    clip_id: None,
                    track_id: Some(track_id.clone()),
                },
                start: Some("5:1".parse().unwrap()),
                end: Some("7:1".parse().unwrap()),
                include_ids: false,
                raw: false,
            })
            .unwrap();
        assert_eq!(listed.count, 1);
        assert_eq!(listed.clips[0].clip_id, second_clip);
        let MusicalNoteListNoteView::Musical(note) = &listed.clips[0].notes[0] else {
            panic!("expected a musical Note");
        };
        assert_eq!(note.id, None);
        assert_eq!(note.pitch.to_string(), "D2");

        let raw = application
            .list_musical_notes(MusicalNoteListRequest {
                scope: MusicalNoteScope {
                    clip_id: Some(second_clip.clone()),
                    track_id: None,
                },
                start: None,
                end: None,
                include_ids: true,
                raw: true,
            })
            .unwrap();
        assert_eq!(raw.timebase.unwrap().ppq, 960);
        assert_eq!(raw.clips[0].start_tick, Some(15_360));
        let MusicalNoteListNoteView::Raw(note) = &raw.clips[0].notes[0] else {
            panic!("expected a raw Note");
        };
        assert_eq!(note.start_tick, 3_840);
        assert!(note.id.is_some());

        application
            .transform_musical_notes(MusicalNoteTransformRequest {
                scope: MusicalNoteScope {
                    clip_id: Some(second_clip.clone()),
                    track_id: None,
                },
                start: Some("5:1".parse().unwrap()),
                end: Some("7:1".parse().unwrap()),
                pitch: Some("D2".parse().unwrap()),
                channel: Some(1),
                timing_offset: Some("+1/48".parse().unwrap()),
                velocity_offset: Some(4),
                transpose_semitones: None,
            })
            .unwrap();
        let transformed = core.canonical_state().unwrap();
        let transformed_note = &transformed.session.arrangement.midi_clips[1].notes[0];
        assert_eq!(transformed_note.start_tick, TimelineTick(3_920));
        assert_eq!(transformed_note.velocity, 127);

        assert!(
            application
                .transform_musical_notes(MusicalNoteTransformRequest {
                    scope: MusicalNoteScope {
                        clip_id: Some(first_clip),
                        track_id: None,
                    },
                    start: Some("1:1".parse().unwrap()),
                    end: Some("5:1".parse().unwrap()),
                    pitch: Some("C4".parse().unwrap()),
                    channel: None,
                    timing_offset: Some("+1/48".parse().unwrap()),
                    velocity_offset: None,
                    transpose_semitones: None,
                })
                .is_err()
        );
        let unchanged = core.canonical_state().unwrap();
        assert_eq!(
            unchanged.session.arrangement.midi_clips[0].notes[1].start_tick,
            TimelineTick(14_400)
        );
    }

    #[test]
    fn musical_clip_and_region_ranges_require_positive_duration() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        let track = application
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.session.arrangement.tracks[0].id.clone();
        assert!(
            application
                .create_musical_midi_clip_with_created_ids(
                    &track_id,
                    "5:2".parse().unwrap(),
                    "5:1".parse().unwrap(),
                    None,
                )
                .is_err()
        );
        assert!(
            application
                .add_region_with_created_ids(
                    " ".into(),
                    "1:1".parse().unwrap(),
                    "2:1".parse().unwrap()
                )
                .is_err()
        );
        assert!(
            application
                .add_region_with_created_ids(
                    "A'".into(),
                    "1:1".parse().unwrap(),
                    "2:1".parse().unwrap()
                )
                .is_ok()
        );
        let regions = application.list_regions().unwrap();
        assert_eq!(regions[0].name, "A'");
    }
}
