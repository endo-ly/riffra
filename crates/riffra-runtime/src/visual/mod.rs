//! Deterministic, time-expanded data for drawing a Project.
//!
//! Every instant is a second from the start of the song, so consumers never
//! read the tempo map, the Score, or the canonical arrangement themselves.

mod category;
mod drums;
mod harmony;

use crate::api::output::VisualExport;
use crate::api::params::ProjectExportVisualParams;
use crate::render::entire_arrangement_end_tick;
use crate::score_yaml::linked::LinkedScore;
use category::Category;
use harmony::ChordRole;
use riffra_core::{
    CreativeSession, InstrumentControlEventKind, MidiClip, MidiEventKind, MusicalFraction,
    MusicalNoteName, MusicalPosition, ProjectTimebase, TimelineTick, Track, TrackKind,
};
use serde::Serialize;
use std::collections::BTreeMap;

/// The version of the snapshot contract.
const SCHEMA_VERSION: u32 = 1;

/// Writes the active Project's visual snapshot as JSON.
///
/// # Errors
///
/// Returns an error when the Project has no clip to measure or when the file
/// cannot be written.
pub(crate) fn export(
    session: &CreativeSession,
    params: &ProjectExportVisualParams,
) -> Result<VisualExport, String> {
    let snapshot = snapshot(session)?;
    let bytes = serde_json::to_vec_pretty(&snapshot)
        .map_err(|error| format!("could not write visual snapshot: {error}"))?;
    std::fs::write(&params.output, bytes)
        .map_err(|error| format!("could not write visual snapshot: {error}"))?;
    Ok(VisualExport {
        path: params.output.to_string_lossy().into_owned(),
    })
}

