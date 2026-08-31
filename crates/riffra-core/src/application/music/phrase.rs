//! Phrase application operations.

use super::{
    ResolvedMidiNoteInput, available_midi_note_capacity, insert_resolved_midi_notes_in_arrangement,
    push_resolved_midi_note, repeated_offset_to_ticks,
};
use crate::application::Application;
use crate::domain::{PhrasePattern, PhrasePlacement, TimelineTick};
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Expands a relative phrase at one or more absolute placements.
    ///
    /// # Errors
    ///
    /// Returns an error when the pattern, placement, channel, or generated
    /// pitch is invalid, or when the canonical commit cannot be persisted.
    pub fn insert_phrase_pattern(
        &self,
        clip_id: &str,
        mut pattern: PhrasePattern,
        placements: Vec<PhrasePlacement>,
        channel: Option<u8>,
    ) -> Result<crate::domain::CreativeSession, ApplicationError> {
        if placements.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one phrase placement is required".into(),
            ));
        }
        if channel.is_some_and(|value| !(1..=16).contains(&value)) {
            return Err(ApplicationError::InvalidCommand(
                "phrase channel must be between 1 and 16".into(),
            ));
        }
        pattern.validate_and_normalize()?;
        for placement in &placements {
            placement.validate()?;
        }
        self.commit_arrangement(|arrangement| {
            let timebase = arrangement.timebase;
            let available_notes = available_midi_note_capacity(arrangement, clip_id)?;
            let mut notes = Vec::new();
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
                            )
                            .into());
                        }
                        push_resolved_midi_note(
                            &mut notes,
                            available_notes,
                            ResolvedMidiNoteInput {
                                pitch: u8::try_from(pitch).expect("pitch was checked above"),
                                absolute_start_tick: TimelineTick(onset),
                                duration_ticks: timebase
                                    .musical_duration_to_ticks(phrase_note.duration)?,
                                velocity: phrase_note.velocity.unwrap_or(100),
                                channel: channel.unwrap_or(1),
                            },
                        )?;
                    }
                }
            }
            insert_resolved_midi_notes_in_arrangement(arrangement, clip_id, notes)
                .map_err(Into::into)
        })
    }
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
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let clip = application
            .create_musical_midi_clip(
                &track.arrangement.tracks[0].id,
                "1:1".parse().unwrap(),
                "2:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.arrangement.midi_clips[0].id.clone();
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
            .insert_phrase_pattern(
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

        let notes = &inserted.arrangement.midi_clips[0].notes;
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
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let clip = application
            .create_musical_midi_clip(
                &track.arrangement.tracks[0].id,
                "1:1".parse().unwrap(),
                "4:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.arrangement.midi_clips[0].id.clone();
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
            .insert_phrase_pattern(
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
            inserted.arrangement.midi_clips[0]
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
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let clip = setup_application
            .create_musical_midi_clip(
                &track.arrangement.tracks[0].id,
                "1:1".parse().unwrap(),
                "2:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let mut session = clip.clone();
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
                .insert_phrase_pattern(
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
