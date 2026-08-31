//! Music-oriented application operations over the canonical arrangement.

mod harmony;
mod phrase;

pub use harmony::{
    ChordVoicingInput, HarmonyEventInput, HarmonyEventPatch, HarmonyRealizeSelection,
    MusicalHarmonyEventView,
};

use super::*;
use crate::DomainError;
use crate::domain::{
    MidiNote, MusicalDuration, MusicalPitch, MusicalPosition, TimelineRegion, TimelineTick,
};
use serde::{Deserialize, Serialize};

/// A MIDI note described using musical position, duration, and pitch values.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMidiNoteInput {
    pub pitch: MusicalPitch,
    pub position: MusicalPosition,
    pub duration: MusicalDuration,
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
    /// Creates an empty MIDI Clip from absolute musical positions.
    ///
    /// # Errors
    ///
    /// Returns an error when a musical position is invalid, the range is not
    /// positive, the Track is missing or not an Instrument Track, or the
    /// canonical commit cannot be persisted.
    pub fn create_musical_midi_clip(
        &self,
        track_id: &str,
        start: MusicalPosition,
        end: MusicalPosition,
        name: Option<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
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
            super::arrangement::create_midi_clip_in_arrangement(
                arrangement,
                track_id,
                start_tick,
                duration_ticks,
                name,
            )
        })
    }

    /// Inserts MIDI notes whose positions are absolute within the arrangement.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is empty, a musical value is invalid,
    /// a note precedes the Clip, the Clip is missing, or the canonical commit
    /// cannot be persisted.
    pub fn insert_musical_notes(
        &self,
        clip_id: &str,
        inputs: Vec<MusicalMidiNoteInput>,
    ) -> Result<CreativeSession, ApplicationError> {
        if inputs.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one musical midi note is required".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let timebase = arrangement.timebase;
            let notes = inputs
                .into_iter()
                .map(|input| resolve_musical_note(timebase, input))
                .collect::<Result<Vec<_>, _>>()?;
            insert_resolved_midi_notes_in_arrangement(arrangement, clip_id, notes)
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

    /// Adds a named timeline range from absolute musical positions.
    ///
    /// # Errors
    ///
    /// Returns an error when the name, positions, or range is invalid, or the
    /// canonical commit cannot be persisted.
    pub fn add_region(
        &self,
        name: String,
        start: MusicalPosition,
        end: MusicalPosition,
    ) -> Result<CreativeSession, ApplicationError> {
        let name = normalize_region_name(name)?;
        self.commit_arrangement(|arrangement| {
            let start_tick = arrangement.timebase.musical_position_to_tick(start)?;
            let end_tick = arrangement.timebase.musical_position_to_tick(end)?;
            arrangement
                .add_region(TimelineRegion {
                    id: next_id("region"),
                    name,
                    start_tick,
                    end_tick,
                })
                .map_err(Into::into)
        })
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

fn insert_resolved_midi_notes_in_arrangement(
    arrangement: &mut crate::domain::Arrangement,
    clip_id: &str,
    inputs: Vec<ResolvedMidiNoteInput>,
) -> Result<(), DomainError> {
    if inputs.is_empty() {
        return Err(DomainError::InvalidMusicalValue(
            "at least one musical midi note is required".into(),
        ));
    }
    let clip_start = arrangement
        .midi_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .map(|clip| clip.start_tick)
        .ok_or_else(|| {
            DomainError::InvalidClip(format!("midi clip '{clip_id}' is not registered"))
        })?;
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
    arrangement.insert_midi_notes(clip_id, notes)
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
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.arrangement.tracks[0].id.clone();
        let clip = application
            .create_musical_midi_clip(
                &track_id,
                "5:1".parse().unwrap(),
                "13:1".parse().unwrap(),
                Some("Piano".into()),
            )
            .unwrap();
        let clip_id = clip.arrangement.midi_clips[0].id.clone();
        let inserted = application
            .insert_musical_notes(
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
                        position: "13:1".parse().unwrap(),
                        duration: "1/8".parse().unwrap(),
                        velocity: None,
                        channel: None,
                    },
                ],
            )
            .unwrap();

        let notes = &inserted.arrangement.midi_clips[0].notes;
        assert_eq!(notes[0].start_tick, TimelineTick(0));
        assert_eq!(notes[0].duration_ticks, 480);
        assert_eq!(notes[0].note, 60);
        assert_eq!(notes[1].start_tick, TimelineTick(6_080));
        assert_eq!(notes[1].duration_ticks, 320);
        assert_eq!(notes[1].note, 70);
        assert_eq!(notes[2].start_tick, TimelineTick(30_720));
        assert_eq!(notes[2].duration_ticks, 480);
        assert_eq!(notes[2].note, 69);
        assert_eq!(inserted.arrangement.midi_clips[0].duration_ticks, 31_200);
        assert!(
            application
                .insert_musical_notes(
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
            .add_track("Keys", TrackKind::Instrument)
            .unwrap();
        let track_id = track.arrangement.tracks[0].id.clone();
        assert!(
            application
                .create_musical_midi_clip(
                    &track_id,
                    "5:2".parse().unwrap(),
                    "5:1".parse().unwrap(),
                    None,
                )
                .is_err()
        );
        assert!(
            application
                .add_region(" ".into(), "1:1".parse().unwrap(), "2:1".parse().unwrap())
                .is_err()
        );
        assert!(
            application
                .add_region("A'".into(), "1:1".parse().unwrap(), "2:1".parse().unwrap())
                .is_ok()
        );
        let regions = application.list_regions().unwrap();
        assert_eq!(regions[0].name, "A'");
    }
}
