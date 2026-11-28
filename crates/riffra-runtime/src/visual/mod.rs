//! Deterministic, time-expanded data for drawing a Project.
//!
//! Every instant is a second from the start of the song, so consumers never
//! read the tempo map, the Score, or the canonical arrangement themselves.

pub mod category;
mod drums;
pub mod harmony;

use crate::api::output::VisualExport;
use crate::api::params::ProjectExportVisualParams;
use crate::render::entire_arrangement_end_tick;
use crate::score::scale;
use crate::score_yaml::linked::LinkedScore;
use category::TrackCategory;
use harmony::{ChordRole, VisualChord};
use riffra_core::{
    CreativeSession, InstrumentControlEventKind, MidiClip, MidiEventKind, MusicalFraction,
    MusicalNoteName, MusicalPosition, ProjectTimebase, TimelineTick, Track, TrackKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

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
    let snapshot = snapshot_optional(session)
        .ok_or_else(|| "visual export requires a positive-duration clip".to_owned())?;
    let bytes = serde_json::to_vec_pretty(&snapshot)
        .map_err(|error| format!("could not write visual snapshot: {error}"))?;
    std::fs::write(&params.output, bytes)
        .map_err(|error| format!("could not write visual snapshot: {error}"))?;
    Ok(VisualExport {
        path: params.output.to_string_lossy().into_owned(),
    })
}

/// Builds the visual snapshot of the active Project.
///
/// Returns `None` when the Project has no clip to measure.
pub(crate) fn snapshot_optional(session: &CreativeSession) -> Option<VisualSnapshot> {
    let end_tick = entire_arrangement_end_tick(session);
    if end_tick == 0 {
        return None;
    }
    let timebase = &session.arrangement.timebase;
    let linked = LinkedScore::load(session);
    let sections = sections(session, linked.as_ref(), timebase);
    let chords = harmony::chords(session, &sections, timebase);
    let mut tracks = tracks(session, end_tick, timebase, linked.as_ref());
    for track in &mut tracks {
        if track.category != TrackCategory::Drums {
            for note in &mut track.notes {
                note.chord_role = harmony::chord_role(note.midi, note.start_tick, &chords);
            }
        }
        harmony::mark_top(&mut track.notes);
    }
    let drum_lanes = drums::lanes(&tracks, linked.as_ref(), timebase);
    Some(VisualSnapshot {
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

/// A time-expanded view of the active Project that a renderer draws.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualSnapshot {
    pub schema_version: u32,
    pub title: String,
    pub duration_sec: f64,
    pub tempos: Vec<VisualTempo>,
    pub meters: Vec<VisualMeter>,
    pub beats: Vec<VisualBeat>,
    pub sections: Vec<VisualSection>,
    pub chords: Vec<VisualChord>,
    pub tracks: Vec<VisualTrack>,
    pub drum_lanes: Vec<VisualDrumLane>,
}

/// A tempo change and its time from the start of the song.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualTempo {
    pub t: f64,
    pub bpm: f64,
}

/// A time-signature change and the bar it starts.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualMeter {
    pub t: f64,
    pub bar: u32,
    pub numerator: u8,
    pub denominator: u8,
}

/// One beat of the arrangement.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualBeat {
    pub t: f64,
    pub bar: u32,
    pub beat: u32,
    pub downbeat: bool,
}

/// One region of the arrangement and its key.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualSection {
    pub name: String,
    pub occurrence: u32,
    pub start_sec: f64,
    pub end_sec: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key: Option<VisualKey>,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) start_tick: u64,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) end_tick: u64,
}

/// The key one section is written in, resolved to pitch classes.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualKey {
    pub tonic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tonic_pc: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scale_pcs: Vec<u8>,
}

/// One playable Track with its notes and sustain ranges.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualTrack {
    pub id: String,
    pub name: String,
    pub category: TrackCategory,
    pub notes: Vec<VisualNote>,
    pub sustain: Vec<VisualSustain>,
}

/// A sustain pedal range.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualSustain {
    pub start_sec: f64,
    pub end_sec: f64,
}

/// One sounding note.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualNote {
    pub start_sec: f64,
    pub end_sec: f64,
    pub midi: u8,
    pub velocity: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub chord_role: Option<ChordRole>,
    pub top: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lyric: Option<String>,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) id: String,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) start_tick: u64,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) end_tick: u64,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) channel: u8,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) source_midi: u8,
}

/// One drum pitch of a Track.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualDrumLane {
    pub track_id: String,
    pub pitch: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    pub hits: Vec<VisualDrumHit>,
}

/// One drum hit on a lane.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualDrumHit {
    pub t: f64,
    pub bar: u32,
    pub step: u64,
    pub velocity: u8,
    pub accent: bool,
}

/// One Instrument Track's playable notes before their visual meaning is set.
struct TrackVisual<'a> {
    track: &'a Track,
    category: TrackCategory,
    notes: Vec<VisualNote>,
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
            .map(|occurrence| visual_key(occurrence, linked));
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

