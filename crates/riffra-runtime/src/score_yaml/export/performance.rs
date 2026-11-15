//! Score performance content: notes, drums, control events, harmony.

use super::{
    Export, ExportError, ExportTrack, Original, SongSection, TrackRole, clips_for, fraction,
    instrument_of, omit, rational, to_quarters,
};
use crate::api::output::ScoreExportOmission;
use crate::score::syntax;
use num_rational::BigRational;
use riffra_core::{HarmonyEvent, InstrumentControlEventKind, KeySwitchMode};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Scale spellings used when a section names a known scale.
const SCALES: &[(&str, &[&str])] = &[
    ("ionian", &["1", "2", "3", "4", "5", "6", "7"]),
    ("dorian", &["1", "2", "b3", "4", "5", "6", "b7"]),
    ("phrygian", &["1", "b2", "b3", "4", "5", "b6", "b7"]),
    ("lydian", &["1", "2", "3", "#4", "5", "6", "7"]),
    ("mixolydian", &["1", "2", "3", "4", "5", "6", "b7"]),
    ("aeolian", &["1", "2", "b3", "4", "5", "b6", "b7"]),
    ("locrian", &["1", "b2", "b3", "4", "b5", "b6", "b7"]),
    ("harmonic_minor", &["1", "2", "b3", "4", "5", "b6", "7"]),
    ("melodic_minor", &["1", "2", "b3", "4", "5", "6", "7"]),
    ("major_pentatonic", &["1", "2", "3", "5", "6"]),
    ("minor_pentatonic", &["1", "b3", "4", "5", "b7"]),
    ("whole_tone", &["1", "2", "3", "#4", "#5", "#6"]),
];

/// Chromatic spellings used for notes outside the section scale.
const CHROMATIC: &[&str] = &[
    "1", "b2", "2", "b3", "3", "4", "#4", "5", "b6", "6", "b7", "7",
];

