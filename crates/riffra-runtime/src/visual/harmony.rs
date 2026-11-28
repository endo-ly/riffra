//! Chord segmentation, roman-numeral naming, and note-level harmony meaning.

use super::{VisualNote, VisualSection};
use crate::score::syntax;
use riffra_core::{CreativeSession, HarmonyChord, HarmonyEvent, ProjectTimebase};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

/// A chord sounding over a non-overlapping range of the arrangement.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualChord {
    pub start_sec: f64,
    pub end_sec: f64,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub degree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub root_pc: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bass_pc: Option<u8>,
    pub tones_pc: Vec<u8>,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) start_tick: u64,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) end_tick: u64,
}

/// The role a note plays in the chord sounding when it starts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChordRole {
    Root,
    ChordTone,
    Tension,
    NonChord,
}

/// Semitone distance from the tonic to its scale degree.
const DEGREES: [&str; 12] = [
    "I", "bII", "II", "bIII", "III", "IV", "#IV", "V", "bVI", "VI", "bVII", "VII",
];

/// Scale degrees of the seven note letters.
const ROMAN: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];

/// Cuts the harmony events into the chord sequence the arrangement actually sounds.
pub(super) fn chords(
    session: &CreativeSession,
    sections: &[VisualSection],
    timebase: &ProjectTimebase,
) -> Vec<VisualChord> {
    let events = &session.arrangement.harmony_events;
    let mut boundaries: Vec<u64> = events
        .iter()
        .flat_map(|event| [event.start_tick.0, event.end_tick.0])
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut spans: Vec<(u64, u64, &HarmonyEvent)> = Vec::new();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let sounding = events
            .iter()
            .filter(|event| event.start_tick.0 <= start && end <= event.end_tick.0)
            .max_by(|left, right| {
                left.start_tick
                    .0
                    .cmp(&right.start_tick.0)
                    .then_with(|| right.id.cmp(&left.id))
            });
        match (sounding, spans.last_mut()) {
            (Some(event), Some(last)) if last.2.id == event.id => last.1 = end,
            (Some(event), _) => spans.push((start, end, event)),
            (None, _) => {}
        }
    }
    spans
        .into_iter()
        .map(|(start, end, event)| chord(start, end, event, sections, timebase))
        .collect()
}

fn chord(
    start_tick: u64,
    end_tick: u64,
    event: &HarmonyEvent,
    sections: &[VisualSection],
    timebase: &ProjectTimebase,
) -> VisualChord {
    let harmony = &event.chord;
    let mut tones_pc = harmony
        .tones
        .iter()
        .map(|tone| tone.pitch_class())
        .collect::<Vec<_>>();
    tones_pc.sort_unstable();
    tones_pc.dedup();
    let (degree, symbol) = naming(harmony, sections, start_tick);
    VisualChord {
        start_sec: timebase.ticks_to_seconds(start_tick),
        end_sec: timebase.ticks_to_seconds(end_tick),
        symbol,
        degree,
        root_pc: harmony.root.map(|root| root.pitch_class()),
        bass_pc: harmony.bass.map(|bass| bass.pitch_class()),
        tones_pc,
        start_tick,
        end_tick,
    }
}

/// Returns the roman-numeral degree and the absolute symbol of one chord.
fn naming(
    chord: &HarmonyChord,
    sections: &[VisualSection],
    start_tick: u64,
) -> (Option<String>, String) {
    if let Ok(Some(parsed)) = syntax::chord(&chord.name) {
        let quality = score_quality(&chord.name);
        let mut symbol = chord.root.map(|root| root.to_string()).unwrap_or_default();
        symbol.push_str(quality);
        if parsed.slash_bass
            && let Some(bass) = chord.bass
        {
            symbol.push('/');
            symbol.push_str(&bass.to_string());
        }
        return (score_degree(&chord.name, &parsed), symbol);
    }
    (
        absolute_degree(chord, sections, start_tick),
        chord.name.clone(),
    )
}

/// Returns the roman-numeral degree of a Score chord notation.
fn score_degree(name: &str, parsed: &syntax::Chord) -> Option<String> {
    let mut degree = roman(root_token(name))?;
    degree.push_str(score_quality(name));
    if parsed.slash_bass {
        degree.push('/');
        degree.push_str(&roman(name.split_once('/')?.1)?);
    }
    Some(degree)
}

/// Returns the part of a Score chord after its `:` and before its `/`.
fn score_quality(name: &str) -> &str {
    let quality = name.split_once(':').map_or(name, |(_, quality)| quality);
    let quality = quality
        .split_once('/')
        .map_or(quality, |(quality, _)| quality);
    if quality == "maj" { "" } else { quality }
}

/// Returns the degree token of a Score chord before its `:`.
fn root_token(name: &str) -> &str {
    name.split_once(':').map_or(name, |(root, _)| root)
}

/// Turns a Score degree token into a roman numeral, keeping its accidentals.
fn roman(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let mut roman = String::new();
    for character in chars.by_ref() {
        match character {
            'b' | '#' => roman.push(character),
            '1'..='7' => {
                roman.push_str(ROMAN[usize::from(character as u8 - b'1')]);
                return Some(roman);
            }
            _ => return None,
        }
    }
    None
}