fn snapshot(session: &CreativeSession) -> Result<VisualSnapshot, String> {
    let end_tick = entire_arrangement_end_tick(session);
    if end_tick == 0 {
        return Err("visual export requires a positive-duration clip".into());
    }
    let timebase = &session.arrangement.timebase;
    let linked = LinkedScore::load(session);
    let sections = sections(session, linked.as_ref(), timebase);
    let chords = harmony::chords(session, &sections, timebase);
    let mut tracks = tracks(session, end_tick, timebase, linked.as_ref());
    for track in &mut tracks {
        if track.category != Category::Drums {
            for note in &mut track.notes {
                note.chord_role = harmony::chord_role(note.midi, note.start_tick, &chords);
            }
        }
        harmony::mark_top(&mut track.notes);
    }
    let drum_lanes = drums::lanes(&tracks, linked.as_ref(), timebase);
    Ok(VisualSnapshot {
        schema_version: SCHEMA_VERSION,
        title: session.project_name.clone().unwrap_or_default(),
        duration_sec: timebase.ticks_to_seconds(end_tick),
        tempos: timebase
            .tempo_changes
            .iter()
            .map(|change| VisualTempo {
                t: timebase.ticks_to_seconds(change.tick),
                bpm: change.bpm,
            })
            .collect(),
        meters: timebase
            .time_signature_changes
            .iter()
            .map(|change| VisualMeter {
                t: timebase.ticks_to_seconds(change.tick),
                bar: timebase
                    .tick_to_musical_position(TimelineTick(change.tick))
                    .bar,
                numerator: change.numerator,
                denominator: change.denominator,
            })
            .collect(),
        beats: beats(timebase, end_tick),
        sections,
        chords,
        tracks: tracks
            .into_iter()
            .map(|track| track.into_visual(timebase))
            .collect(),
        drum_lanes,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualSnapshot {
    schema_version: u32,
    title: String,
    duration_sec: f64,
    tempos: Vec<VisualTempo>,
    meters: Vec<VisualMeter>,
    beats: Vec<VisualBeat>,
    sections: Vec<VisualSection>,
    chords: Vec<harmony::Chord>,
    tracks: Vec<VisualTrack>,
    drum_lanes: Vec<VisualDrumLane>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualTempo {
    t: f64,
    bpm: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualMeter {
    t: f64,
    bar: u32,
    numerator: u8,
    denominator: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualBeat {
    t: f64,
    bar: u32,
    beat: u32,
    downbeat: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualSection {
    name: String,
    occurrence: u32,
    start_sec: f64,
    end_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<VisualKey>,
    #[serde(skip)]
    start_tick: u64,
    #[serde(skip)]
    end_tick: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualKey {
    tonic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<String>,
    #[serde(skip)]
    tonic_pc: Option<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualTrack {
    id: String,
    name: String,
    category: Category,
    notes: Vec<Note>,
    sustain: Vec<VisualSustain>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualSustain {
    start_sec: f64,
    end_sec: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Note {
    start_sec: f64,
    end_sec: f64,
    midi: u8,
    velocity: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    chord_role: Option<ChordRole>,
    top: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lyric: Option<String>,
    #[serde(skip)]
    id: String,
    #[serde(skip)]
    start_tick: u64,
    #[serde(skip)]
    end_tick: u64,
    #[serde(skip)]
    channel: u8,
    #[serde(skip)]
    source_midi: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualDrumLane {
    track_id: String,
    pitch: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    hits: Vec<DrumHit>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DrumHit {
    t: f64,
    bar: u32,
    step: u64,
    velocity: u8,
    accent: bool,
}

/// One Instrument Track's playable notes before their visual meaning is set.
struct TrackVisual<'a> {
    track: &'a Track,
    category: Category,
    notes: Vec<Note>,
    /// Merged sustain pedal ranges in ticks.
    sustain: Vec<(u64, u64)>,
}

impl TrackVisual<'_> {
    fn into_visual(self, timebase: &ProjectTimebase) -> VisualTrack {
        VisualTrack {
            id: self.track.id.clone(),
            name: self.track.name.clone(),
            category: self.category,
            notes: self.notes,
            sustain: self
                .sustain
                .into_iter()
                .map(|(start, end)| VisualSustain {
                    start_sec: timebase.ticks_to_seconds(start),
                    end_sec: timebase.ticks_to_seconds(end),
                })
                .collect(),
        }
    }
}

fn beats(timebase: &ProjectTimebase, end_tick: u64) -> Vec<VisualBeat> {
    let mut beats = Vec::new();
    let mut bar = 1_u32;
    while let Ok(TimelineTick(bar_start)) = timebase.musical_position_to_tick(MusicalPosition {
        bar,
        beat: 1,
        offset: MusicalFraction::default(),
    }) {
        if bar_start >= end_tick {
            break;
        }
        let numerator = timebase
            .time_signature_changes
            .iter()
            .rev()
            .find(|change| change.tick <= bar_start)
            .expect("validated project timebase")
            .numerator;
        for beat in 1..=u32::from(numerator) {
            let Ok(tick) = timebase.musical_position_to_tick(MusicalPosition {
                bar,
                beat,
                offset: MusicalFraction::default(),
            }) else {
                // A meter change truncates this bar; its later beats do not exist.
                break;
            };
            beats.push(VisualBeat {
                t: timebase.ticks_to_seconds(tick.0),
                bar,
                beat,
                downbeat: beat == 1,
            });
        }
        bar += 1;
    }
    beats
}

fn sections(
    session: &CreativeSession,
    linked: Option<&LinkedScore>,
    timebase: &ProjectTimebase,
) -> Vec<VisualSection> {
    let mut occurrences = BTreeMap::new();
    let mut sections = Vec::new();
    for region in &session.arrangement.regions {
        let occurrence = occurrences.entry(region.name.clone()).or_insert(0_u32);
        *occurrence += 1;
        let key = linked
            .and_then(|score| score.occurrence(&region.name, region.start_tick.0, timebase.ppq))
            .map(|occurrence| VisualKey {
                tonic: occurrence.tonic.clone(),
                scale: occurrence.scale.clone(),
                tonic_pc: MusicalNoteName::new(&occurrence.tonic)
                    .ok()
                    .map(MusicalNoteName::pitch_class),
            });
        sections.push(VisualSection {
            name: region.name.clone(),
            occurrence: *occurrence,
            start_sec: timebase.ticks_to_seconds(region.start_tick.0),
            end_sec: timebase.ticks_to_seconds(region.end_tick.0),
            key,
            start_tick: region.start_tick.0,
            end_tick: region.end_tick.0,
        });
    }
    sections
}

fn tracks<'a>(
    session: &'a CreativeSession,
    end_tick: u64,
    timebase: &ProjectTimebase,
    linked: Option<&LinkedScore>,
) -> Vec<TrackVisual<'a>> {
    let arrangement = &session.arrangement;
    let mut tracks = Vec::new();
    for track in &arrangement.tracks {
        if track.kind != TrackKind::Instrument || track.muted {
            continue;
        }
        let Some(instrument) = track.instrument.as_ref().filter(|item| !item.bypassed) else {
            continue;
        };
        let mut notes = Vec::new();
        let mut sustain = Vec::new();
        for clip in arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == track.id && !clip.muted)
        {
            for (pass_start, pass_end) in passes(clip, end_tick) {
                for note in &clip.notes {
                    let start_tick = pass_start.saturating_add(note.start_tick.0);
                    if start_tick >= end_tick {
                        continue;
                    }
                    let Some(midi) = instrument.midi.output_note(note.note) else {
                        continue;
                    };
                    if instrument
                        .midi
                        .keyswitches
                        .iter()
                        .any(|keyswitch| keyswitch.note == midi)
                    {
                        continue;
                    }
                    let note_end = start_tick.saturating_add(note.duration_ticks).min(pass_end);
                    notes.push(Note {
                        start_sec: timebase.ticks_to_seconds(start_tick),
                        end_sec: timebase.ticks_to_seconds(note_end),
                        midi,
                        velocity: note.velocity,
                        chord_role: None,
                        top: false,
                        lyric: note.lyric.clone(),
                        id: note.id.clone(),
                        start_tick,
                        end_tick: note_end,
                        channel: note.channel,
                        source_midi: note.note,
                    });
                }
                sustain.extend(sustain_ranges(clip, pass_start, pass_end));
            }
        }
        if notes.is_empty() {
            continue;
        }
        notes.sort_by(|left, right| {
            (left.start_tick, left.midi, &left.id).cmp(&(right.start_tick, right.midi, &right.id))
        });
        tracks.push(TrackVisual {
            track,
            category: category::classify(track, &notes, timebase.ppq, linked),
            notes,
            sustain: merge_ranges(sustain),
        });
    }
    tracks
}

/// Returns the tick range of every pass a Clip plays on the arrangement.
fn passes(clip: &MidiClip, end_tick: u64) -> Vec<(u64, u64)> {
    let mut passes = Vec::new();
    let mut start = clip.start_tick.0;
    while start < end_tick {
        passes.push((
            start,
            start.saturating_add(clip.duration_ticks).min(end_tick),
        ));
        if !clip.loop_enabled {
            break;
        }
        start = start.saturating_add(clip.duration_ticks);
    }
    passes
}

fn sustain_ranges(clip: &MidiClip, pass_start: u64, pass_end: u64) -> Vec<(u64, u64)> {
    let mut events = Vec::new();
    for event in &clip.instrument_control_events {
        if let InstrumentControlEventKind::SustainPedal { down } = event.kind {
            events.push((event.tick.0, down));
        }
    }
    for event in &clip.events {
        if event.kind == MidiEventKind::ControlChange && event.data1 == 64 {
            events.push((event.tick.0, event.data2 >= 64));
        }
    }
    // A release precedes a press at the same tick.
    events.sort_by_key(|event| (event.0, event.1));
    let mut ranges = Vec::new();
    let mut pressed = None;
    for (tick, down) in events {
        match (down, pressed) {
            (true, None) => pressed = Some(tick),
            (false, Some(from)) => {
                ranges.push((
                    pass_start.saturating_add(from),
                    pass_start.saturating_add(tick),
                ));
                pressed = None;
            }
            _ => {}
        }
    }
    if let Some(from) = pressed {
        ranges.push((pass_start + from, pass_end));
    }
    ranges
        .into_iter()
        .filter(|(start, end)| start < end)
        .collect()
}

/// Merges overlapping and touching ranges into one ordered list.
fn merge_ranges(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{
        InstrumentControlEvent, InstrumentKeyswitch, InstrumentMidiSettings, MidiEvent, MidiNote,
        ScoreSource, TempoChange, TimeSignatureChange, TimelineRegion, TrackInstrument,
    };

    fn instrument(
        transpose_semitones: i16,
        keyswitches: Vec<InstrumentKeyswitch>,
    ) -> TrackInstrument {
        let mut instrument = TrackInstrument::built_in(
            "device:instrument".into(),
            "Keys".into(),
            "01-keys".into(),
            "{}".into(),
        )
        .unwrap();
        instrument.midi = InstrumentMidiSettings {
            transpose_semitones,
            playable_notes: None,
            keyswitches,
        };
        instrument
    }

    fn track(id: &str, instrument: TrackInstrument) -> Track {
        let mut track = Track::instrument(id.into(), id.into());
        track.instrument = Some(instrument);
        track
    }

    fn clip(id: &str, track_id: &str, start_tick: u64, duration_ticks: u64) -> MidiClip {
        MidiClip {
            id: id.into(),
            name: id.into(),
            track_id: track_id.into(),
            asset_id: None,
            start_tick: TimelineTick(start_tick),
            duration_ticks,
            notes: Vec::new(),
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        }
    }

    fn note(id: &str, note: u8, start_tick: u64, duration_ticks: u64) -> MidiNote {
        MidiNote {
            lyric: None,
            id: id.into(),
            note,
            start_tick: TimelineTick(start_tick),
            duration_ticks,
            velocity: 100,
            channel: 1,
        }
    }

    fn pedal(id: &str, tick: u64, down: bool) -> InstrumentControlEvent {
        InstrumentControlEvent {
            id: id.into(),
            tick: TimelineTick(tick),
            source_order: 0,
            kind: InstrumentControlEventKind::SustainPedal { down },
        }
    }

    fn controller(id: &str, tick: u64, value: u8) -> MidiEvent {
        MidiEvent {
            id: id.into(),
            kind: MidiEventKind::ControlChange,
            tick: TimelineTick(tick),
            channel: 1,
            data1: 64,
            data2: value,
        }
    }

    #[test]
    fn beats_and_notes_convert_through_tempo_and_meter_changes() {
        // Arrange
        let mut session = CreativeSession::new(0);
        session.arrangement.timebase.tempo_changes = vec![
            TempoChange {
                tick: 0,
                bpm: 120.0,
            },
            TempoChange {
                tick: 1920,
                bpm: 60.0,
            },
        ];
        session.arrangement.timebase.time_signature_changes = vec![
            TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            },
            TimeSignatureChange {
                tick: 3840,
                numerator: 3,
                denominator: 4,
            },
        ];
        session.arrangement.tracks = vec![track("keys", instrument(0, Vec::new()))];
        let mut clip = clip("clip", "keys", 0, 6720);
        clip.notes = vec![note("held", 60, 2880, 960)];
        session.arrangement.midi_clips = vec![clip];

        // Act
        let snapshot = snapshot(&session).unwrap();

        // Assert
        assert_eq!(
            snapshot
                .tempos
                .iter()
                .map(|tempo| (tempo.t, tempo.bpm))
                .collect::<Vec<_>>(),
            [(0.0, 120.0), (1.0, 60.0)]
        );
        assert_eq!(
            snapshot
                .meters
                .iter()
                .map(|meter| (meter.t, meter.bar, meter.numerator, meter.denominator))
                .collect::<Vec<_>>(),
            [(0.0, 1, 4, 4), (3.0, 2, 3, 4)]
        );
        assert_eq!(
            snapshot
                .beats
                .iter()
                .map(|beat| (beat.t, beat.bar, beat.beat, beat.downbeat))
                .collect::<Vec<_>>(),
            [
                (0.0, 1, 1, true),
                (0.5, 1, 2, false),
                (1.0, 1, 3, false),
                (2.0, 1, 4, false),
                (3.0, 2, 1, true),
                (4.0, 2, 2, false),
                (5.0, 2, 3, false),
            ]
        );
        assert_eq!(snapshot.duration_sec, 6.0);
        assert_eq!(
            snapshot.tracks[0]
                .notes
                .iter()
                .map(|note| (note.start_sec, note.end_sec))
                .collect::<Vec<_>>(),
            [(2.0, 3.0)]
        );
    }

    const KEYED: &str = r#"{
        "meta":{"title":"Keyed","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[
            {"section":"first","bars":1,"tonic":"F","scale":"ionian"},
            {"section":"second","bars":1,"tonic":"A","scale":"aeolian"}
        ],
        "sections":{"first":{},"second":{}},
        "instruments":{"lead":{"preset":"TEST/001-lead"}}
    }"#;

    #[test]
    fn linked_score_sections_carry_their_key_and_unlinked_projects_do_not() {
        // Arrange
        let mut session = CreativeSession::new(0);
        session.settings.mixdown.musical_end_tick = 9600;
        session.arrangement.regions = vec![
            TimelineRegion {
                id: "region:1".into(),
                name: "first".into(),
                start_tick: TimelineTick(0),
                end_tick: TimelineTick(3840),
            },
            TimelineRegion {
                id: "region:2".into(),
                name: "second".into(),
                start_tick: TimelineTick(3840),
                end_tick: TimelineTick(7680),
            },
            TimelineRegion {
                id: "region:3".into(),
                name: "first".into(),
                start_tick: TimelineTick(7680),
                end_tick: TimelineTick(9600),
            },
        ];
        session.score_source = Some(ScoreSource {
            file_name: "score.yaml".into(),
            yaml: KEYED.into(),
            path: None,
            bindings: Default::default(),
            production_inputs: Default::default(),
        });

        // Act
        let linked = snapshot(&session).unwrap();
        session.score_source = None;
        let unlinked = snapshot(&session).unwrap();

        // Assert
        assert_eq!(
            linked
                .sections
                .iter()
                .map(|section| (
                    section.occurrence,
                    section
                        .key
                        .as_ref()
                        .map(|key| (key.tonic.as_str(), key.scale.as_deref()))
                ))
                .collect::<Vec<_>>(),
            [
                (1, Some(("F", Some("ionian")))),
                (1, Some(("A", Some("aeolian")))),
                (2, None),
            ]
        );
        assert!(
            unlinked
                .sections
                .iter()
                .all(|section| section.key.is_none())
        );
    }

    #[test]
    fn notes_reflect_mute_bypass_keyswitches_transposition_and_loops() {
        // Arrange
        let mut session = CreativeSession::new(0);
        session.settings.mixdown.musical_end_tick = 2520;
        let mut muted = track("muted", instrument(0, Vec::new()));
        muted.muted = true;
        let mut bypassed = track("bypassed", instrument(0, Vec::new()));
        bypassed.instrument.as_mut().unwrap().bypassed = true;
        session.arrangement.tracks = vec![
            track(
                "keys",
                instrument(
                    12,
                    vec![InstrumentKeyswitch {
                        note: 24,
                        name: "Soft".into(),
                    }],
                ),
            ),
            muted,
            bypassed,
            Track::audio("audio".into(), "Audio".into()),
        ];
        let mut looping = clip("clip:loop", "keys", 0, 960);
        looping.loop_enabled = true;
        looping.notes = vec![
            note("held", 60, 480, 480),
            note("keyswitch", 12, 0, 480),
            note("unmapped", 120, 0, 480),
        ];
        let mut silent = clip("clip:muted", "muted", 0, 960);
        silent.muted = true;
        silent.notes = vec![note("silent", 60, 0, 480)];
        let mut bypassed_clip = clip("clip:bypassed", "bypassed", 0, 960);
        bypassed_clip.notes = vec![note("bypassed", 60, 0, 480)];
        session.arrangement.midi_clips = vec![looping, silent, bypassed_clip];

        // Act
        let snapshot = snapshot(&session).unwrap();

        // Assert
        assert_eq!(
            snapshot
                .tracks
                .iter()
                .map(|track| (track.name.as_str(), track.notes.len()))
                .collect::<Vec<_>>(),
            [("keys", 3)]
        );
        assert_eq!(
            snapshot.tracks[0]
                .notes
                .iter()
                .map(|note| (note.start_sec, note.end_sec, note.midi))
                .collect::<Vec<_>>(),
            [(0.25, 0.5, 72), (0.75, 1.0, 72), (1.25, 1.3125, 72)]
        );
    }

    #[test]
    fn sustain_ranges_merge_pedal_and_controller_events() {
        // Arrange
        let mut session = CreativeSession::new(0);
        session.arrangement.tracks = vec![track("keys", instrument(0, Vec::new()))];
        let mut first = clip("clip:first", "keys", 0, 1920);
        first.notes = vec![note("held", 60, 0, 1920)];
        first.instrument_control_events = vec![
            pedal("late", 1500, true),
            pedal("release", 480, false),
            pedal("press", 0, true),
        ];
        first.events = vec![controller("down", 960, 100), controller("up", 1200, 0)];
        let mut second = clip("clip:second", "keys", 1920, 960);
        second.notes = vec![note("tail", 60, 0, 480)];
        second.instrument_control_events = vec![pedal("next", 0, true), pedal("off", 480, false)];
        session.arrangement.midi_clips = vec![first, second];

        // Act
        let snapshot = snapshot(&session).unwrap();

        // Assert
        assert_eq!(
            snapshot.tracks[0]
                .sustain
                .iter()
                .map(|sustain| (sustain.start_sec, sustain.end_sec))
                .collect::<Vec<_>>(),
            [(0.0, 0.25), (0.5, 0.625), (0.78125, 1.25)]
        );
    }
}