/// Chord qualities recognized when a harmony event has no Score spelling.
const QUALITIES: &[(&str, &[i32])] = &[
    ("maj", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("maj7", &[0, 4, 7, 11]),
    ("7", &[0, 4, 7, 10]),
    ("m7", &[0, 3, 7, 10]),
    ("m7b5", &[0, 3, 6, 10]),
    ("dim7", &[0, 3, 6, 9]),
    ("mMaj7", &[0, 3, 7, 11]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("maj9", &[0, 2, 4, 7, 11]),
    ("9", &[0, 2, 4, 7, 10]),
    ("m9", &[0, 2, 3, 7, 10]),
    ("add9", &[0, 2, 4, 7]),
    ("7sus4", &[0, 5, 7, 10]),
    ("11", &[0, 2, 4, 5, 7, 10]),
    ("13", &[0, 2, 4, 5, 7, 9, 10]),
];

/// One melodic line in a section.
struct Line {
    instrument: String,
    part: String,
    material: Vec<String>,
}

/// One section's rendered body.
#[derive(Default)]
struct Content {
    velocity: Option<u8>,
    lines: Vec<Line>,
    placements: Vec<Value>,
    events: Vec<Value>,
    chords: Option<Vec<String>>,
    chord_changes: Vec<Value>,
    /// Kit ID, then member ID, then one bar pattern per entry.
    drums: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Kit hits when a section cannot use drum grids.
    hits: Vec<Value>,
    notes: Option<String>,
    tonic: String,
    scale: Option<String>,
}

impl Content {
    fn fingerprint(&self) -> String {
        serde_json::to_string(&json!({
            "velocity": self.velocity,
            "lines": self
                .lines
                .iter()
                .map(|line| (&line.instrument, &line.part, &line.material))
                .collect::<Vec<_>>(),
            "placements": self.placements,
            "events": self.events,
            "chords": self.chords,
            "chord_changes": self.chord_changes,
            "drums": self.drums,
            "hits": self.hits,
            "notes": self.notes,
            "tonic": self.tonic,
            "scale": self.scale,
        }))
        .expect("section content serializes")
    }
}

/// One note on the absolute timeline.
#[derive(Clone, Copy, Debug)]
struct Note {
    start: u64,
    end: u64,
    midi: u8,
    velocity: u8,
}

/// One drum hit on the absolute timeline.
struct Hit {
    part: String,
    start: u64,
    length: u64,
    velocity: u8,
}

/// One articulation interval on the absolute timeline.
struct Articulation {
    instrument: String,
    name: String,
    start: u64,
    end: u64,
}

/// Renders every section body and writes the performance documents.
///
/// # Errors
/// Returns an error when a section cannot be expressed.
pub(super) fn apply(
    export: &mut Export<'_>,
    document: &mut Map<String, Value>,
) -> Result<(), ExportError> {
    report_clips(export);
    let voices = voices(export);
    let articulations = articulations(export);
    let mut contents = Vec::with_capacity(export.sections.len());
    for index in 0..export.sections.len() {
        let section = export.sections[index].clone();
        contents.push(build(export, index, &section, &voices, &articulations)?);
    }
    let mut fingerprints: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut used = BTreeSet::new();
    let mut bodies = Vec::with_capacity(contents.len());
    for (index, content) in contents.iter().enumerate() {
        let fingerprint = content.fingerprint();
        let body = match fingerprints.iter().position(|other| *other == fingerprint) {
            Some(position) => position,
            None => {
                names.push(unique_name(&export.sections[index].base, &mut used));
                fingerprints.push(fingerprint);
                fingerprints.len() - 1
            }
        };
        bodies.push(body);
    }
    let mut melodies = Map::new();
    let mut drums = Map::new();
    let mut sections = Map::new();
    let mut materials = BTreeMap::<String, String>::new();
    for (body, name) in names.iter().enumerate() {
        let content = &contents[first_body(&bodies, body)];
        let mut section = Map::new();
        if let Some(lines) = lines(content, name, &mut melodies, &mut materials) {
            section.insert("lines".into(), lines);
        }
        if !content.drums.is_empty() {
            drums.insert(format!("{name}_drums"), drum_phrase(content));
            section.insert("drums".into(), Value::String(format!("{name}_drums")));
        }
        if !content.placements.is_empty() {
            section.insert(
                "placements".into(),
                Value::Array(content.placements.clone()),
            );
        }
        if !content.events.is_empty() {
            section.insert("events".into(), Value::Array(content.events.clone()));
        }
        if let Some(chords) = &content.chords {
            section.insert(
                "chords".into(),
                Value::Array(
                    chords
                        .iter()
                        .map(|chord| Value::String(chord.clone()))
                        .collect(),
                ),
            );
        }
        if !content.chord_changes.is_empty() {
            section.insert(
                "chord_changes".into(),
                Value::Array(content.chord_changes.clone()),
            );
        }
        if let Some(velocity) = content.velocity {
            section.insert("velocity".into(), json!(velocity));
        }
        if let Some(notes) = &content.notes {
            section.insert("notes".into(), Value::String(notes.clone()));
        }
        sections.insert(name.clone(), Value::Object(section));
    }
    for (index, section) in export.sections.iter_mut().enumerate() {
        section.name.clone_from(&names[bodies[index]]);
    }
    document.insert("melodies".into(), Value::Object(melodies));
    if !drums.is_empty() {
        document.insert("drums".into(), Value::Object(drums));
    }
    if let Some(transitions) = export.original.as_ref().and_then(Original::transitions) {
        document.insert("transitions".into(), transitions.clone());
    }
    document.insert("sections".into(), Value::Object(sections));
    Ok(())
}

/// Reports the MIDI clips the Score cannot carry.
fn report_clips(export: &mut Export<'_>) {
    let clips = export.session.arrangement.midi_clips.clone();
    let tracks = export.tracks.clone();
    for track in &tracks {
        for clip in clips.iter().filter(|clip| clip.track_id == track.track.id) {
            let subject = format!("track \"{}\" / clip \"{}\"", track.track.name, clip.name);
            if clip.muted {
                omit(&mut export.omissions, subject.clone(), "the clip is muted");
                continue;
            }
            if clip.loop_enabled {
                omit(&mut export.omissions, subject.clone(), "the clip loops");
            }
            if clip.notes.iter().any(|note| note.lyric.is_some()) {
                omit(
                    &mut export.omissions,
                    subject,
                    "the Score cannot store note lyrics",
                );
            }
        }
    }
}

fn first_body(bodies: &[usize], body: usize) -> usize {
    bodies
        .iter()
        .position(|candidate| *candidate == body)
        .expect("a body occurs at least once")
}

fn lines(
    content: &Content,
    section: &str,
    melodies: &mut Map<String, Value>,
    materials: &mut BTreeMap<String, String>,
) -> Option<Value> {
    let mut result = Map::new();
    for line in &content.lines {
        let key = serde_json::to_string(&line.material).expect("material serializes");
        let name = materials.entry(key).or_insert_with(|| {
            let name = format!("{section}_{}", line.part);
            melodies.insert(
                name.clone(),
                Value::Array(
                    line.material
                        .iter()
                        .map(|bar| Value::String(bar.clone()))
                        .collect(),
                ),
            );
            name
        });
        result.insert(
            line.part.clone(),
            json!({"instrument": line.instrument, "melody": name}),
        );
    }
    (!result.is_empty()).then_some(Value::Object(result))
}

fn drum_phrase(content: &Content) -> Value {
    let mut parts = Map::new();
    for members in content.drums.values() {
        for (member, bars) in members {
            parts.insert(
                member.clone(),
                Value::Array(bars.iter().map(|bar| Value::String(bar.clone())).collect()),
            );
        }
    }
    json!({"parts": parts})
}

fn unique_name(base: &str, used: &mut BTreeSet<String>) -> String {
    let stem = super::identifier(base).unwrap_or_else(|| "section".into());
    let mut candidate = stem.clone();
    let mut number = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}_{number}");
        number += 1;
    }
    candidate
}

/// Assigns every note to the lowest-numbered voice that is free.
fn voices(export: &Export<'_>) -> BTreeMap<String, Vec<Vec<Note>>> {
    let mut result = BTreeMap::new();
    for track in &export.tracks {
        if matches!(track.role, TrackRole::Kit(_) | TrackRole::Audio) {
            continue;
        }
        let mut notes = Vec::new();
        for clip in clips_for(export.session, &track.track.id) {
            if clip.muted {
                continue;
            }
            notes.extend(clip.notes.iter().map(|note| Note {
                start: clip.start_tick.0 + note.start_tick.0,
                end: clip.start_tick.0 + note.start_tick.0 + note.duration_ticks,
                midi: note.note,
                velocity: note.velocity,
            }));
        }
        result.insert(track.track.id.clone(), assign_voices(notes));
    }
    result
}

