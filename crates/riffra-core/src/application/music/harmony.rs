//! Harmony application operations.

use super::{
    ResolvedMidiNoteInput, available_midi_note_capacity, insert_resolved_midi_notes_in_arrangement,
    push_resolved_midi_note, repeated_offset_to_ticks,
};
use crate::application::Application;
use crate::domain::{
    HarmonyChord, HarmonyEvent, MusicalNoteName, MusicalPosition, RhythmPattern, TimelineTick,
};
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;
use serde::{Deserialize, Serialize};

/// Input for one harmony event, using either a chord symbol or explicit tones.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyEventInput {
    pub start: MusicalPosition,
    pub end: MusicalPosition,
    pub chord: Option<String>,
    pub pitches: Option<Vec<MusicalNoteName>>,
    pub root: Option<MusicalNoteName>,
    pub bass: Option<MusicalNoteName>,
    pub label: Option<String>,
}

/// Partial update for one canonical harmony event.
///
/// A chord symbol and explicit tone fields are alternative complete chord
/// definitions. Explicit fields are never inherited from the current chord.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyEventPatch {
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    pub chord: Option<String>,
    pub pitches: Option<Vec<MusicalNoteName>>,
    pub root: Option<MusicalNoteName>,
    pub bass: Option<MusicalNoteName>,
    pub label: Option<String>,
}

/// The lowest octave used by deterministic chord realization.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordVoicingInput {
    pub lowest_octave: i8,
}

impl Default for ChordVoicingInput {
    fn default() -> Self {
        Self { lowest_octave: 3 }
    }
}

/// Optional absolute range restricting harmony realization.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyRealizeSelection {
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
}

