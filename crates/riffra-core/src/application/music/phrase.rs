//! Phrase application operations.

use super::{
    ResolvedMidiNoteInput, available_midi_note_capacity, insert_resolved_midi_notes_in_arrangement,
    push_resolved_midi_note, repeated_offset_to_ticks,
};
use crate::application::Application;
use crate::domain::{Arrangement, PhrasePattern, PhrasePlacement, TimelineTick};
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;

/// One identity-free MIDI note resolved from a phrase pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPhraseNote {
    /// MIDI pitch.
    pub pitch: u8,
    /// Absolute project start tick.
    pub start_tick: TimelineTick,
    /// Duration in project ticks.
    pub duration_ticks: u64,
    /// MIDI velocity.
    pub velocity: u8,
    /// MIDI channel.
    pub channel: u8,
}

/// The validated expansion shared by phrase preview and insertion.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPhrase {
    /// Expanded notes without canonical identities.
    pub notes: Vec<ResolvedPhraseNote>,
    /// Number of placements used for the expansion.
    pub placement_count: usize,
    /// Earliest generated note onset.
    pub start_tick: TimelineTick,
    /// End of the latest generated note.
    pub end_tick: TimelineTick,
}

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Expands a relative phrase at one or more absolute placements and
    /// returns the generated Note IDs.
    ///
    /// # Errors
    ///
    /// Returns an error when the pattern, placement, channel, or generated
    /// pitch is invalid, or when the canonical commit cannot be persisted.
    pub fn insert_phrase_pattern_with_created_ids(
        &self,
        clip_id: &str,
        pattern: PhrasePattern,
        placements: Vec<PhrasePlacement>,
        channel: Option<u8>,
    ) -> Result<super::ApplicationMutation, ApplicationError> {
        let mut created_entity_ids = super::CreatedEntityIds::new();
        let session = self.commit_arrangement(|arrangement| {
            let resolved = resolve_phrase_pattern_in_arrangement(
                arrangement,
                clip_id,
                pattern.clone(),
                placements.clone(),
                channel,
            )?;
            let notes = resolved
                .notes
                .into_iter()
                .map(|note| ResolvedMidiNoteInput {
                    pitch: note.pitch,
                    absolute_start_tick: note.start_tick,
                    duration_ticks: note.duration_ticks,
                    velocity: note.velocity,
                    channel: note.channel,
                })
                .collect();
            let ids = insert_resolved_midi_notes_in_arrangement(arrangement, clip_id, notes)?;
            for id in ids {
                super::record_created(&mut created_entity_ids, "midiNotes", id);
            }
            Ok(())
        })?;
        Ok(super::ApplicationMutation::new(session, created_entity_ids))
    }

    /// Resolves a phrase without changing canonical state.
    ///
    /// # Errors
    /// Returns an error when the pattern, placement, Clip, generated note, or
    /// Clip capacity is invalid.
    pub fn resolve_phrase_pattern(
        &self,
        clip_id: &str,
        pattern: PhrasePattern,
        placements: Vec<PhrasePlacement>,
        channel: Option<u8>,
    ) -> Result<ResolvedPhrase, ApplicationError> {
        let session = self.get_session()?;
        resolve_phrase_pattern_in_arrangement(
            &session.arrangement,
            clip_id,
            pattern,
            placements,
            channel,
        )
        .map_err(Into::into)
    }
}