/// Splits notes into voices, using the lowest free voice for each onset.
fn assign_voices(mut notes: Vec<Note>) -> Vec<Vec<Note>> {
    notes.sort_by_key(|note| (note.start, note.end, note.midi, note.velocity));
    let mut lanes: Vec<Vec<Note>> = Vec::new();
    for note in notes {
        let lane = lanes
            .iter()
            .position(|lane| lane.last().is_none_or(|last| last.end <= note.start));
        match lane {
            Some(index) => lanes[index].push(note),
            None => lanes.push(vec![note]),
        }
    }
    lanes
}

fn build(
    export: &mut Export<'_>,
    _index: usize,
    section: &SongSection,
    voices: &BTreeMap<String, Vec<Vec<Note>>>,
    articulations: &[Articulation],
) -> Result<Content, ExportError> {
    let mut content = Content {
        tonic: section.tonic.clone(),
        scale: section.scale.clone(),
        ..Content::default()
    };
    let tracks = export.tracks.clone();
    let mut section_velocity: Option<u8> = None;
    for track in &tracks {
        match &track.role {
            TrackRole::Instrument => {
                let lanes = voices.get(&track.track.id).cloned().unwrap_or_default();
                let notes = lanes
                    .iter()
                    .flatten()
                    .filter(|note| {
                        note.start >= section.start_tick && note.start < section.end_tick
                    })
                    .collect::<Vec<_>>();
                if notes.is_empty() {
                    continue;
                }
                let velocity = mode(notes.iter().map(|note| note.velocity));
                section_velocity = Some(match section_velocity {
                    Some(current) => mode([current, velocity].into_iter()),
                    None => velocity,
                });
                for (number, lane) in lanes.iter().enumerate() {
                    let part = if number == 0 {
                        track.id.clone()
                    } else {
                        format!("{}_{}", track.id, number + 1)
                    };
                    let local = lane
                        .iter()
                        .copied()
                        .filter(|note| {
                            note.start >= section.start_tick && note.start < section.end_tick
                        })
                        .collect::<Vec<_>>();
                    if local.is_empty() {
                        continue;
                    }
                    let continuous = local
                        .iter()
                        .filter(|note| note.end <= section.end_tick)
                        .copied()
                        .collect::<Vec<_>>();
                    if !continuous.is_empty() {
                        content.lines.push(Line {
                            instrument: track.id.clone(),
                            part,
                            material: melody(section, export.ppq, &continuous),
                        });
                    }
                    for note in local.iter().filter(|note| note.end > section.end_tick) {
                        content
                            .placements
                            .push(placement(section, export.ppq, &track.id, note, velocity));
                    }
                }
            }
            TrackRole::Kit(kit) => {
                let hits = kit_hits(export, section, &track.track.id, kit);
                if hits.is_empty() {
                    continue;
                }
                match drum_grid(section, &hits) {
                    Some(drums) => {
                        content.drums.insert(kit.clone(), drums);
                    }
                    None => {
                        for hit in &hits {
                            content
                                .hits
                                .push(hit_event(section, export.ppq, &hit.part, hit));
                        }
                    }
                }
            }
            TrackRole::Audio => {}
        }
    }
    content.velocity = section_velocity;
    content.events = events(export, section, articulations);
    content.events.append(&mut content.hits);
    let (chords, chord_changes) = harmony(export, section, export.ppq);
    content.chords = chords;
    content.chord_changes = chord_changes;
    if let Some(original) = export.original.as_ref() {
        content.notes = original
            .section_notes(&section.base)
            .filter(|notes| !notes.is_empty())
            .map(str::to_owned);
    }
    Ok(content)
}

fn mode(values: impl Iterator<Item = u8>) -> u8 {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, std::cmp::Reverse(*value)))
        .map_or(100, |(value, _)| value)
}

/// Builds one bar list of melody tokens for a voice.
fn melody(section: &SongSection, ppq: u32, notes: &[Note]) -> Vec<String> {
    let mut bars = Vec::new();
    for (index, start) in section.bar_starts.iter().enumerate() {
        let length = section.bar_lengths[index];
        let end = start + length;
        let mut segments = Vec::new();
        for note in notes {
            if note.start >= end || note.end <= *start {
                continue;
            }
            let from = note.start.max(*start);
            let to = note.end.min(end);
            segments.push((from - start, to - from, note, note.start < *start));
        }
        segments.sort_by_key(|(at, _, note, tie)| (*at, *tie, note.midi));
        let mut tokens = Vec::new();
        let mut cursor = 0;
        for (at, length, note, tie) in segments {
            if at > cursor {
                tokens.push(format!(".:{}", steps(at - cursor, ppq)));
            }
            if tie {
                tokens.push(format!("-:{}", steps(length, ppq)));
            } else {
                tokens.push(format!(
                    "{}:{}",
                    degree(section, note.midi),
                    steps(length, ppq)
                ));
            }
            cursor = at + length;
        }
        if cursor < length {
            tokens.push(format!(".:{}", steps(length - cursor, ppq)));
        }
        bars.push(tokens.join(" "));
    }
    bars
}

fn steps(ticks: u64, ppq: u32) -> String {
    fraction(&(to_quarters(ticks, ppq) * BigRational::from_integer(4.into())))
}

/// Renders a note crossing the section end as an exact placement.
fn placement(
    section: &SongSection,
    ppq: u32,
    instrument: &str,
    note: &Note,
    velocity: u8,
) -> Value {
    let mut value = json!({
        "instrument": instrument,
        "degree": degree(section, note.midi),
        "at": section.position(note.start, ppq),
        "duration_quarters": rational(note.end - note.start, ppq),
    });
    if note.velocity != velocity {
        value["velocity"] = json!(note.velocity);
    }
    value
}