/// A harmony event represented in musical coordinates for read operations.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalHarmonyEventView {
    pub id: String,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
    pub chord: HarmonyChord,
}

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Resolves a chord symbol without changing the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the symbol is not accepted by the harmony
    /// resolver.
    pub fn resolve_harmony_chord(&self, symbol: &str) -> Result<HarmonyChord, ApplicationError> {
        HarmonyChord::resolve(symbol).map_err(Into::into)
    }

    /// Inserts one or more harmony events as one canonical edit.
    ///
    /// # Errors
    ///
    /// Returns an error when an input is invalid or the canonical commit
    /// cannot be persisted.
    pub fn insert_harmony_events(
        &self,
        inputs: Vec<HarmonyEventInput>,
    ) -> Result<crate::domain::CreativeSession, ApplicationError> {
        if inputs.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one harmony event is required".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let timebase = arrangement.timebase;
            let events = inputs
                .into_iter()
                .map(|input| {
                    let start_tick = timebase.musical_position_to_tick(input.start)?;
                    let end_tick = timebase.musical_position_to_tick(input.end)?;
                    Ok(HarmonyEvent {
                        id: super::next_id("harmony"),
                        start_tick,
                        end_tick,
                        chord: resolve_harmony_input(input)?,
                    })
                })
                .collect::<Result<Vec<_>, crate::DomainError>>()?;
            arrangement.add_harmony_events(events).map_err(Into::into)
        })
    }

    /// Updates one harmony event and re-resolves a changed chord definition.
    ///
    /// # Errors
    ///
    /// Returns an error when the event or patch is invalid, or the canonical
    /// commit cannot be persisted.
    pub fn update_harmony_event(
        &self,
        event_id: &str,
        patch: HarmonyEventPatch,
    ) -> Result<crate::domain::CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let current = arrangement
                .harmony_events
                .iter()
                .find(|event| event.id == event_id)
                .cloned()
                .ok_or_else(|| {
                    crate::DomainError::InvalidHarmony(format!(
                        "harmony event '{event_id}' is not registered"
                    ))
                })?;
            let HarmonyEventPatch {
                start,
                end,
                chord,
                pitches,
                root,
                bass,
                label,
            } = patch;
            let chord = match (chord, pitches) {
                (Some(symbol), None) => {
                    if root.is_some() || bass.is_some() || label.is_some() {
                        return Err(crate::DomainError::InvalidHarmony(
                            "a chord symbol cannot include explicit harmony fields".into(),
                        )
                        .into());
                    }
                    HarmonyChord::resolve(&symbol)?
                }
                (None, Some(pitches)) => HarmonyChord::from_explicit(pitches, root, bass, label)?,
                (Some(_), Some(_)) => {
                    return Err(crate::DomainError::InvalidHarmony(
                        "a harmony patch must use either a chord symbol or explicit tones".into(),
                    )
                    .into());
                }
                (None, None) => {
                    if root.is_some() || bass.is_some() || label.is_some() {
                        return Err(crate::DomainError::InvalidHarmony(
                            "explicit harmony fields require pitches".into(),
                        )
                        .into());
                    }
                    current.chord.clone()
                }
            };
            let timebase = arrangement.timebase;
            let event = HarmonyEvent {
                id: event_id.to_owned(),
                start_tick: start
                    .map(|position| timebase.musical_position_to_tick(position))
                    .transpose()?
                    .unwrap_or(current.start_tick),
                end_tick: end
                    .map(|position| timebase.musical_position_to_tick(position))
                    .transpose()?
                    .unwrap_or(current.end_tick),
                chord,
            };
            arrangement.update_harmony_event(event_id, event)?;
            Ok(())
        })
    }

    /// Removes one or more harmony events as one canonical edit.
    ///
    /// # Errors
    ///
    /// Returns an error when an id is empty, duplicated, or unknown, or when
    /// the canonical commit cannot be persisted.
    pub fn remove_harmony_events(
        &self,
        event_ids: Vec<String>,
    ) -> Result<crate::domain::CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .remove_harmony_events(event_ids)
                .map_err(Into::into)
        })
    }

    /// Lists harmony events in musical coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error when the canonical session cannot be read.
    pub fn list_harmony_events(&self) -> Result<Vec<MusicalHarmonyEventView>, ApplicationError> {
        let session = self.get_session()?;
        let timebase = session.arrangement.timebase;
        Ok(session
            .arrangement
            .harmony_events
            .into_iter()
            .map(|event| MusicalHarmonyEventView {
                id: event.id,
                start: timebase.tick_to_musical_position(event.start_tick),
                end: timebase.tick_to_musical_position(event.end_tick),
                chord: event.chord,
            })
            .collect())
    }

    /// Realizes selected harmony events into MIDI notes in one canonical edit.
    ///
    /// # Errors
    ///
    /// Returns an error when the clip, selection, voicing, rhythm, or generated
    /// notes are invalid, or when the canonical commit cannot be persisted.
    pub fn realize_harmony(
        &self,
        clip_id: &str,
        selection: HarmonyRealizeSelection,
        voicing: ChordVoicingInput,
        rhythm: Option<RhythmPattern>,
        velocity: Option<u8>,
        channel: Option<u8>,
    ) -> Result<crate::domain::CreativeSession, ApplicationError> {
        if velocity.is_some_and(|value| value > 127) {
            return Err(ApplicationError::InvalidCommand(
                "harmony velocity must be between 0 and 127".into(),
            ));
        }
        if channel.is_some_and(|value| !(1..=16).contains(&value)) {
            return Err(ApplicationError::InvalidCommand(
                "harmony channel must be between 1 and 16".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            let clip_start = clip.start_tick;
            let clip_end = TimelineTick(clip_start.0.checked_add(clip.duration_ticks).ok_or_else(
                || crate::DomainError::InvalidHarmony("clip range is too large".into()),
            )?);
            let available_notes = available_midi_note_capacity(arrangement, clip_id)?;
            let timebase = arrangement.timebase;
            let start_tick = selection
                .start
                .map(|position| timebase.musical_position_to_tick(position))
                .transpose()?
                .unwrap_or(clip_start);
            let end_tick = selection
                .end
                .map(|position| timebase.musical_position_to_tick(position))
                .transpose()?
                .unwrap_or(clip_end);
            if end_tick <= start_tick {
                return Err(crate::DomainError::InvalidHarmony(
                    "harmony realization selection must have a positive range".into(),
                )
                .into());
            }
            let mut events = arrangement
                .harmony_events
                .iter()
                .filter(|event| event.start_tick >= start_tick && event.start_tick < end_tick)
                .cloned()
                .collect::<Vec<_>>();
            events.sort_by_key(|event| (event.start_tick, event.end_tick, event.id.clone()));
            if events.is_empty() {
                return Err(crate::DomainError::InvalidHarmony(
                    "no harmony events fall within the realization selection".into(),
                )
                .into());
            }
            let mut pattern = rhythm;
            if let Some(pattern) = &mut pattern {
                pattern.validate_and_normalize()?;
            }
            let mut notes = Vec::new();
            for event in events {
                let pitches = realize_chord(&event.chord, voicing.lowest_octave)?;
                let event_duration = event
                    .end_tick
                    .0
                    .checked_sub(event.start_tick.0)
                    .ok_or_else(|| {
                        crate::DomainError::InvalidHarmony("harmony event range is invalid".into())
                    })?;
                if let Some(pattern) = &pattern {
                    let mut repeat = 0_u64;
                    loop {
                        let mut added_step = false;
                        for step in &pattern.steps {
                            let offset = repeated_offset_to_ticks(
                                timebase,
                                pattern.length,
                                repeat,
                                step.offset,
                            )?;
                            let onset =
                                event.start_tick.0.checked_add(offset).ok_or_else(|| {
                                    crate::DomainError::InvalidHarmony(
                                        "rhythm onset is too large".into(),
                                    )
                                })?;
                            if onset >= event.end_tick.0 {
                                continue;
                            }
                            added_step = true;
                            let duration = timebase.musical_duration_to_ticks(step.duration)?;
                            for pitch in &pitches {
                                push_resolved_midi_note(
                                    &mut notes,
                                    available_notes,
                                    ResolvedMidiNoteInput {
                                        pitch: pitch.midi_pitch(),
                                        absolute_start_tick: TimelineTick(onset),
                                        duration_ticks: duration,
                                        velocity: step.velocity.or(velocity).unwrap_or(100),
                                        channel: channel.unwrap_or(1),
                                    },
                                )?;
                            }
                        }
                        if !added_step {
                            break;
                        }
                        repeat = repeat.saturating_add(1);
                    }
                } else {
                    for pitch in &pitches {
                        push_resolved_midi_note(
                            &mut notes,
                            available_notes,
                            ResolvedMidiNoteInput {
                                pitch: pitch.midi_pitch(),
                                absolute_start_tick: event.start_tick,
                                duration_ticks: event_duration,
                                velocity: velocity.unwrap_or(100),
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

fn resolve_harmony_input(input: HarmonyEventInput) -> Result<HarmonyChord, crate::DomainError> {
    match (input.chord, input.pitches) {
        (Some(symbol), None) => {
            if input.root.is_some() || input.bass.is_some() || input.label.is_some() {
                return Err(crate::DomainError::InvalidHarmony(
                    "a chord symbol cannot include explicit harmony fields".into(),
                ));
            }
            HarmonyChord::resolve(&symbol)
        }
        (None, Some(pitches)) => {
            HarmonyChord::from_explicit(pitches, input.root, input.bass, input.label)
        }
        (Some(_), Some(_)) => Err(crate::DomainError::InvalidHarmony(
            "harmony input must use either a chord symbol or explicit tones".into(),
        )),
        (None, None) => Err(crate::DomainError::InvalidHarmony(
            "harmony input requires a chord symbol or explicit tones".into(),
        )),
    }
}

fn realize_chord(
    chord: &HarmonyChord,
    lowest_octave: i8,
) -> Result<Vec<crate::domain::MusicalPitch>, crate::DomainError> {
    if !(-1..=8).contains(&lowest_octave) {
        return Err(crate::DomainError::InvalidHarmony(
            "lowest octave must be between -1 and 8".into(),
        ));
    }
    let mut ordered = Vec::with_capacity(chord.tones.len() + 1);
    if let Some(bass) = chord.bass {
        if let Some(index) = chord
            .tones
            .iter()
            .position(|tone| tone.pitch_class() == bass.pitch_class())
        {
            ordered.push(bass);
            ordered.extend(chord.tones.iter().skip(index + 1).copied());
            ordered.extend(chord.tones.iter().take(index).copied());
        } else {
            ordered.push(bass);
            ordered.extend(chord.tones.iter().copied());
        }
    } else {
        ordered.extend(chord.tones.iter().copied());
    }

    let mut realized = Vec::with_capacity(ordered.len());
    let mut previous = None;
    for note in ordered {
        let mut octave = lowest_octave;
        loop {
            let pitch = note.with_octave(octave)?;
            if previous.is_none_or(|previous: crate::domain::MusicalPitch| {
                pitch.midi_pitch() > previous.midi_pitch()
            }) {
                previous = Some(pitch);
                realized.push(pitch);
                break;
            }
            octave = octave.checked_add(1).ok_or_else(|| {
                crate::DomainError::InvalidHarmony("chord voicing exceeds MIDI range".into())
            })?;
        }
    }
    Ok(realized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PortError;
    use crate::app::AppCore;
    use crate::domain::{CreativeSession, RhythmPattern, RhythmStep, TrackKind};
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
    fn harmony_events_are_inserted_as_one_canonical_edit_and_listed_musically() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        application
            .insert_harmony_events(vec![
                HarmonyEventInput {
                    start: "2:1".parse().unwrap(),
                    end: "3:1".parse().unwrap(),
                    chord: Some("G7(b9,#11)".into()),
                    pitches: None,
                    root: None,
                    bass: None,
                    label: None,
                },
                HarmonyEventInput {
                    start: "1:1".parse().unwrap(),
                    end: "2:1".parse().unwrap(),
                    chord: None,
                    pitches: Some(vec!["Bb".parse().unwrap(), "C".parse().unwrap()]),
                    root: None,
                    bass: Some("F".parse().unwrap()),
                    label: Some("cluster".into()),
                },
            ])
            .unwrap();

        assert_eq!(storage.0.lock().unwrap().len(), 1);
        let events = application.list_harmony_events().unwrap();
        assert_eq!(events[0].start.to_string(), "1:1");
        assert_eq!(events[1].start.to_string(), "2:1");
        assert_eq!(events[1].chord.name, "G7b9#11");

        let updated = application
            .update_harmony_event(
                &events[0].id,
                HarmonyEventPatch {
                    chord: Some("Dm9".into()),
                    ..HarmonyEventPatch::default()
                },
            )
            .unwrap();
        assert_eq!(
            updated.arrangement.harmony_events[0].chord.tones,
            ["D", "F", "A", "C", "E"]
                .into_iter()
                .map(|value| value.parse().unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(storage.0.lock().unwrap().len(), 2);

        assert!(
            application
                .remove_harmony_events(vec![events[0].id.clone(), "missing".into()])
                .is_err()
        );
        assert_eq!(application.list_harmony_events().unwrap().len(), 2);
    }

    #[test]
    fn harmony_definition_patch_replaces_explicit_fields_without_inheriting_them() {
        let storage = MemoryStorage::default();
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            (),
            false,
            false,
        );
        let application = core.application(&storage);
        let inserted = application
            .insert_harmony_events(vec![HarmonyEventInput {
                start: "1:1".parse().unwrap(),
                end: "2:1".parse().unwrap(),
                chord: Some("C/E".into()),
                pitches: None,
                root: None,
                bass: None,
                label: None,
            }])
            .unwrap();
        let event_id = inserted.arrangement.harmony_events[0].id.clone();

        let updated = application
            .update_harmony_event(
                &event_id,
                HarmonyEventPatch {
                    pitches: Some(vec![
                        "D".parse().unwrap(),
                        "F".parse().unwrap(),
                        "A".parse().unwrap(),
                    ]),
                    ..HarmonyEventPatch::default()
                },
            )
            .unwrap();
        let chord = &updated.arrangement.harmony_events[0].chord;
        assert_eq!(chord.name, "D F A");
        assert_eq!(chord.root, None);
        assert_eq!(chord.bass, None);
        assert!(
            application
                .update_harmony_event(
                    &event_id,
                    HarmonyEventPatch {
                        root: Some("D".parse().unwrap()),
                        ..HarmonyEventPatch::default()
                    },
                )
                .is_err()
        );
    }

    #[test]
    fn rhythm_pattern_repeats_inside_a_harmony_event() {
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
        application
            .insert_harmony_events(vec![HarmonyEventInput {
                start: "1:1".parse().unwrap(),
                end: "2:1".parse().unwrap(),
                chord: Some("C".into()),
                pitches: None,
                root: None,
                bass: None,
                label: None,
            }])
            .unwrap();

        let realized = application
            .realize_harmony(
                &clip_id,
                HarmonyRealizeSelection::default(),
                ChordVoicingInput::default(),
                Some(
                    RhythmPattern::new(
                        "1/2".parse().unwrap(),
                        vec![
                            RhythmStep {
                                offset: "0/1".parse().unwrap(),
                                duration: "1/8".parse().unwrap(),
                                velocity: Some(80),
                            },
                            RhythmStep {
                                offset: "1/4".parse().unwrap(),
                                duration: "1/8".parse().unwrap(),
                                velocity: None,
                            },
                        ],
                    )
                    .unwrap(),
                ),
                None,
                None,
            )
            .unwrap();

        let notes = &realized.arrangement.midi_clips[0].notes;
        assert_eq!(notes.len(), 12);
        assert_eq!(
            notes
                .iter()
                .map(|note| note.start_tick.0)
                .collect::<Vec<_>>(),
            [
                0, 0, 0, 960, 960, 960, 1_920, 1_920, 1_920, 2_880, 2_880, 2_880
            ]
        );
        assert_eq!(notes[0].velocity, 80);
        assert_eq!(notes[3].velocity, 100);
    }

    #[test]
    fn realization_places_slash_bass_first_and_uses_shared_note_insertion_rules() {
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
        let track_id = track.arrangement.tracks[0].id.clone();
        let clip = application
            .create_musical_midi_clip(
                &track_id,
                "1:1".parse().unwrap(),
                "2:1".parse().unwrap(),
                None,
            )
            .unwrap();
        let clip_id = clip.arrangement.midi_clips[0].id.clone();
        application
            .insert_harmony_events(vec![HarmonyEventInput {
                start: "1:1".parse().unwrap(),
                end: "3:1".parse().unwrap(),
                chord: Some("C/E".into()),
                pitches: None,
                root: None,
                bass: None,
                label: None,
            }])
            .unwrap();
        let realized = application
            .realize_harmony(
                &clip_id,
                HarmonyRealizeSelection::default(),
                ChordVoicingInput::default(),
                None,
                None,
                None,
            )
            .unwrap();
        let notes = &realized.arrangement.midi_clips[0].notes;
        assert_eq!(
            notes.iter().map(|note| note.note).collect::<Vec<_>>(),
            [52, 55, 60]
        );
        assert_eq!(realized.arrangement.midi_clips[0].duration_ticks, 7_680);
        assert_eq!(storage.0.lock().unwrap().len(), 4);
    }
}