fn resolve_phrase_pattern_in_arrangement(
    arrangement: &Arrangement,
    clip_id: &str,
    mut pattern: PhrasePattern,
    placements: Vec<PhrasePlacement>,
    channel: Option<u8>,
) -> Result<ResolvedPhrase, crate::DomainError> {
    if placements.is_empty() {
        return Err(crate::DomainError::InvalidMusicalValue(
            "at least one phrase placement is required".into(),
        ));
    }
    if channel.is_some_and(|value| !(1..=16).contains(&value)) {
        return Err(crate::DomainError::InvalidMusicalValue(
            "phrase channel must be between 1 and 16".into(),
        ));
    }
    pattern.validate_and_normalize()?;
    for placement in &placements {
        placement.validate()?;
    }
    let placement_count = placements.len();

    let clip = arrangement
        .midi_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| {
            crate::DomainError::InvalidClip(format!("midi clip '{clip_id}' is not registered"))
        })?;
    let available_notes = available_midi_note_capacity(arrangement, clip_id)?;
    let timebase = arrangement.timebase;
    let mut notes = Vec::new();
    let mut start_tick: Option<u64> = None;
    let mut end_tick: Option<u64> = None;
    for placement in placements {
        let placement_tick = timebase.musical_position_to_tick(placement.position)?;
        for repeat in 0..placement.repeats {
            for phrase_note in &pattern.notes {
                let note_offset = repeated_offset_to_ticks(
                    timebase,
                    pattern.length,
                    u64::from(repeat),
                    phrase_note.offset,
                )?;
                let onset = placement_tick.0.checked_add(note_offset).ok_or_else(|| {
                    crate::DomainError::InvalidMusicalValue(
                        "phrase note position is too large".into(),
                    )
                })?;
                let pitch = i16::from(placement.anchor.midi_pitch())
                    .checked_add(phrase_note.semitones)
                    .ok_or_else(|| {
                        crate::DomainError::InvalidMusicalValue(
                            "phrase pitch is outside the MIDI range".into(),
                        )
                    })?;
                if !(0..=127).contains(&pitch) {
                    return Err(crate::DomainError::InvalidMusicalValue(
                        "phrase pitch is outside the MIDI range".into(),
                    ));
                }
                let duration_ticks = timebase.musical_duration_to_ticks(phrase_note.duration)?;
                let relative_start = onset.checked_sub(clip.start_tick.0).ok_or_else(|| {
                    crate::DomainError::InvalidMusicalValue(
                        "phrase note position must not precede the MIDI clip".into(),
                    )
                })?;
                let relative_end = relative_start.checked_add(duration_ticks).ok_or_else(|| {
                    crate::DomainError::InvalidMusicalValue(
                        "phrase note exceeds the MIDI clip".into(),
                    )
                })?;
                if relative_end > clip.duration_ticks {
                    return Err(crate::DomainError::InvalidMusicalValue(
                        "phrase note exceeds the MIDI clip".into(),
                    ));
                }
                push_resolved_midi_note(
                    &mut notes,
                    available_notes,
                    ResolvedMidiNoteInput {
                        pitch: u8::try_from(pitch).expect("pitch was checked above"),
                        absolute_start_tick: TimelineTick(onset),
                        duration_ticks,
                        velocity: phrase_note.velocity.unwrap_or(100),
                        channel: channel.unwrap_or(1),
                    },
                )?;
                start_tick = Some(start_tick.map_or(onset, |start| start.min(onset)));
                let note_end = onset.checked_add(duration_ticks).ok_or_else(|| {
                    crate::DomainError::InvalidMusicalValue(
                        "phrase note position is too large".into(),
                    )
                })?;
                end_tick = Some(end_tick.map_or(note_end, |end| end.max(note_end)));
            }
        }
    }
    let start_tick = start_tick.expect("a non-empty pattern and placement produce a note");
    Ok(ResolvedPhrase {
        notes: notes
            .into_iter()
            .map(|note| ResolvedPhraseNote {
                pitch: note.pitch,
                start_tick: note.absolute_start_tick,
                duration_ticks: note.duration_ticks,
                velocity: note.velocity,
                channel: note.channel,
            })
            .collect(),
        placement_count,
        start_tick: TimelineTick(start_tick),
        end_tick: TimelineTick(end_tick.expect("a generated note has an end")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PortError;
    use crate::app::AppCore;
    use crate::domain::{CreativeSession, MidiNote, PhraseNote, TrackKind};
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
    fn phrase_pattern_expands_placements_and_repeats_in_one_commit() {
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
                "2:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.session.arrangement.midi_clips[0].id.clone();
        let pattern = PhrasePattern::new(
            "1/4".parse().unwrap(),
            vec![
                PhraseNote {
                    offset: "0/1".parse().unwrap(),
                    duration: "1/8".parse().unwrap(),
                    semitones: 0,
                    velocity: None,
                },
                PhraseNote {
                    offset: "1/8".parse().unwrap(),
                    duration: "1/8".parse().unwrap(),
                    semitones: -2,
                    velocity: Some(90),
                },
            ],
        )
        .unwrap();
        let inserted = application
            .insert_phrase_pattern_with_created_ids(
                &clip_id,
                pattern,
                vec![PhrasePlacement {
                    position: "1:1".parse().unwrap(),
                    anchor: "C4".parse().unwrap(),
                    repeats: 2,
                }],
                None,
            )
            .unwrap();

        let notes = &inserted.session.arrangement.midi_clips[0].notes;
        assert_eq!(notes.len(), 4);
        assert_eq!(
            notes.iter().map(|note| note.note).collect::<Vec<_>>(),
            [60, 58, 60, 58]
        );
        assert_eq!(notes[2].start_tick, TimelineTick(960));
        assert_eq!(storage.0.lock().unwrap().len(), 3);
    }

    #[test]
    fn phrase_repeats_round_the_absolute_rational_offset_once() {
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
                "4:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.session.arrangement.midi_clips[0].id.clone();
        let pattern = PhrasePattern::new(
            "1/7".parse().unwrap(),
            vec![PhraseNote {
                offset: "0/1".parse().unwrap(),
                duration: "1/16".parse().unwrap(),
                semitones: 0,
                velocity: None,
            }],
        )
        .unwrap();

        let inserted = application
            .insert_phrase_pattern_with_created_ids(
                &clip_id,
                pattern,
                vec![PhrasePlacement {
                    position: "1:1".parse().unwrap(),
                    anchor: "C4".parse().unwrap(),
                    repeats: 8,
                }],
                None,
            )
            .unwrap();

        assert_eq!(
            inserted.session.arrangement.midi_clips[0]
                .notes
                .iter()
                .map(|note| note.start_tick.0)
                .collect::<Vec<_>>(),
            [0, 549, 1_097, 1_646, 2_194, 2_743, 3_291, 3_840]
        );
    }

    #[test]
    fn phrase_generation_stops_at_the_canonical_note_limit() {
        let setup_storage = MemoryStorage::default();
        let setup_core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let setup_application = setup_core.application(&setup_storage);
        let track = setup_application
            .add_track_with_created_ids("Keys", TrackKind::Instrument)
            .unwrap();
        let clip = setup_application
            .create_musical_midi_clip_with_created_ids(
                &track.session.arrangement.tracks[0].id,
                "1:1".parse().unwrap(),
                "2:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let mut session = clip.session.clone();
        let midi_clip = &mut session.arrangement.midi_clips[0];
        midi_clip.notes = (0..crate::domain::arrangement::MAX_MIDI_NOTES_PER_CLIP - 1)
            .map(|index| MidiNote {
                id: format!("note:{index}"),
                note: 60,
                start_tick: TimelineTick(0),
                duration_ticks: 1,
                velocity: 100,
                channel: 1,
            })
            .collect();
        let clip_id = midi_clip.id.clone();
        let storage = MemoryStorage::default();
        let core = AppCore::new(PathBuf::from("data"), session, (), false, false);
        let application = core.application(&storage);
        let pattern = PhrasePattern::new(
            "1/4".parse().unwrap(),
            vec![PhraseNote {
                offset: "0/1".parse().unwrap(),
                duration: "1/16".parse().unwrap(),
                semitones: 0,
                velocity: None,
            }],
        )
        .unwrap();

        assert!(
            application
                .insert_phrase_pattern_with_created_ids(
                    &clip_id,
                    pattern,
                    vec![PhrasePlacement {
                        position: "1:1".parse().unwrap(),
                        anchor: "C4".parse().unwrap(),
                        repeats: 2,
                    }],
                    None,
                )
                .is_err()
        );
    }
}