/// Maps a MIDI note to a degree token for its section.
fn degree(section: &SongSection, midi: u8) -> String {
    let tonic = syntax::tonic(&section.tonic).unwrap_or(0);
    let semitones = i32::from(midi) - (60 + tonic);
    let octave = semitones.div_euclid(12);
    let pitch = semitones.rem_euclid(12);
    let spelling = section
        .scale
        .as_deref()
        .and_then(scale_spelling)
        .and_then(|scale| {
            scale.iter().find_map(|name| {
                (syntax::degree(name).ok()?.rem_euclid(12) == pitch).then(|| (*name).to_owned())
            })
        })
        .unwrap_or_else(|| CHROMATIC[pitch as usize].to_owned());
    match octave {
        0 => spelling,
        n if n > 0 => format!("{spelling}{}", "'".repeat(n as usize)),
        n => format!("{spelling}{}", ",".repeat((-n) as usize)),
    }
}

fn scale_spelling(name: &str) -> Option<&'static [&'static str]> {
    SCALES
        .iter()
        .find(|(scale, _)| *scale == name)
        .map(|(_, degrees)| *degrees)
}

fn kit_hits(export: &Export<'_>, section: &SongSection, track_id: &str, kit: &str) -> Vec<Hit> {
    let members = export.kits.get(kit).cloned().unwrap_or_default();
    let mut hits = Vec::new();
    for clip in clips_for(export.session, track_id) {
        if clip.muted {
            continue;
        }
        for note in &clip.notes {
            let start = clip.start_tick.0 + note.start_tick.0;
            if start < section.start_tick || start >= section.end_tick {
                continue;
            }
            let part = members
                .iter()
                .find(|member| member.trigger == note.note)
                .map_or_else(
                    || format!("{kit}_{}", note.note),
                    |member| member.id.clone(),
                );
            hits.push(Hit {
                part,
                start,
                length: note.duration_ticks,
                velocity: note.velocity,
            });
        }
    }
    hits.sort_by_key(|hit| (hit.start, hit.part.clone(), hit.velocity));
    hits
}

fn drum_grid(section: &SongSection, hits: &[Hit]) -> Option<BTreeMap<String, Vec<String>>> {
    let mut grid: BTreeMap<String, Vec<Vec<char>>> = BTreeMap::new();
    for hit in hits {
        if !matches!(hit.velocity, 64 | 96 | 120) {
            return None;
        }
        let index = section
            .bar_starts
            .iter()
            .rposition(|start| *start <= hit.start)?;
        let bar_start = section.bar_starts[index];
        let cells = usize::from(section.meter.0) * 4;
        let cell = BigRational::new(section.bar_lengths[index].into(), cells.into());
        let offset = BigRational::from_integer((hit.start - bar_start).into());
        let duration = BigRational::from_integer(hit.length.into());
        if duration != cell || !(offset.clone() / cell.clone()).is_integer() {
            return None;
        }
        let column = usize::try_from((offset / cell).to_integer()).ok()?;
        if column >= cells {
            return None;
        }
        let bars = grid
            .entry(hit.part.clone())
            .or_insert_with(|| vec![vec!['.'; cells]; section.bars()]);
        bars[index][column] = match hit.velocity {
            64 => 'o',
            96 => 'x',
            _ => 'X',
        };
    }
    let mut result = BTreeMap::new();
    for (part, bars) in grid {
        result.insert(part, compress(bars));
    }
    Some(result)
}

/// Shortens a bar list to its smallest repeating unit.
fn compress(bars: Vec<Vec<char>>) -> Vec<String> {
    let rendered = bars
        .iter()
        .map(|bar| bar.iter().collect::<String>())
        .collect::<Vec<_>>();
    for period in 1..=rendered.len() {
        if rendered.len().is_multiple_of(period)
            && rendered
                .iter()
                .enumerate()
                .all(|(index, bar)| *bar == rendered[index % period])
        {
            return rendered[..period].to_vec();
        }
    }
    rendered
}

fn hit_event(section: &SongSection, ppq: u32, part: &str, hit: &Hit) -> Value {
    let mut value = json!({
        "kind": "hit",
        "instrument": part,
        "at": section.position(hit.start, ppq),
        "velocity": hit.velocity,
    });
    if hit.length != u64::from(ppq) / 4 {
        value["duration_steps"] = json!(steps(hit.length, ppq));
    }
    value
}