/// Names a chord symbol whose degrees are relative to the section key.
fn absolute_degree(
    chord: &HarmonyChord,
    sections: &[VisualSection],
    start_tick: u64,
) -> Option<String> {
    let section = sections
        .iter()
        .find(|section| section.start_tick <= start_tick && start_tick < section.end_tick)?;
    let tonic_pc = section.key.as_ref()?.tonic_pc?;
    let root = chord.root?;
    let rest = chord.name.strip_prefix(&root.to_string())?;
    let mut degree = degrees_above(i32::from(root.pitch_class()), i32::from(tonic_pc)).to_owned();
    degree.push_str(rest.split_once('/').map_or(rest, |(quality, _)| quality));
    if let Some(bass) = chord.bass {
        degree.push('/');
        degree.push_str(degrees_above(
            i32::from(bass.pitch_class()),
            i32::from(tonic_pc),
        ));
    }
    Some(degree)
}

fn degrees_above(pitch_class: i32, tonic_pc: i32) -> &'static str {
    DEGREES[usize::try_from((pitch_class - tonic_pc).rem_euclid(12)).expect("a pitch class index")]
}

/// Returns the role of a note in the chord sounding when the note starts.
pub(super) fn chord_role(midi: u8, start_tick: u64, chords: &[VisualChord]) -> Option<ChordRole> {
    let chord = chords
        .iter()
        .find(|chord| chord.start_tick <= start_tick && start_tick < chord.end_tick)?;
    Some(chord.role(midi % 12))
}

impl VisualChord {
    fn role(&self, pitch_class: u8) -> ChordRole {
        let sounding = self.tones_pc.contains(&pitch_class);
        let Some(root_pc) = self.root_pc else {
            return if sounding {
                ChordRole::ChordTone
            } else {
                ChordRole::NonChord
            };
        };
        if pitch_class == root_pc {
            return ChordRole::Root;
        }
        if !sounding {
            return ChordRole::NonChord;
        }
        let interval = (12 + pitch_class - root_pc) % 12;
        let intervals: BTreeSet<u8> = self
            .tones_pc
            .iter()
            .map(|tone| (12 + tone - root_pc) % 12)
            .collect();
        match interval {
            3 | 4 | 7 | 10 | 11 => ChordRole::ChordTone,
            6 | 8 if intervals.contains(&7) => ChordRole::Tension,
            6 | 8 => ChordRole::ChordTone,
            9 if intervals.contains(&10) || intervals.contains(&11) => ChordRole::Tension,
            9 => ChordRole::ChordTone,
            _ => ChordRole::Tension,
        }
    }
}