/// Resolves one Score occurrence to the key its section is drawn in.
fn visual_key(occurrence: &crate::score::Occurrence, linked: Option<&LinkedScore>) -> VisualKey {
    let tonic_pc = MusicalNoteName::new(&occurrence.tonic)
        .ok()
        .map(MusicalNoteName::pitch_class);
    let scale_pcs = occurrence
        .scale
        .as_deref()
        .zip(tonic_pc)
        .map(|(scale, tonic_pc)| pitch_classes(tonic_pc, scale_offsets(scale, linked)))
        .unwrap_or_default();
    VisualKey {
        tonic: occurrence.tonic.clone(),
        scale: occurrence.scale.clone(),
        tonic_pc,
        scale_pcs,
    }
}

/// Returns the semitone offsets of a built-in or a custom Score scale.
fn scale_offsets(scale: &str, linked: Option<&LinkedScore>) -> Vec<i32> {
    if let Some(degrees) = scale::degrees(scale) {
        return scale::offsets(degrees.iter().copied());
    }
    let Some(definitions) = linked
        .and_then(|score| score.field(&["scale_definitions", scale]))
        .and_then(|definitions| definitions.as_array())
    else {
        return Vec::new();
    };
    scale::offsets(definitions.iter().filter_map(|degree| degree.as_str()))
}

/// Places semitone offsets onto the circle of pitch classes.
fn pitch_classes(tonic_pc: u8, offsets: Vec<i32>) -> Vec<u8> {
    let mut pitch_classes = offsets
        .into_iter()
        .map(|offset| (i32::from(tonic_pc) + offset).rem_euclid(12) as u8)
        .collect::<Vec<_>>();
    pitch_classes.sort_unstable();
    pitch_classes.dedup();
    pitch_classes
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
                    notes.push(VisualNote {
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
        let snapshot = snapshot_optional(&session).unwrap();

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
            {"section":"second","bars":1,"tonic":"A","scale":"harmonic_minor"}
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
        let linked = snapshot_optional(&session).unwrap();
        session.score_source = None;
        let unlinked = snapshot_optional(&session).unwrap();

        // Assert
        assert_eq!(
            linked
                .sections
                .iter()
                .map(|section| (
                    section.occurrence,
                    section.key.as_ref().map(|key| (
                        key.tonic.as_str(),
                        key.scale.as_deref(),
                        key.tonic_pc,
                        key.scale_pcs.clone()
                    ))
                ))
                .collect::<Vec<_>>(),
            [
                (
                    1,
                    Some(("F", Some("ionian"), Some(5), vec![0, 2, 4, 5, 7, 9, 10]))
                ),
                (
                    1,
                    Some((
                        "A",
                        Some("harmonic_minor"),
                        Some(9),
                        vec![0, 2, 4, 5, 8, 9, 11]
                    ))
                ),
                (2, None),
            ]
        );
        assert_eq!(
            serde_json::to_value(linked.sections[0].key.as_ref().unwrap()).unwrap(),
            serde_json::json!({
                "tonic": "F",
                "scale": "ionian",
                "tonicPc": 5,
                "scalePcs": [0, 2, 4, 5, 7, 9, 10],
            })
        );
        assert!(
            unlinked
                .sections
                .iter()
                .all(|section| section.key.is_none())
        );
    }

    fn keyed_sections(yaml: &str) -> Vec<VisualSection> {
        let mut session = CreativeSession::new(0);
        session.settings.mixdown.musical_end_tick = 3840;
        session.arrangement.regions = vec![TimelineRegion {
            id: "region:1".into(),
            name: "first".into(),
            start_tick: TimelineTick(0),
            end_tick: TimelineTick(3840),
        }];
        session.score_source = Some(ScoreSource {
            file_name: "score.yaml".into(),
            yaml: yaml.into(),
            path: None,
            bindings: Default::default(),
            production_inputs: Default::default(),
        });
        snapshot_optional(&session).unwrap().sections
    }

    #[test]
    fn custom_scales_resolve_through_their_degree_definitions() {
        // Arrange
        let custom = r#"{
            "meta":{"title":"Custom","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
            "scale_definitions":{"hexatonic":["1","2","b3","4","5","b7"]},
            "form":[{"section":"first","bars":1,"tonic":"D","scale":"hexatonic"}],
            "sections":{"first":{}},
            "instruments":{"lead":{"preset":"TEST/001-lead"}}
        }"#;
        let unspecified = r#"{
            "meta":{"title":"Unspecified","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
            "form":[{"section":"first","bars":1,"tonic":"D"}],
            "sections":{"first":{}},
            "instruments":{"lead":{"preset":"TEST/001-lead"}}
        }"#;

        // Act
        let custom = keyed_sections(custom);
        let unspecified = keyed_sections(unspecified);

        // Assert
        assert_eq!(
            custom[0].key.as_ref().map(|key| (
                key.scale.as_deref(),
                key.tonic_pc,
                key.scale_pcs.as_slice()
            )),
            Some((Some("hexatonic"), Some(2), &[0, 2, 4, 5, 7, 9][..]))
        );
        assert_eq!(
            unspecified[0].key.as_ref().map(|key| (
                key.scale.as_deref(),
                key.tonic_pc,
                key.scale_pcs.len()
            )),
            Some((None, Some(2), 0))
        );
        assert_eq!(
            serde_json::to_value(unspecified[0].key.as_ref().unwrap()).unwrap(),
            serde_json::json!({"tonic": "D", "tonicPc": 2})
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
        let snapshot = snapshot_optional(&session).unwrap();

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
        let snapshot = snapshot_optional(&session).unwrap();

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