/// Renders every performance control of one section.
fn events(
    export: &mut Export<'_>,
    section: &SongSection,
    articulations: &[Articulation],
) -> Vec<Value> {
    let mut events = Vec::new();
    for track in export.tracks.clone() {
        let TrackRole::Instrument = track.role else {
            continue;
        };
        let vst3 =
            instrument_of(track.track).is_some_and(|instrument| instrument.as_vst3().is_some());
        let mut controls: Vec<(u64, u32, InstrumentControlEventKind)> = Vec::new();
        for clip in clips_for(export.session, &track.track.id) {
            if clip.muted {
                continue;
            }
            for event in &clip.instrument_control_events {
                let tick = clip.start_tick.0 + event.tick.0;
                if tick >= section.start_tick && tick < section.end_tick {
                    controls.push((tick, event.source_order, event.kind.clone()));
                }
            }
            for event in &clip.events {
                let tick = clip.start_tick.0 + event.tick.0;
                if tick < section.start_tick || tick >= section.end_tick {
                    continue;
                }
                if event.kind == riffra_core::MidiEventKind::ControlChange {
                    controls.push((
                        tick,
                        u32::MAX,
                        InstrumentControlEventKind::ControlChange {
                            number: event.data1,
                            value: event.data2,
                        },
                    ));
                } else {
                    omit(
                        &mut export.omissions,
                        format!("track \"{}\" / MIDI event", track.track.name),
                        "the Score cannot express this MIDI event",
                    );
                }
            }
        }
        controls.sort_by_key(|(tick, order, _)| (*tick, *order));
        let mut points = Vec::new();
        for (tick, _, control) in controls {
            let at = section.position(tick, export.ppq);
            if vst3 && !matches!(control, InstrumentControlEventKind::ControlChange { .. }) {
                omit(
                    &mut export.omissions,
                    format!("track \"{}\" / control", track.track.name),
                    "Score sends only note, articulation, and cc events to VST3 tracks",
                );
                continue;
            }
            let event = match control {
                InstrumentControlEventKind::KeySwitch { .. } => continue,
                InstrumentControlEventKind::ControlChange { number, value } => {
                    Control::Point(Point::cc(number, value))
                }
                InstrumentControlEventKind::SustainPedal { down } => Control::Event(json!({
                    "kind": "sustain_pedal",
                    "instrument": track.id,
                    "at": at,
                    "down": down,
                })),
                InstrumentControlEventKind::PitchBend { value } => {
                    Control::Point(Point::value(PointKind::PitchBend, value))
                }
                InstrumentControlEventKind::ModWheel { value } => {
                    Control::Point(Point::value(PointKind::ModWheel, value))
                }
                InstrumentControlEventKind::Aftertouch { value } => {
                    Control::Point(Point::value(PointKind::Aftertouch, value))
                }
                InstrumentControlEventKind::ParameterChange {
                    parameter,
                    native_value,
                } => Control::Event(json!({
                    "kind": "parameter_change",
                    "instrument": track.id,
                    "at": at,
                    "parameter": parameter,
                    "native_value": native_value,
                })),
                InstrumentControlEventKind::ParameterRamp {
                    parameter,
                    from_value,
                    to_value,
                    duration_ticks,
                } => {
                    if tick + duration_ticks > section.end_tick {
                        omit(
                            &mut export.omissions,
                            format!("track \"{}\" / control", track.track.name),
                            "a parameter ramp crosses a section boundary",
                        );
                        continue;
                    }
                    Control::Event(json!({
                        "kind": "automation",
                        "instrument": track.id,
                        "at": at,
                        "parameter": parameter,
                        "from_value": from_value,
                        "to_value": to_value,
                        "duration_quarters": rational(duration_ticks, export.ppq),
                    }))
                }
            };
            match event {
                Control::Point(point) => points.push((tick, point)),
                Control::Event(event) => events.push((tick, event)),
            }
        }
        for (tick, event) in coalesce(&points, export.ppq, section, &track.id) {
            events.push((tick, event));
        }
        for articulation in articulations
            .iter()
            .filter(|articulation| articulation.instrument == track.id)
        {
            let start = articulation.start.max(section.start_tick);
            let end = articulation.end.min(section.end_tick);
            if end <= start {
                continue;
            }
            events.push((
                start,
                json!({
                    "kind": "articulation",
                    "instrument": track.id,
                    "name": articulation.name,
                    "at": section.position(start, export.ppq),
                    "duration_quarters": rational(end - start, export.ppq),
                }),
            ));
        }
    }
    events.sort_by_key(|(tick, _)| *tick);
    events.into_iter().map(|(_, event)| event).collect()
}

/// One control event of a section, before automation runs are detected.
enum Control {
    Point(Point),
    Event(Value),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PointKind {
    ModWheel,
    Aftertouch,
    PitchBend,
    Cc(u8),
}

#[derive(Clone, Copy)]
struct Point {
    kind: PointKind,
    value: f32,
}

impl Point {
    fn value(kind: PointKind, value: f32) -> Self {
        Self { kind, value }
    }

    fn cc(number: u8, value: u8) -> Self {
        Self {
            kind: PointKind::Cc(number),
            value: f32::from(value),
        }
    }

    fn control(self) -> String {
        match self.kind {
            PointKind::ModWheel => "mod_wheel".into(),
            PointKind::Aftertouch => "aftertouch".into(),
            PointKind::PitchBend => "pitch_bend".into(),
            PointKind::Cc(number) => format!("cc:{number}"),
        }
    }

    fn name(self) -> &'static str {
        match self.kind {
            PointKind::ModWheel => "mod_wheel",
            PointKind::Aftertouch => "aftertouch",
            PointKind::PitchBend => "pitch_bend",
            PointKind::Cc(_) => "cc",
        }
    }
}

