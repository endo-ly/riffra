//! Deterministic visual categories derived from the notes a Track actually plays.

use super::Note;
use crate::score_yaml::linked::LinkedScore;
use riffra_core::Track;
use serde::Serialize;
use std::collections::BTreeMap;

/// Tracks whose notes all use the General MIDI percussion channel.
const DRUM_CHANNEL: u8 = 10;
/// Highest polyphony ratio a bass line may have.
const BASS_MAX_POLYPHONY: f64 = 0.2;
/// Highest median pitch a bass line may have.
const BASS_MAX_MEDIAN: u8 = 55;
/// Lowest pitch that always makes a Track a bass line.
const BASS_MAX_LOWEST: u8 = 43;
/// Lowest polyphony ratio a chord Track may have.
const CHORDS_MIN_POLYPHONY: f64 = 0.3;
/// Shortest average note a chord Track may have, in beats.
const CHORDS_MIN_BEATS: f64 = 1.0;
/// Highest polyphony ratio an arpeggio Track may have.
const ARP_MAX_POLYPHONY: f64 = 0.25;
/// Longest average note an arpeggio Track may have, in beats.
const ARP_MAX_BEATS: f64 = 0.5;

/// How a Track is drawn without relying on its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Category {
    Vocal,
    Lead,
    Chords,
    Arp,
    Bass,
    Drums,
}

/// Classifies one Track from its playable notes and its Score structure.
///
/// # Panics
///
/// Panics when `notes` is empty; Tracks without playable notes are not drawn.
pub(super) fn classify(
    track: &Track,
    notes: &[Note],
    ppq: u32,
    linked: Option<&LinkedScore>,
) -> Category {
    if is_drums(track, notes, linked) {
        return Category::Drums;
    }
    if notes.iter().any(|note| note.lyric.is_some()) {
        return Category::Vocal;
    }
    let polyphony = polyphony(notes);
    let lowest = notes
        .iter()
        .map(|note| note.midi)
        .min()
        .expect("a drawn Track has notes");
    if polyphony < BASS_MAX_POLYPHONY
        && (median(notes) < BASS_MAX_MEDIAN || lowest < BASS_MAX_LOWEST)
    {
        return Category::Bass;
    }
    let beats = average_beats(notes, ppq);
    if polyphony >= CHORDS_MIN_POLYPHONY && beats >= CHORDS_MIN_BEATS {
        return Category::Chords;
    }
    if polyphony < ARP_MAX_POLYPHONY && beats <= ARP_MAX_BEATS {
        return Category::Arp;
    }
    Category::Lead
}

fn is_drums(track: &Track, notes: &[Note], linked: Option<&LinkedScore>) -> bool {
    let instrument = track.score_instrument.as_deref();
    instrument.is_some_and(|instrument| instrument.starts_with("kit:"))
        || instrument
            .zip(linked)
            .is_some_and(|(instrument, linked)| plays_only_triggers(instrument, linked))
        || notes.iter().all(|note| note.channel == DRUM_CHANNEL)
}

/// Returns whether the Score plays an instrument through trigger patterns only.
fn plays_only_triggers(instrument: &str, linked: &LinkedScore) -> bool {
    let mut lanes = linked
        .plan()
        .lanes
        .iter()
        .filter(|lane| lane.id == instrument)
        .peekable();
    lanes.peek().is_some()
        && lanes.all(|lane| {
            !lane.sources.is_empty() && lane.sources.iter().all(|source| source.trigger)
        })
}

/// Returns the ratio of attacks that sound more than one note.
fn polyphony(notes: &[Note]) -> f64 {
    let mut attacks: BTreeMap<u64, usize> = BTreeMap::new();
    for note in notes {
        *attacks.entry(note.start_tick).or_insert(0) += 1;
    }
    let polyphonic = attacks.values().filter(|count| **count >= 2).count();
    polyphonic as f64 / attacks.len() as f64
}

/// Returns the lower median of the Track's pitches.
fn median(notes: &[Note]) -> u8 {
    let mut pitches = notes.iter().map(|note| note.midi).collect::<Vec<_>>();
    pitches.sort_unstable();
    pitches[(pitches.len() - 1) / 2]
}