/// Marks the notes that no other note sounding with them is higher than.
pub(super) fn mark_top(notes: &mut [VisualNote]) {
    let tops = notes
        .iter()
        .enumerate()
        .map(|(index, note)| {
            notes.iter().enumerate().all(|(other, candidate)| {
                other == index
                    || !(candidate.start_tick <= note.start_tick
                        && note.start_tick < candidate.end_tick)
                    || candidate.midi <= note.midi
            })
        })
        .collect::<Vec<_>>();
    for (note, top) in notes.iter_mut().zip(tops) {
        note.top = top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{HarmonyChord, MusicalNoteName, TimelineTick};

    fn chord(tones: &[&str], root: Option<&str>, bass: Option<&str>, name: &str) -> HarmonyChord {
        HarmonyChord::from_explicit(
            tones
                .iter()
                .map(|tone| MusicalNoteName::new(tone).unwrap())
                .collect(),
            root.map(|root| MusicalNoteName::new(root).unwrap()),
            bass.map(|bass| MusicalNoteName::new(bass).unwrap()),
            Some(name.into()),
        )
        .unwrap()
    }

    fn event(id: &str, start: u64, end: u64, chord: HarmonyChord) -> HarmonyEvent {
        HarmonyEvent {
            id: id.into(),
            start_tick: TimelineTick(start),
            end_tick: TimelineTick(end),
            chord,
        }
    }

    fn session(events: Vec<HarmonyEvent>) -> CreativeSession {
        let mut session = CreativeSession::new(0);
        session.arrangement.harmony_events = events;
        session
    }

    fn section(name: &str, start_tick: u64, end_tick: u64, tonic: &str) -> VisualSection {
        VisualSection {
            name: name.into(),
            occurrence: 1,
            start_sec: 0.0,
            end_sec: 0.0,
            key: Some(super::super::VisualKey {
                tonic: tonic.into(),
                scale: Some("ionian".into()),
                tonic_pc: MusicalNoteName::new(tonic).ok().map(|n| n.pitch_class()),
                scale_pcs: Vec::new(),
            }),
            start_tick,
            end_tick,
        }
    }

    fn note(id: &str, midi: u8, start_tick: u64, end_tick: u64) -> VisualNote {
        VisualNote {
            start_sec: 0.0,
            end_sec: 0.0,
            midi,
            velocity: 100,
            chord_role: None,
            top: false,
            lyric: None,
            id: id.into(),
            start_tick,
            end_tick,
            channel: 1,
            source_midi: midi,
        }
    }

    #[test]
    fn overlapping_events_are_cut_into_the_latest_start_and_named_by_their_notation() {
        // Arrange
        let key = [section("verse", 0, 3840, "F")];
        let session = session(vec![
            event("b", 0, 1920, chord(&["G", "B", "D"], Some("G"), None, "G")),
            event(
                "a",
                0,
                2880,
                chord(&["Bb", "D", "F", "A"], Some("Bb"), Some("C"), "4:maj7/5"),
            ),
            event(
                "c",
                960,
                2880,
                chord(&["D", "F", "A", "C", "E"], Some("D"), Some("F"), "Dm9/F"),
            ),
        ]);

        // Act
        let keyed = chords(&session, &key, &ProjectTimebase::default());
        let unkeyed = chords(&session, &[], &ProjectTimebase::default());

        // Assert
        assert_eq!(
            keyed
                .iter()
                .map(|chord| (
                    chord.start_tick,
                    chord.end_tick,
                    chord.degree.as_deref(),
                    chord.symbol.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                (0, 960, Some("IVmaj7/V"), "Bbmaj7/C"),
                (960, 2880, Some("VIm9/I"), "Dm9/F"),
            ]
        );
        assert_eq!(
            unkeyed
                .iter()
                .map(|chord| (chord.degree.as_deref(), chord.symbol.as_str()))
                .collect::<Vec<_>>(),
            [(Some("IVmaj7/V"), "Bbmaj7/C"), (None, "Dm9/F")]
        );
    }

    #[test]
    fn a_bare_major_quality_is_omitted_and_unknown_roots_keep_no_degree() {
        // Act
        let (degree, symbol) = naming(&chord(&["C", "E", "G"], Some("C"), None, "1:maj"), &[], 0);

        // Assert
        assert_eq!((degree.as_deref(), symbol.as_str()), (Some("I"), "C"));
        assert_eq!(
            naming(
                &chord(&["A#", "D", "F"], Some("A#"), None, "Bbmaj7"),
                &[section("verse", 0, 3840, "C")],
                0
            )
            .0,
            None
        );
    }

    fn role(tones: &[&str], root: Option<&str>, pitch_class: u8) -> ChordRole {
        let session = session(vec![event("a", 0, 960, chord(tones, root, None, "test"))]);
        let chords = chords(&session, &[], &ProjectTimebase::default());
        chord_role(60 + pitch_class, 0, &chords).expect("the note starts inside the chord")
    }

    #[test]
    fn chord_roles_follow_the_interval_table() {
        // Arrange
        let cases: [(&[&str], Option<&str>, u8, ChordRole); 14] = [
            (&["C", "E", "G", "B", "F#"], Some("C"), 0, ChordRole::Root),
            (
                &["C", "E", "G", "B", "F#"],
                Some("C"),
                4,
                ChordRole::ChordTone,
            ),
            (
                &["C", "E", "G", "B", "F#"],
                Some("C"),
                7,
                ChordRole::ChordTone,
            ),
            (
                &["C", "E", "G", "B", "F#"],
                Some("C"),
                11,
                ChordRole::ChordTone,
            ),
            (
                &["C", "E", "G", "B", "F#"],
                Some("C"),
                6,
                ChordRole::Tension,
            ),
            (
                &["C", "E", "G", "B", "F#"],
                Some("C"),
                9,
                ChordRole::NonChord,
            ),
            (&["C", "E", "Ab", "B"], Some("C"), 8, ChordRole::ChordTone),
            (&["C", "E", "G", "A"], Some("C"), 9, ChordRole::ChordTone),
            (
                &["C", "E", "G", "Bb", "A"],
                Some("C"),
                9,
                ChordRole::Tension,
            ),
            (
                &["C", "E", "G", "Bb", "Db"],
                Some("C"),
                1,
                ChordRole::Tension,
            ),
            (
                &["C", "E", "G", "Bb", "D"],
                Some("C"),
                2,
                ChordRole::Tension,
            ),
            (&["C", "F", "G"], Some("C"), 5, ChordRole::Tension),
            (&["C", "E", "G"], None, 4, ChordRole::ChordTone),
            (&["C", "E", "G"], None, 2, ChordRole::NonChord),
        ];

        // Act / Assert
        for (tones, root, pitch_class, expected) in cases {
            assert_eq!(
                role(tones, root, pitch_class),
                expected,
                "{root:?} {tones:?} {pitch_class}"
            );
        }
    }

    #[test]
    fn the_highest_note_sounding_with_a_held_chord_is_top() {
        // Arrange
        let mut notes = vec![
            note("c", 60, 0, 960),
            note("e", 64, 0, 960),
            note("g", 67, 0, 960),
            note("melody", 72, 480, 960),
        ];

        // Act
        mark_top(&mut notes);

        // Assert
        assert_eq!(
            notes.iter().map(|note| note.top).collect::<Vec<_>>(),
            [false, false, true, true]
        );
    }
}