/// Joins point runs the Score expands back to the same events.
fn coalesce(
    points: &[(u64, Point)],
    ppq: u32,
    section: &SongSection,
    instrument: &str,
) -> Vec<(u64, Value)> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < points.len() {
        let mut end = index;
        if ppq.is_multiple_of(64) {
            for candidate in (index + 1..points.len()).rev() {
                if matches(&points[index..=candidate], ppq) {
                    end = candidate;
                    break;
                }
            }
        }
        let (tick, point) = points[index];
        if end > index {
            let last = points[end];
            let values = if matches!(point.kind, PointKind::Cc(_)) {
                (json!(point.value as u8), json!(last.1.value as u8))
            } else {
                (json!(point.value), json!(last.1.value))
            };
            result.push((
                tick,
                point_json(
                    section,
                    ppq,
                    tick,
                    instrument,
                    &json!({
                        "kind": "automation",
                        "control": point.control(),
                        "from_value": values.0,
                        "to_value": values.1,
                        "duration_quarters": rational(last.0 - tick, ppq),
                    }),
                ),
            ));
        } else {
            let value = if matches!(point.kind, PointKind::Cc(_)) {
                json!(point.value as u8)
            } else {
                json!(point.value)
            };
            let mut body = json!({
                "kind": point.name(),
                "value": value,
            });
            if let PointKind::Cc(number) = point.kind {
                body["kind"] = json!("cc");
                body["number"] = json!(number);
            }
            result.push((tick, point_json(section, ppq, tick, instrument, &body)));
        }
        index = end + 1;
    }
    result
}

fn point_json(section: &SongSection, ppq: u32, tick: u64, instrument: &str, body: &Value) -> Value {
    let mut body = body.clone();
    body["instrument"] = json!(instrument);
    body["at"] = section.position(tick, ppq);
    body
}

/// Reports whether a run re-expands to exactly its own points.
fn matches(points: &[(u64, Point)], ppq: u32) -> bool {
    if points.len() < 2 {
        return false;
    }
    let step = u64::from(ppq) / 64;
    if step == 0 || points.windows(2).any(|pair| pair[1].0 != pair[0].0 + step) {
        return false;
    }
    let first = points[0].1;
    let last = points[points.len() - 1].1;
    if first.kind != last.kind || points.iter().any(|(_, point)| point.kind != first.kind) {
        return false;
    }
    let start = points[0].0;
    let end = points[points.len() - 1].0;
    let mut previous = None;
    let mut expanded = Vec::new();
    for (tick, _) in points {
        let ratio = (tick - start) as f64 / (end - start) as f64;
        let value = if *tick == start {
            first.value
        } else if *tick == end {
            last.value
        } else {
            (f64::from(first.value) + (f64::from(last.value) - f64::from(first.value)) * ratio)
                as f32
        };
        if let PointKind::Cc(_) = first.kind {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = value.round().clamp(0.0, 127.0) as u8;
            if previous != Some(value) {
                expanded.push(f32::from(value));
                previous = Some(value);
            }
        } else {
            expanded.push(value);
        }
    }
    expanded.len() == points.len()
        && expanded
            .iter()
            .zip(points)
            .all(|(value, (_, point))| value.to_bits() == point.value.to_bits())
}

/// Reconstructs articulation intervals from their key switch sends.
fn articulations(export: &mut Export<'_>) -> Vec<Articulation> {
    let mut result = Vec::new();
    let Some(original) = export.original.as_ref() else {
        return result;
    };
    let tracks = export.tracks.clone();
    let song_end = export.sections.last().map_or(0, |section| section.end_tick);
    let mut omissions = Vec::new();
    for track in &tracks {
        let TrackRole::Instrument = track.role else {
            continue;
        };
        let mut sends = Vec::new();
        for clip in clips_for(export.session, &track.track.id) {
            if clip.muted {
                continue;
            }
            for event in &clip.instrument_control_events {
                if let InstrumentControlEventKind::KeySwitch { note, mode, down } = event.kind {
                    sends.push((
                        clip.start_tick.0 + event.tick.0,
                        event.source_order,
                        note,
                        mode,
                        down,
                    ));
                }
            }
        }
        if sends.is_empty() {
            continue;
        }
        sends.sort_by_key(|(tick, order, note, _, down)| (*tick, *order, *note, *down));
        let Some(declarations) = original.articulations(&track.id).and_then(Value::as_object)
        else {
            for (_, _, note, _, _) in &sends {
                omissions.push(ScoreExportOmission {
                    subject: format!("track \"{}\" / key switch {note}", track.track.name),
                    message: "the articulation declaration is not available".into(),
                });
            }
            continue;
        };
        let named = declarations
            .iter()
            .filter_map(|(name, declaration)| {
                let note = declaration.get("keyswitch").and_then(Value::as_u64)?;
                let mode = match declaration.get("mode").and_then(Value::as_str)? {
                    "hold" => KeySwitchMode::Hold,
                    "latch" => KeySwitchMode::Latch,
                    _ => return None,
                };
                Some((u8::try_from(note).ok()?, mode, name.clone()))
            })
            .collect::<Vec<_>>();
        let default = named
            .iter()
            .find(|(_, _, name)| name == "default")
            .map(|(note, _, _)| *note);
        let mut active: Option<(usize, u64, KeySwitchMode, bool)> = None;
        let mut previous_end = None;
        let mut restored = false;
        let mut released = BTreeSet::new();
        for (tick, _, note, mode, down) in sends {
            let Some(position) = named.iter().position(|(candidate, candidate_mode, _)| {
                *candidate == note && *candidate_mode == mode
            }) else {
                omissions.push(ScoreExportOmission {
                    subject: format!("track \"{}\" / key switch {note}", track.track.name),
                    message: "no articulation declares this key switch".into(),
                });
                continue;
            };
            let is_default = default == Some(note);
            if !down {
                if mode == KeySwitchMode::Hold {
                    released.insert(note);
                }
                if let Some((active_position, press, _, same_hold_release)) = active.take() {
                    push_interval(
                        &mut result,
                        &mut omissions,
                        track,
                        &named[active_position].2,
                        export.ppq,
                        press,
                        tick,
                        previous_end,
                        restored,
                        same_hold_release,
                    );
                    previous_end = Some(tick);
                    restored = false;
                }
                continue;
            }
            if is_default {
                if let Some((active_position, press, _, same_hold_release)) = active.take() {
                    push_interval(
                        &mut result,
                        &mut omissions,
                        track,
                        &named[active_position].2,
                        export.ppq,
                        press,
                        tick,
                        previous_end,
                        restored,
                        same_hold_release,
                    );
                    previous_end = Some(tick);
                }
                restored = true;
                continue;
            }
            let same_hold_release = mode == KeySwitchMode::Hold && released.contains(&note);
            if let Some((active_position, press, _, released_before)) = active.take() {
                push_interval(
                    &mut result,
                    &mut omissions,
                    track,
                    &named[active_position].2,
                    export.ppq,
                    press,
                    tick,
                    previous_end,
                    restored,
                    released_before,
                );
                previous_end = Some(tick);
            }
            active = Some((position, tick, mode, same_hold_release));
            restored = false;
            released.clear();
        }
        if let Some((position, press, _, _)) = active {
            push_interval(
                &mut result,
                &mut omissions,
                track,
                &named[position].2,
                export.ppq,
                press,
                song_end,
                previous_end,
                restored,
                false,
            );
        }
    }
    export.omissions.extend(omissions);
    result
}