fn average_beats(notes: &[Note], ppq: u32) -> f64 {
    let ticks: u64 = notes
        .iter()
        .map(|note| note.end_tick.saturating_sub(note.start_tick))
        .sum();
    ticks as f64 / f64::from(ppq) / notes.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, ScoreSource, Track};

    const GROOVE: &str = r#"{
        "meta":{"title":"Groove","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"a","bars":1,"tonic":"C"}],
        "sections":{"a":{"drums":"beat"}},
        "drums":{"beat":{"parts":{"kick":"x...|....|....|...."}}},
        "instruments":{"kick":{"kit":"drums","trigger_note_midi":36}},
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}}
    }"#;

    fn track(score_instrument: Option<&str>) -> Track {
        let mut track = Track::instrument("track".into(), "Track".into());
        track.score_instrument = score_instrument.map(str::to_owned);
        track
    }

    fn note(midi: u8, start_tick: u64, end_tick: u64, channel: u8) -> Note {
        note_with(midi, start_tick, end_tick, channel, None)
    }

    fn note_with(
        midi: u8,
        start_tick: u64,
        end_tick: u64,
        channel: u8,
        lyric: Option<&str>,
    ) -> Note {
        Note {
            start_sec: 0.0,
            end_sec: 0.0,
            midi,
            velocity: 100,
            chord_role: None,
            top: false,
            lyric: lyric.map(str::to_owned),
            id: format!("note:{start_tick}:{midi}"),
            start_tick,
            end_tick,
            channel,
            source_midi: midi,
        }
    }

    fn linked() -> LinkedScore {
        let mut session = CreativeSession::new(0);
        session.score_source = Some(ScoreSource {
            file_name: "score.yaml".into(),
            yaml: GROOVE.into(),
            path: None,
            bindings: Default::default(),
            production_inputs: Default::default(),
        });
        LinkedScore::load(&session).expect("the Score compiles")
    }

    fn classify_with(track: &Track, notes: &[Note], linked: Option<&LinkedScore>) -> Category {
        classify(track, notes, 960, linked)
    }

    #[test]
    fn drums_are_detected_from_the_kit_the_trigger_lanes_or_the_channel() {
        // Arrange
        let kit = track(Some("kit:drums"));
        let triggered = track(Some("kick"));
        let channelled = track(None);
        let mixed = track(None);
        let notes = [note(60, 0, 960, 1)];
        let mixed_notes = [note(36, 0, 240, 10), note(60, 0, 960, 1)];
        let linked = linked();

        // Act / Assert
        assert_eq!(classify_with(&kit, &notes, None), Category::Drums);
        assert_eq!(
            classify_with(&triggered, &notes, Some(&linked)),
            Category::Drums
        );
        assert_eq!(
            classify_with(&channelled, &[note(36, 0, 240, 10)], None),
            Category::Drums
        );
        assert_eq!(classify_with(&mixed, &mixed_notes, None), Category::Lead);
    }

    #[test]
    fn melodic_categories_come_from_lyrics_polyphony_pitch_and_length() {
        // Arrange
        let track = track(None);
        let sung = [note_with(72, 0, 960, 1, Some("ah"))];
        let bass = [note(50, 0, 480, 1), note(52, 960, 1440, 1)];
        let chords = [
            note(60, 0, 960, 1),
            note(64, 0, 960, 1),
            note(67, 0, 960, 1),
        ];
        let arpeggio = [
            note(72, 0, 240, 1),
            note(76, 240, 480, 1),
            note(79, 480, 720, 1),
        ];
        let lead = [note(72, 0, 960, 1), note(74, 960, 1920, 1)];

        // Act / Assert
        assert_eq!(classify_with(&track, &sung, None), Category::Vocal);
        assert_eq!(classify_with(&track, &bass, None), Category::Bass);
        assert_eq!(classify_with(&track, &chords, None), Category::Chords);
        assert_eq!(classify_with(&track, &arpeggio, None), Category::Arp);
        assert_eq!(classify_with(&track, &lead, None), Category::Lead);
    }
}
