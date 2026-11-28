//! Drum lanes and their sixteen-step positions.

use super::category::Category;
use super::{DrumHit, Note, TrackVisual, VisualDrumLane};
use crate::score::ScorePlan;
use crate::score_yaml::linked::LinkedScore;
use riffra_core::{MusicalFraction, MusicalPosition, ProjectTimebase, TimelineTick};
use std::collections::BTreeMap;

/// Velocity at which a hit counts as an accent.
const ACCENT_VELOCITY: u8 = 110;

/// Builds one lane per Track and output pitch of the drum Tracks.
pub(super) fn lanes(
    tracks: &[TrackVisual<'_>],
    linked: Option<&LinkedScore>,
    timebase: &ProjectTimebase,
) -> Vec<VisualDrumLane> {
    let mut lanes = Vec::new();
    for track in tracks
        .iter()
        .filter(|track| track.category == Category::Drums)
    {
        let mut by_pitch: BTreeMap<u8, Vec<&Note>> = BTreeMap::new();
        for note in &track.notes {
            by_pitch.entry(note.midi).or_default().push(note);
        }
        for (pitch, notes) in by_pitch {
            lanes.push(VisualDrumLane {
                track_id: track.track.id.clone(),
                pitch,
                label: label(track.track.score_instrument.as_deref(), &notes, linked),
                hits: notes.iter().map(|note| hit(note, timebase)).collect(),
            });
        }
    }
    lanes
}

fn hit(note: &Note, timebase: &ProjectTimebase) -> DrumHit {
    let position = timebase.tick_to_musical_position(TimelineTick(note.start_tick));
    let bar_start = timebase
        .musical_position_to_tick(MusicalPosition {
            bar: position.bar,
            beat: 1,
            offset: MusicalFraction::default(),
        })
        .expect("validated project timebase")
        .0;
    DrumHit {
        t: timebase.ticks_to_seconds(note.start_tick),
        bar: position.bar,
        step: (note.start_tick - bar_start) * 4 / u64::from(timebase.ppq),
        velocity: note.velocity,
        accent: note.velocity >= ACCENT_VELOCITY,
    }
}

/// Returns the Score instrument a lane's source pitch belongs to.
fn label(
    instrument: Option<&str>,
    notes: &[&Note],
    linked: Option<&LinkedScore>,
) -> Option<String> {
    let instrument = instrument?;
    let Some(kit) = instrument.strip_prefix("kit:") else {
        return Some(instrument.to_owned());
    };
    let plan = linked?.plan();
    let source = notes.first()?.source_midi;
    plan.kit_members
        .iter()
        .filter(|(_, member_kit)| member_kit.as_str() == kit)
        .find(|(member, _)| trigger_note(plan, member) == Some(source))
        .map(|(member, _)| member.clone())
}

fn trigger_note(plan: &ScorePlan, member: &str) -> Option<u8> {
    plan.instruments
        .get(member)?
        .get("trigger_note_midi")?
        .as_u64()
        .and_then(|note| u8::try_from(note).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        CreativeSession, InstrumentMidiSettings, ScoreSource, TempoChange, TimeSignatureChange,
        Track, TrackInstrument,
    };

    const KIT: &str = r#"{
        "meta":{"title":"Kit","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"a","bars":1,"tonic":"C"}],
        "sections":{"a":{"drums":"beat"}},
        "drums":{"beat":{"parts":{"kick":"x...|....|....|....","snare":"....|x...|....|...."}}},
        "instruments":{
            "kick":{"kit":"drums","trigger_note_midi":36},
            "snare":{"kit":"drums","trigger_note_midi":38}
        },
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}}
    }"#;

    fn linked() -> LinkedScore {
        let mut session = CreativeSession::new(0);
        session.score_source = Some(ScoreSource {
            file_name: "score.yaml".into(),
            yaml: KIT.into(),
            path: None,
            bindings: Default::default(),
            production_inputs: Default::default(),
        });
        LinkedScore::load(&session).expect("the Score compiles")
    }

    fn track(score_instrument: &str) -> Track {
        let mut track = Track::instrument("track".into(), "Track".into());
        let mut instrument = TrackInstrument::built_in(
            "device:instrument".into(),
            "Kit".into(),
            "01-kit".into(),
            "{}".into(),
        )
        .unwrap();
        instrument.midi = InstrumentMidiSettings::default();
        track.instrument = Some(instrument);
        track.score_instrument = Some(score_instrument.into());
        track
    }

    fn note(midi: u8, source_midi: u8, start_tick: u64, velocity: u8) -> Note {
        Note {
            start_sec: 0.0,
            end_sec: 0.0,
            midi,
            velocity,
            chord_role: None,
            top: false,
            lyric: None,
            id: format!("note:{start_tick}:{midi}"),
            start_tick,
            end_tick: start_tick + 120,
            channel: 1,
            source_midi,
        }
    }

    #[test]
    fn kit_tracks_split_into_labelled_pitch_lanes() {
        // Arrange
        let track = track("kit:drums");
        let visual = TrackVisual {
            track: &track,
            category: Category::Drums,
            notes: vec![
                note(38, 38, 480, 96),
                note(36, 36, 0, 120),
                note(36, 36, 1920, 96),
            ],
            sustain: Vec::new(),
        };

        // Act
        let lanes = lanes(&[visual], Some(&linked()), &ProjectTimebase::default());

        // Assert
        assert_eq!(
            lanes
                .iter()
                .map(|lane| (lane.pitch, lane.label.as_deref(), lane.hits.len()))
                .collect::<Vec<_>>(),
            [(36, Some("kick"), 2), (38, Some("snare"), 1)]
        );
    }

    #[test]
    fn lanes_label_instruments_and_place_hits_in_four_four_and_six_eight() {
        // Arrange
        let track = track("kick");
        let visual = TrackVisual {
            track: &track,
            category: Category::Drums,
            notes: vec![
                note(36, 36, 0, 120),
                note(36, 36, 960, 100),
                note(36, 36, 2400, 110),
                note(36, 36, 3840, 120),
                note(36, 36, 4080, 96),
                note(36, 36, 4320, 96),
                note(36, 36, 5760, 96),
            ],
            sustain: Vec::new(),
        };
        let timebase = ProjectTimebase {
            tempo_changes: vec![TempoChange {
                tick: 0,
                bpm: 120.0,
            }],
            time_signature_changes: vec![
                TimeSignatureChange {
                    tick: 0,
                    numerator: 4,
                    denominator: 4,
                },
                TimeSignatureChange {
                    tick: 3840,
                    numerator: 6,
                    denominator: 8,
                },
            ],
            ..ProjectTimebase::default()
        };

        // Act
        let lanes = lanes(&[visual], None, &timebase);

        // Assert
        assert_eq!(lanes[0].label.as_deref(), Some("kick"));
        assert_eq!(
            lanes[0]
                .hits
                .iter()
                .map(|hit| (hit.bar, hit.step, hit.velocity, hit.accent))
                .collect::<Vec<_>>(),
            [
                (1, 0, 120, true),
                (1, 4, 100, false),
                (1, 10, 110, true),
                (2, 0, 120, true),
                (2, 1, 96, false),
                (2, 2, 96, false),
                (2, 8, 96, false),
            ]
        );
    }
}