#[allow(clippy::too_many_arguments)]
fn push_interval(
    result: &mut Vec<Articulation>,
    omissions: &mut Vec<ScoreExportOmission>,
    track: &ExportTrack<'_>,
    name: &str,
    ppq: u32,
    press: u64,
    end: u64,
    previous_end: Option<u64>,
    restored: bool,
    same_hold_release: bool,
) {
    let Some(start) = interval_start(ppq, press, end, previous_end, restored, same_hold_release)
    else {
        omissions.push(ScoreExportOmission {
            subject: format!("track \"{}\" / articulation \"{name}\"", track.track.name),
            message: "a key switch interval is too short to express".into(),
        });
        return;
    };
    result.push(Articulation {
        instrument: track.id.clone(),
        name: name.to_owned(),
        start,
        end,
    });
}

fn interval_start(
    ppq: u32,
    press: u64,
    end: u64,
    previous_end: Option<u64>,
    restored: bool,
    same_hold_release: bool,
) -> Option<u64> {
    if press == 0 {
        return Some(0);
    }
    if previous_end == Some(press) && !restored && !same_hold_release {
        return Some(press);
    }
    let lead = press + u64::from(ppq) / 32;
    if end > lead {
        return Some(lead);
    }
    (end > press + 1).then(|| (press + end) / 2)
}

/// Renders the section harmony as bar chords or explicit changes.
fn harmony(
    export: &Export<'_>,
    section: &SongSection,
    ppq: u32,
) -> (Option<Vec<String>>, Vec<Value>) {
    let mut events = export
        .session
        .arrangement
        .harmony_events
        .iter()
        .filter_map(|event| {
            let start = event.start_tick.0.max(section.start_tick);
            let end = event.end_tick.0.min(section.end_tick);
            (end > start).then_some((start, end, event))
        })
        .collect::<Vec<_>>();
    if events.is_empty() {
        return (None, Vec::new());
    }
    events.sort_by_key(|(start, _, _)| *start);
    let mut changes = Vec::new();
    for (start, end, event) in events {
        if let Some(chord) = chord(section, event) {
            changes.push((start, end, chord));
        } else {
            omit(
                &mut Vec::new(),
                format!("harmony \"{}\"", event.chord.name),
                "",
            );
        }
    }
    if changes.is_empty() {
        return (None, Vec::new());
    }
    let bar_start = |tick: u64| section.bar_starts.iter().position(|start| *start == tick);
    let boundary = |tick: u64| {
        if tick == section.end_tick {
            Some(section.bars())
        } else {
            bar_start(tick)
        }
    };
    let per_bar = changes[0].0 == section.start_tick
        && changes
            .last()
            .is_some_and(|(_, end, _)| *end == section.end_tick)
        && changes.windows(2).all(|pair| pair[0].1 == pair[1].0)
        && changes
            .iter()
            .all(|(start, end, _)| boundary(*start).is_some() && boundary(*end).is_some());
    if per_bar {
        let mut bars = vec![String::new(); section.bars()];
        for (start, end, chord) in &changes {
            let from = boundary(*start).expect("bar start");
            let to = boundary(*end).expect("bar end");
            bars[from..to].fill(chord.clone());
        }
        return (Some(bars), Vec::new());
    }
    let changes = changes
        .into_iter()
        .map(|(start, _, chord)| {
            json!({
                "at": section.position(start, ppq),
                "chord": chord,
            })
        })
        .collect();
    (None, changes)
}

/// Writes one harmony event as a Score chord, preferring its original spelling.
fn chord(section: &SongSection, event: &HarmonyEvent) -> Option<String> {
    if section.inherited && matches!(syntax::chord(&event.chord.name), Ok(Some(_))) {
        return Some(event.chord.name.clone());
    }
    let tonic = syntax::tonic(&section.tonic).ok()?;
    let root = i32::from(event.chord.root?.pitch_class());
    let bass = event
        .chord
        .bass
        .map_or(root, |bass| i32::from(bass.pitch_class()));
    let mut tones = BTreeSet::new();
    tones.insert(0_i32);
    for tone in &event.chord.tones {
        tones.insert((i32::from(tone.pitch_class()) - root).rem_euclid(12));
    }
    tones.insert((bass - root).rem_euclid(12));
    let intervals = tones.into_iter().collect::<Vec<_>>();
    let quality = QUALITIES
        .iter()
        .find(|(_, semitones)| {
            let mut expected = semitones.to_vec();
            expected.sort_unstable();
            expected == intervals
        })
        .map(|(name, _)| *name)?;
    let root_degree = CHROMATIC[usize::try_from((root - tonic).rem_euclid(12)).ok()?];
    let bass_degree = CHROMATIC[usize::try_from((bass - tonic).rem_euclid(12)).ok()?];
    if bass == root {
        Some(format!("{root_degree}:{quality}"))
    } else {
        Some(format!("{root_degree}:{quality}/{bass_degree}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> SongSection {
        SongSection {
            base: "verse".into(),
            name: "verse".into(),
            start_tick: 0,
            end_tick: 3840,
            bar_lengths: vec![1920, 1920],
            bar_starts: vec![0, 1920],
            meter: (4, 4),
            bpm: 120.0,
            tonic: "C".into(),
            scale: None,
            inherited: false,
        }
    }

    fn note(start: u64, end: u64, midi: u8) -> Note {
        Note {
            start,
            end,
            midi,
            velocity: 100,
        }
    }

    #[test]
    fn voices_split_chords_and_overlaps_without_reordering_onsets() {
        let voices = assign_voices(vec![
            note(480, 960, 64),
            note(0, 960, 60),
            note(0, 480, 67),
            note(480, 1920, 71),
        ]);
        assert_eq!(voices.len(), 3);
        assert_eq!(
            voices[0].iter().map(|note| note.midi).collect::<Vec<_>>(),
            [67, 64]
        );
        assert_eq!(
            voices[1].iter().map(|note| note.midi).collect::<Vec<_>>(),
            [60]
        );
        assert_eq!(
            voices[2].iter().map(|note| note.midi).collect::<Vec<_>>(),
            [71]
        );
    }

    #[test]
    fn melodies_tie_across_bar_lines_and_end_with_exact_rests() {
        let ticks = melody(
            &section(),
            480,
            &[note(0, 960, 60), note(960, 2880, 64), note(2880, 3840, 67)],
        );
        assert_eq!(ticks, ["1:8 3:8", "-:8 5:8"]);
    }

    #[test]
    fn degrees_use_the_scale_when_the_pitch_belongs_to_it() {
        let mut section = section();
        section.tonic = "A".into();
        section.scale = Some("aeolian".into());
        assert_eq!(degree(&section, 69), "1");
        assert_eq!(degree(&section, 71), "2");
        assert_eq!(degree(&section, 72), "b3");
        assert_eq!(degree(&section, 70), "b2");
        assert_eq!(degree(&section, 57), "1,");

        section.scale = None;
        section.tonic = "C".into();
        assert_eq!(degree(&section, 66), "#4");
        assert_eq!(degree(&section, 72), "1'");
    }

    #[test]
    fn drum_grids_require_the_bar_grid_and_fall_back_to_hits() {
        let hits = vec![
            Hit {
                part: "kick".into(),
                start: 0,
                length: 120,
                velocity: 96,
            },
            Hit {
                part: "snare".into(),
                start: 960,
                length: 120,
                velocity: 120,
            },
        ];
        let grid = drum_grid(&section(), &hits).expect("the hits fall on the grid");
        assert_eq!(grid["kick"], ["x...............", "................"]);

        let mut off = hits;
        off[0].start = 60;
        off[0].length = 60;
        assert!(drum_grid(&section(), &off).is_none());
    }

    #[test]
    fn automation_runs_coalesce_only_when_the_expansion_matches() {
        let points = [
            (0, Point::value(PointKind::ModWheel, 0.0)),
            (15, Point::value(PointKind::ModWheel, 0.5)),
            (30, Point::value(PointKind::ModWheel, 1.0)),
        ];
        assert!(matches(&points, 960));
        let section = section();
        let events = coalesce(&points, 960, &section, "lead");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["kind"], "automation");
        assert_eq!(events[0].1["control"], "mod_wheel");

        let broken = [
            (0, Point::value(PointKind::ModWheel, 0.0)),
            (15, Point::value(PointKind::ModWheel, 0.9)),
            (30, Point::value(PointKind::ModWheel, 1.0)),
        ];
        assert!(!matches(&broken, 960));
        assert_eq!(coalesce(&broken, 960, &section, "lead").len(), 2);

        let cc = [
            (0, Point::cc(11, 0)),
            (15, Point::cc(11, 64)),
            (30, Point::cc(11, 127)),
        ];
        assert!(matches(&cc, 960));
        let events = coalesce(&cc, 960, &section, "lead");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["control"], "cc:11");
        assert_eq!(events[0].1["from_value"], 0.0);
        assert_eq!(events[0].1["to_value"], 127.0);

        let repeated = [
            (0, Point::cc(11, 5)),
            (15, Point::cc(11, 5)),
            (30, Point::cc(11, 5)),
        ];
        assert!(!matches(&repeated, 960));
        assert_eq!(coalesce(&repeated, 960, &section, "lead").len(), 3);
    }
}
