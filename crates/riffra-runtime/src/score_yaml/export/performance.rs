//! Score performance content: notes, drums, control events, harmony.

use super::{
    Export, ExportError, ExportFile, ExportTrack, Original, SongSection, TrackRole, add_file,
    clips_for, file_identifier, fraction, instrument_of, omit, rational, to_quarters,
};
use crate::api::output::ScoreExportOmission;
use crate::instrument::BuiltInInstrumentCatalog;
use crate::score::syntax;
use num_rational::BigRational;
use riffra_core::{
    AudioClip, FadeShape, HarmonyEvent, InstrumentControlEventKind, InternalInstrumentResource,
    KeySwitchMode, TrackInstrumentSource,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
#[derive(PartialEq)]
struct Line {
    instrument: String,
    part: String,
    material: Vec<String>,
}

/// One section's rendered body.
#[derive(Default, PartialEq)]
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
    /// Audio clips in canonical order, without their Score IDs.
    audio: Vec<AudioDeclaration>,
    notes: Option<String>,
    tonic: String,
    scale: Option<String>,
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
#[derive(Clone)]
struct Hit {
    part: String,
    start: u64,
    length: u64,
    velocity: u8,
}

/// One audio clip of a section before its Score ID is assigned.
#[derive(PartialEq)]
struct AudioDeclaration {
    name: String,
    body: Value,
}

/// One articulation interval on the absolute timeline.
struct Articulation {
    instrument: String,
    name: String,
    start: u64,
    end: u64,
    /// Source order of the key switch that opens the interval.
    order: u32,
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
    let performance = performance(export)?;
    let articulations = articulations(export);
    for articulation in &articulations {
        export
            .written_articulations
            .entry(articulation.instrument.clone())
            .or_default()
            .insert(articulation.name.clone());
    }
    let mut contents: Vec<Content> = Vec::new();
    let mut names = Vec::new();
    let mut used = BTreeSet::new();
    let mut bodies = Vec::with_capacity(export.sections.len());
    for index in 0..export.sections.len() {
        let section = export.sections[index].clone();
        let content = build(export, &section, &performance, &articulations)?;
        let body = contents.iter().position(|other| other == &content);
        bodies.push(match body {
            Some(body) => body,
            None => {
                names.push(super::unique_id(
                    &export.sections[index].base,
                    "section",
                    &mut used,
                ));
                contents.push(content);
                contents.len() - 1
            }
        });
    }
    let mut melodies = Map::new();
    let mut drums = Map::new();
    let mut sections = Map::new();
    let mut materials = BTreeMap::<Vec<String>, String>::new();
    let mut clip_ids = BTreeSet::new();
    for (body, content) in contents.into_iter().enumerate() {
        let name = &names[body];
        let mut section = Map::new();
        if let Some(lines) = lines(&content, name, &mut melodies, &mut materials) {
            section.insert("lines".into(), lines);
        }
        if !content.drums.is_empty() {
            drums.insert(format!("{name}_drums"), drum_phrase(&content));
            section.insert("drums".into(), Value::String(format!("{name}_drums")));
        }
        if !content.placements.is_empty() {
            section.insert("placements".into(), Value::Array(content.placements));
        }
        if !content.events.is_empty() {
            section.insert("events".into(), Value::Array(content.events));
        }
        if let Some(chords) = content.chords {
            section.insert(
                "chords".into(),
                Value::Array(chords.into_iter().map(Value::String).collect()),
            );
        }
        if !content.chord_changes.is_empty() {
            section.insert("chord_changes".into(), Value::Array(content.chord_changes));
        }
        if !content.audio.is_empty() {
            section.insert(
                "audio_clips".into(),
                Value::Array(
                    content
                        .audio
                        .into_iter()
                        .map(|clip| {
                            let AudioDeclaration {
                                name: clip_name,
                                mut body,
                            } = clip;
                            body["id"] = json!(super::unique_id(&clip_name, "clip", &mut clip_ids));
                            body
                        })
                        .collect(),
                ),
            );
        }
        if let Some(velocity) = content.velocity {
            section.insert("velocity".into(), json!(velocity));
        }
        if let Some(notes) = content.notes {
            section.insert("notes".into(), Value::String(notes));
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
    let session = export.session;
    let tracks = &export.tracks;
    let omissions = &mut export.omissions;
    for track in tracks {
        for clip in session
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == track.track.id)
        {
            let subject = format!("track \"{}\" / clip \"{}\"", track.track.name, clip.name);
            if clip.muted {
                omit(omissions, subject.clone(), "the clip is muted");
                continue;
            }
            if clip.loop_enabled {
                omit(omissions, subject.clone(), "the clip loops");
            }
            if clip.notes.iter().any(|note| note.lyric.is_some()) {
                omit(
                    omissions,
                    subject.clone(),
                    "the Score cannot store note lyrics",
                );
            }
            for note in &clip.notes {
                if note.channel != 1 {
                    omit(
                        omissions,
                        format!(
                            "{} / note {} at tick {}",
                            subject,
                            note.note,
                            clip.start_tick.0.saturating_add(note.start_tick.0)
                        ),
                        format!(
                            "the Score cannot store MIDI channel {}; the note is written on channel 1",
                            note.channel
                        ),
                    );
                }
            }
        }
    }
}

fn lines(
    content: &Content,
    section: &str,
    melodies: &mut Map<String, Value>,
    materials: &mut BTreeMap<Vec<String>, String>,
) -> Option<Value> {
    let mut result = Map::new();
    for line in &content.lines {
        let name = materials.entry(line.material.clone()).or_insert_with(|| {
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

/// The material every section of the song is rendered from.
struct Performance {
    voices: BTreeMap<String, Vec<Vec<Note>>>,
    hits: BTreeMap<String, Vec<Hit>>,
    /// Parsed instrument definitions by Score track ID.
    definitions: BTreeMap<String, Value>,
}

/// Collects the playable notes and drum hits of every Track.
fn performance(export: &mut Export<'_>) -> Result<Performance, ExportError> {
    let tracks = &export.tracks;
    let session = export.session;
    let kits = &export.kits;
    let catalog = export.catalog;
    let data_root = export.data_root;
    let omissions = &mut export.omissions;
    let mut voices = BTreeMap::new();
    let mut hits = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    for track in tracks {
        let rules = note_rules(catalog, data_root, track)?;
        match &track.role {
            TrackRole::Instrument => {
                let mut notes = Vec::new();
                for clip in clips_for(session, &track.track.id) {
                    if clip.muted {
                        continue;
                    }
                    for note in &clip.notes {
                        let note = Note {
                            start: clip.start_tick.0 + note.start_tick.0,
                            end: clip.start_tick.0 + note.start_tick.0 + note.duration_ticks,
                            midi: note.note,
                            velocity: note.velocity,
                        };
                        if playable(omissions, track, rules.as_ref(), note) {
                            notes.push(note);
                        }
                    }
                }
                voices.insert(track.track.id.clone(), assign_voices(notes));
            }
            TrackRole::Kit(kit) => {
                let members = kits.get(kit).map(Vec::as_slice).unwrap_or_default();
                let mut list = Vec::new();
                for clip in clips_for(session, &track.track.id) {
                    if clip.muted {
                        continue;
                    }
                    for note in &clip.notes {
                        let note = Note {
                            start: clip.start_tick.0 + note.start_tick.0,
                            end: clip.start_tick.0 + note.start_tick.0 + note.duration_ticks,
                            midi: note.note,
                            velocity: note.velocity,
                        };
                        if !playable(omissions, track, rules.as_ref(), note) {
                            continue;
                        }
                        let part = members
                            .iter()
                            .find(|member| member.trigger == note.midi)
                            .expect("every kit trigger has a member")
                            .id
                            .clone();
                        list.push(Hit {
                            part,
                            start: note.start,
                            length: note.end - note.start,
                            velocity: note.velocity,
                        });
                    }
                }
                list.sort_by_key(|hit| (hit.start, hit.part.clone(), hit.velocity));
                hits.insert(track.track.id.clone(), list);
            }
            TrackRole::Audio => {}
        }
        if let Some(definition) = rules {
            definitions.insert(track.id.clone(), definition);
        }
    }
    Ok(Performance {
        voices,
        hits,
        definitions,
    })
}

/// Resolves the definition that will be written for one Track.
///
/// The Score reads built-in presets from the catalog and user snapshots from
/// the package copied into the song folder, so note rules come from those
/// sources rather than the canonical `definition_json` copy.
fn note_rules(
    catalog: &BuiltInInstrumentCatalog,
    data_root: &Path,
    track: &ExportTrack<'_>,
) -> Result<Option<Value>, ExportError> {
    let Some(instrument) = instrument_of(track.track) else {
        return Ok(None);
    };
    let TrackInstrumentSource::Internal { resource, .. } = &instrument.source else {
        return Ok(None);
    };
    match resource {
        InternalInstrumentResource::BuiltInPreset { preset_id } => {
            let definition = catalog.resolve(preset_id).map_err(|error| {
                ExportError::new(format!("built-in preset {preset_id}: {error}"))
            })?;
            parse_definition(
                &format!("built-in preset {preset_id}"),
                &definition.definition_json,
            )
            .map(Some)
        }
        InternalInstrumentResource::UserSnapshot { snapshot_id, .. } => {
            let path = data_root
                .join("project-instruments")
                .join(snapshot_id)
                .join("definition.json");
            let text = std::fs::read_to_string(&path).map_err(|error| {
                ExportError::new(format!(
                    "user instrument {snapshot_id} definition could not be read: {error}"
                ))
            })?;
            parse_definition(&format!("user instrument {snapshot_id}"), &text).map(Some)
        }
    }
}

fn parse_definition(description: &str, text: &str) -> Result<Value, ExportError> {
    let definition: Value = serde_json::from_str(text).map_err(|error| {
        ExportError::new(format!("{description} definition is invalid: {error}"))
    })?;
    if definition["layers"].as_array().is_none() {
        return Err(ExportError::new(format!(
            "{description} definition has no layers"
        )));
    }
    Ok(definition)
}

/// Reports whether one note can trigger the instrument.
fn playable(
    omissions: &mut Vec<ScoreExportOmission>,
    track: &ExportTrack<'_>,
    definition: Option<&Value>,
    note: Note,
) -> bool {
    let Some(definition) = definition else {
        return true;
    };
    if let Err(reason) =
        crate::score_yaml::import::validate_note(definition, note.midi, note.velocity)
    {
        omit(
            omissions,
            format!(
                "track \"{}\" / note {} at tick {}",
                track.track.name, note.midi, note.start
            ),
            reason,
        );
        return false;
    }
    true
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
    section: &SongSection,
    performance: &Performance,
    articulations: &[Articulation],
) -> Result<Content, ExportError> {
    let audio = audio(export, section)?;
    let mut content = Content {
        tonic: section.tonic.clone(),
        scale: section.scale.clone(),
        audio,
        ..Content::default()
    };
    let tracks = &export.tracks;
    let session = export.session;
    let omissions = &mut export.omissions;
    let velocity = section_velocity(tracks, section, &performance.voices);
    for track in tracks {
        match &track.role {
            TrackRole::Instrument => {
                let lanes = performance
                    .voices
                    .get(&track.track.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
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
                let velocity = velocity.unwrap_or(100);
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
                            material: melody(section, export.ppq, &continuous, velocity),
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
                let hits = performance
                    .hits
                    .get(&track.track.id)
                    .map(|hits| {
                        hits.iter()
                            .filter(|hit| {
                                hit.start >= section.start_tick && hit.start < section.end_tick
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if hits.is_empty() {
                    continue;
                }
                match drum_grid(section, export.ppq, &hits) {
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
    content.velocity = velocity;
    content.events = events(
        session,
        tracks,
        export.ppq,
        omissions,
        section,
        &performance.definitions,
        articulations,
    );
    content.events.append(&mut content.hits);
    let (chords, chord_changes, omitted) =
        harmony(&session.arrangement.harmony_events, section, export.ppq);
    content.chords = chords;
    content.chord_changes = chord_changes;
    omissions.extend(omitted);
    if let Some(original) = export.original.as_ref() {
        content.notes = original
            .section_notes(&section.base)
            .filter(|notes| !notes.is_empty())
            .map(str::to_owned);
    }
    Ok(content)
}

/// Returns the most common velocity of the section, preferring the lower one.
fn section_velocity(
    tracks: &[ExportTrack<'_>],
    section: &SongSection,
    voices: &BTreeMap<String, Vec<Vec<Note>>>,
) -> Option<u8> {
    let notes = tracks.iter().filter_map(|track| {
        let TrackRole::Instrument = track.role else {
            return None;
        };
        Some(voices.get(&track.track.id)?.iter().flatten())
    });
    let mut counts = BTreeMap::new();
    for note in notes
        .flatten()
        .filter(|note| note.start >= section.start_tick && note.start < section.end_tick)
    {
        *counts.entry(note.velocity).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, std::cmp::Reverse(*value)))
        .map(|(value, _)| value)
}

/// Builds one bar list of melody tokens for a voice.
fn melody(section: &SongSection, ppq: u32, notes: &[Note], velocity: u8) -> Vec<String> {
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
                tokens.push(note_token(section, note, velocity, steps(length, ppq)));
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

/// Renders one pitched token, naming the velocity when it differs.
fn note_token(section: &SongSection, note: &Note, velocity: u8, length: String) -> String {
    let degree = degree(section, note.midi);
    if note.velocity == velocity {
        format!("{degree}:{length}")
    } else {
        format!("{degree}@{}:{length}", note.velocity)
    }
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

fn drum_grid(
    section: &SongSection,
    ppq: u32,
    hits: &[Hit],
) -> Option<BTreeMap<String, Vec<String>>> {
    let cell = u64::from(section_cell(ppq)?);
    let mut grid: BTreeMap<String, Vec<Vec<char>>> = BTreeMap::new();
    for hit in hits {
        let token = match hit.velocity {
            64 => 'o',
            96 => 'x',
            120 => 'X',
            _ => return None,
        };
        let index = section
            .bar_starts
            .iter()
            .rposition(|start| *start <= hit.start)?;
        let bar_start = section.bar_starts[index];
        let cells = section.bar_lengths[index] / cell;
        if !section.bar_lengths[index].is_multiple_of(cell)
            || hit.length != cell
            || !(hit.start - bar_start).is_multiple_of(cell)
        {
            return None;
        }
        let column = usize::try_from((hit.start - bar_start) / cell).ok()?;
        if column >= usize::try_from(cells).ok()? {
            return None;
        }
        let bars = grid.entry(hit.part.clone()).or_insert_with(|| {
            section
                .bar_lengths
                .iter()
                .map(|length| vec!['.'; usize::try_from(length / cell).unwrap_or(0)])
                .collect()
        });
        if bars[index][column] != '.' {
            return None;
        }
        bars[index][column] = token;
    }
    let mut result = BTreeMap::new();
    for (part, bars) in grid {
        result.insert(part, compress(bars));
    }
    Some(result)
}

/// Returns the drum cell length in ticks, or `None` when the clock has no
/// exact sixteenth-note grid.
fn section_cell(ppq: u32) -> Option<u32> {
    ppq.is_multiple_of(4).then_some(ppq / 4)
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

/// Builds the audio clip declarations of one section.
fn audio(
    export: &mut Export<'_>,
    section: &SongSection,
) -> Result<Vec<AudioDeclaration>, ExportError> {
    let mut result = Vec::new();
    let tracks = export
        .tracks
        .iter()
        .filter(|track| matches!(track.role, TrackRole::Audio))
        .map(|track| {
            (
                track.id.clone(),
                track.track.id.clone(),
                track.track.name.clone(),
            )
        })
        .collect::<Vec<_>>();
    for (instrument, track_id, track_name) in tracks {
        for clip in export
            .session
            .arrangement
            .audio_clips
            .iter()
            .filter(|clip| clip.track_id == track_id)
        {
            if clip.start_tick.0 < section.start_tick || clip.start_tick.0 >= section.end_tick {
                continue;
            }
            if !exportable_clip(export, clip, &track_name) {
                continue;
            }
            if clip.fade_shape != FadeShape::EqualPower {
                omit(
                    &mut export.omissions,
                    format!("clip \"{}\" / fade shape", clip.name),
                    "the Score only stores equal-power fades",
                );
            }
            let file = audio_file(export, clip)?;
            let rate = f64::from(clip.source_sample_rate);
            let mut body = json!({
                "instrument": instrument,
                "file": file,
                "at": section.position(clip.start_tick.0, export.ppq),
                "offset_seconds": clip.source_range.start as f64 / rate,
                "duration_seconds": clip.source_range.end.saturating_sub(clip.source_range.start) as f64 / rate,
                "gain_db": clip.gain_db,
                "pan": clip.pan,
            });
            for (key, fade) in [
                ("fade_in_seconds", &clip.fade_in),
                ("fade_out_seconds", &clip.fade_out),
            ] {
                if fade.frames > 0 {
                    body[key] = json!(fade.frames as f64 / f64::from(fade.sample_rate));
                }
            }
            result.push(AudioDeclaration {
                name: clip.name.clone(),
                body,
            });
        }
    }
    Ok(result)
}

/// Reports whether one audio clip can be written and reports the rest.
fn exportable_clip(export: &mut Export<'_>, clip: &AudioClip, track_name: &str) -> bool {
    if clip.muted {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip is muted",
        );
        return false;
    }
    if clip.loop_enabled {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip loops",
        );
        return false;
    }
    if clip.take_variant != riffra_core::AudioTakeVariant::Raw {
        omit(
            &mut export.omissions,
            format!("clip \"{}\"", clip.name),
            "the clip uses a processed take",
        );
        return false;
    }
    let source_frames = u128::from(
        clip.source_range
            .end
            .saturating_sub(clip.source_range.start),
    );
    let source_rate = u128::from(clip.source_sample_rate);
    let timeline_frames = u128::from(clip.timeline_duration.frames);
    let timeline_rate = u128::from(clip.timeline_duration.sample_rate);
    if source_rate == 0
        || timeline_rate == 0
        || source_frames.saturating_mul(timeline_rate)
            != timeline_frames.saturating_mul(source_rate)
    {
        omit(
            &mut export.omissions,
            format!(
                "track \"{track_name}\" / clip \"{}\" / playback length",
                clip.name
            ),
            "the clip plays for a different length than its source",
        );
    }
    true
}

/// Registers the WAV of one asset once and returns its Score path.
fn audio_file(export: &mut Export<'_>, clip: &AudioClip) -> Result<String, ExportError> {
    if let Some(file) = export.audio_files.get(&clip.asset_id) {
        return Ok(file.clone());
    }
    let Some(asset) = crate::asset::load(export.data_root, &clip.asset_id)
        .map_err(|error| ExportError::new(format!("asset could not be read: {error}")))?
    else {
        return Err(ExportError::new(format!(
            "clip \"{}\" references a missing asset",
            clip.name
        )));
    };
    let Some(location) =
        crate::asset::resolve_content_location(export.data_root, &clip.asset_id)
            .map_err(|error| ExportError::new(format!("asset could not be read: {error}")))?
    else {
        return Err(ExportError::new(format!(
            "clip \"{}\" references a missing file",
            clip.name
        )));
    };
    let name = file_identifier(&asset.name);
    let mut relative = format!("audio/{name}.wav");
    let mut number = 2;
    while export.files.contains_key(&relative) {
        relative = format!("audio/{name}_{number}.wav");
        number += 1;
    }
    add_file(
        export,
        relative.clone(),
        ExportFile::Copy(PathBuf::from(location)),
    )?;
    export
        .audio_files
        .insert(clip.asset_id.clone(), relative.clone());
    Ok(relative)
}

/// Renders every performance control of one section.
fn events(
    session: &riffra_core::CreativeSession,
    tracks: &[ExportTrack<'_>],
    ppq: u32,
    omissions: &mut Vec<ScoreExportOmission>,
    section: &SongSection,
    definitions: &BTreeMap<String, Value>,
    articulations: &[Articulation],
) -> Vec<Value> {
    let mut events: Vec<(u64, u32, Value)> = Vec::new();
    for track in tracks {
        match track.role {
            TrackRole::Kit(_) => {
                report_kit_controls(session, omissions, section, track);
                continue;
            }
            TrackRole::Audio => continue,
            TrackRole::Instrument => {}
        }
        let vst3 =
            instrument_of(track.track).is_some_and(|instrument| instrument.as_vst3().is_some());
        let definition = definitions.get(&track.id);
        let mut controls: Vec<(u64, u32, InstrumentControlEventKind)> = Vec::new();
        for clip in clips_for(session, &track.track.id) {
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
                    if !vst3 {
                        omit(
                            omissions,
                            format!(
                                "track \"{}\" / clip \"{}\" / MIDI CC at tick {tick}",
                                track.track.name, clip.name
                            ),
                            "the Score cannot send MIDI CC to a built-in instrument",
                        );
                        continue;
                    }
                    if event.channel != 1 {
                        omit(
                            omissions,
                            format!(
                                "track \"{}\" / clip \"{}\" / MIDI CC at tick {tick}",
                                track.track.name, clip.name
                            ),
                            format!(
                                "the Score cannot store MIDI channel {}; the CC is written on channel 1",
                                event.channel
                            ),
                        );
                    }
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
                        omissions,
                        format!("track \"{}\" / MIDI event", track.track.name),
                        "the Score cannot express this MIDI event",
                    );
                }
            }
        }
        controls.sort_by_key(|(tick, order, _)| (*tick, *order));
        let mut points: Vec<(u64, u32, Point)> = Vec::new();
        for (tick, order, control) in controls {
            let at = section.position(tick, ppq);
            if vst3
                && !matches!(
                    control,
                    InstrumentControlEventKind::ControlChange { .. }
                        | InstrumentControlEventKind::KeySwitch { .. }
                )
            {
                omit(
                    omissions,
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
                } => {
                    if let Err(reason) =
                        internal_parameter(vst3, definition, &parameter, &[native_value])
                    {
                        omit(
                            omissions,
                            format!("track \"{}\" / parameter {parameter}", track.track.name),
                            reason,
                        );
                        continue;
                    }
                    Control::Event(json!({
                        "kind": "parameter_change",
                        "instrument": track.id,
                        "at": at,
                        "parameter": parameter,
                        "native_value": native_value,
                    }))
                }
                InstrumentControlEventKind::ParameterRamp {
                    parameter,
                    from_value,
                    to_value,
                    duration_ticks,
                } => {
                    if tick + duration_ticks > section.end_tick {
                        omit(
                            omissions,
                            format!("track \"{}\" / control", track.track.name),
                            "a parameter ramp crosses a section boundary",
                        );
                        continue;
                    }
                    if let Err(reason) =
                        internal_parameter(vst3, definition, &parameter, &[from_value, to_value])
                    {
                        omit(
                            omissions,
                            format!("track \"{}\" / parameter {parameter}", track.track.name),
                            reason,
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
                        "duration_quarters": rational(duration_ticks, ppq),
                    }))
                }
            };
            match event {
                Control::Point(point) => points.push((tick, order, point)),
                Control::Event(event) => events.push((tick, order, event)),
            }
        }
        for (tick, order, event) in coalesce(&points, ppq, section, &track.id) {
            events.push((tick, order, event));
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
                articulation.order,
                json!({
                    "kind": "articulation",
                    "instrument": track.id,
                    "name": articulation.name,
                    "at": section.position(start, ppq),
                    "duration_quarters": rational(end - start, ppq),
                }),
            ));
        }
    }
    events.sort_by_key(|(tick, order, _)| (*tick, *order));
    events.into_iter().map(|(_, _, event)| event).collect()
}

/// Reports the kit controls the Score cannot address per drum part.
fn report_kit_controls(
    session: &riffra_core::CreativeSession,
    omissions: &mut Vec<ScoreExportOmission>,
    section: &SongSection,
    track: &ExportTrack<'_>,
) {
    for clip in clips_for(session, &track.track.id) {
        if clip.muted {
            continue;
        }
        for event in &clip.instrument_control_events {
            let tick = clip.start_tick.0 + event.tick.0;
            if tick >= section.start_tick && tick < section.end_tick {
                omit(
                    omissions,
                    format!("track \"{}\" / control", track.track.name),
                    "the Score cannot send control events to a kit track",
                );
            }
        }
        for event in &clip.events {
            let tick = clip.start_tick.0 + event.tick.0;
            if tick >= section.start_tick && tick < section.end_tick {
                omit(
                    omissions,
                    format!("track \"{}\" / MIDI event", track.track.name),
                    "the Score cannot express this MIDI event",
                );
            }
        }
    }
}

/// Validates a built-in instrument parameter event against its definition.
fn internal_parameter(
    vst3: bool,
    definition: Option<&Value>,
    parameter: &str,
    values: &[f32],
) -> Result<(), String> {
    if vst3 {
        return Err("the Score cannot change a VST3 instrument parameter".into());
    }
    let Some(definition) = definition else {
        return Err("the track has no built-in instrument definition".into());
    };
    crate::score_yaml::import::validate_parameter(definition, parameter, values)
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

    /// Returns whether the Score expands this control as an integer CC.
    fn is_cc(self) -> bool {
        matches!(self.kind, PointKind::Cc(_))
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
    points: &[(u64, u32, Point)],
    ppq: u32,
    section: &SongSection,
    instrument: &str,
) -> Vec<(u64, u32, Value)> {
    let mut result = Vec::new();
    if !ppq.is_multiple_of(64) {
        for (tick, order, point) in points {
            result.push((
                *tick,
                *order,
                point_json(section, ppq, *tick, instrument, &point_body(*point)),
            ));
        }
        return result;
    }
    let step = u64::from(ppq) / 64;
    let mut run_end = vec![0; points.len()];
    for index in (0..points.len()).rev() {
        let next = index + 1;
        run_end[index] = if next < points.len()
            && points[next].0 == points[index].0 + step
            && points[next].2.kind == points[index].2.kind
        {
            run_end[next]
        } else {
            index
        };
    }
    let mut index = 0;
    while index < points.len() {
        let limit = run_end[index];
        let mut end = index;
        if limit > index {
            let limit = if points[index].2.is_cc() {
                limit.min(index + 127)
            } else {
                geometric_limit(points, index, limit)
            };
            for candidate in (index + 1..=limit).rev() {
                let run = &points[index..=candidate];
                if !linear_at_end(run) || !matches(run, ppq) {
                    continue;
                }
                end = candidate;
                break;
            }
        }
        let (tick, order, point) = points[index];
        if end > index {
            let last = points[end].2;
            let (from_value, to_value) = if point.is_cc() {
                (json!(point.value as u8), json!(last.value as u8))
            } else {
                (json!(point.value), json!(last.value))
            };
            result.push((
                tick,
                order,
                point_json(
                    section,
                    ppq,
                    tick,
                    instrument,
                    &json!({
                        "kind": "automation",
                        "control": point.control(),
                        "from_value": from_value,
                        "to_value": to_value,
                        "duration_quarters": rational(points[end].0 - tick, ppq),
                    }),
                ),
            ));
        } else {
            result.push((
                tick,
                order,
                point_json(section, ppq, tick, instrument, &point_body(point)),
            ));
        }
        index = end + 1;
    }
    result
}

/// Narrows a run to the prefix one straight line can describe.
///
/// A longer ramp that starts at the same point must stay on the line through
/// the current end, so a point that leaves it ends every possible match.
fn geometric_limit(points: &[(u64, u32, Point)], start: usize, limit: usize) -> usize {
    let first = points[start].2;
    let mut end = start + 1;
    while end < limit {
        let last = points[end].2;
        let next = points[end + 1].2;
        let ratio =
            (points[end + 1].0 - points[start].0) as f64 / (points[end].0 - points[start].0) as f64;
        let expected = (f64::from(first.value)
            + (f64::from(last.value) - f64::from(first.value)) * ratio)
            as f32;
        let scale = first
            .value
            .abs()
            .max(last.value.abs())
            .max(next.value.abs())
            .max(1.0);
        if (next.value - expected).abs() > 16.0 * f32::EPSILON * scale {
            break;
        }
        end += 1;
    }
    end
}

/// Renders one control that is not an automation run.
fn point_body(point: Point) -> Value {
    let mut body = json!({
        "kind": point.name(),
        "value": point.value,
    });
    if let PointKind::Cc(number) = point.kind {
        body["kind"] = json!("cc");
        body["number"] = json!(number);
        body["value"] = json!(point.value as u8);
    }
    body
}

fn point_json(section: &SongSection, ppq: u32, tick: u64, instrument: &str, body: &Value) -> Value {
    let mut body = body.clone();
    body["instrument"] = json!(instrument);
    body["at"] = section.position(tick, ppq);
    body
}

/// The ramp value at one tick, using the Score importer's arithmetic.
fn ramp_value(start: u64, end: u64, tick: u64, from: f32, to: f32) -> f32 {
    if tick <= start {
        return from;
    }
    if tick >= end {
        return to;
    }
    let ratio = (tick - start) as f64 / (end - start) as f64;
    (f64::from(from) + (f64::from(to) - f64::from(from)) * ratio) as f32
}

/// The integer CC value at one tick, using the Score importer's rounding.
fn cc_ramp_value(start: u64, end: u64, tick: u64, from: u8, to: u8) -> u8 {
    ramp_value(start, end, tick, f32::from(from), f32::from(to)).round() as u8
}

/// Rejects a candidate whose last interior point is off the ramp.
fn linear_at_end(points: &[(u64, u32, Point)]) -> bool {
    let Some(interior) = points.len().checked_sub(2) else {
        return false;
    };
    if interior == 0 {
        return true;
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let point = points[interior];
    if first.2.is_cc() {
        cc_ramp_value(
            first.0,
            last.0,
            point.0,
            first.2.value as u8,
            last.2.value as u8,
        ) == point.2.value as u8
    } else {
        ramp_value(first.0, last.0, point.0, first.2.value, last.2.value).to_bits()
            == point.2.value.to_bits()
    }
}

/// Reports whether a run re-expands to exactly its own points.
fn matches(points: &[(u64, u32, Point)], ppq: u32) -> bool {
    if points.len() < 2 {
        return false;
    }
    let step = u64::from(ppq) / 64;
    if step == 0 || points.windows(2).any(|pair| pair[1].0 != pair[0].0 + step) {
        return false;
    }
    let first = points[0].2;
    let last = points[points.len() - 1].2;
    if first.kind != last.kind || points.iter().any(|(_, _, point)| point.kind != first.kind) {
        return false;
    }
    let start = points[0].0;
    let end = points[points.len() - 1].0;
    if first.is_cc() {
        let from = first.value as u8;
        let to = last.value as u8;
        let mut expected = Vec::new();
        let mut cursor = start;
        let mut previous = None;
        loop {
            let value = cc_ramp_value(start, end, cursor, from, to);
            if previous != Some(value) {
                expected.push((cursor, value));
                previous = Some(value);
            }
            if cursor == end {
                break;
            }
            cursor = (cursor + step).min(end);
        }
        return expected.len() == points.len()
            && expected
                .iter()
                .zip(points)
                .all(|((tick, value), (at, _, point))| {
                    *tick == *at && *value == point.value as u8
                });
    }
    points.iter().all(|(tick, _, point)| {
        ramp_value(start, end, *tick, first.value, last.value).to_bits() == point.value.to_bits()
    })
}

/// Reconstructs articulation intervals from their key switch sends.
fn articulations(export: &mut Export<'_>) -> Vec<Articulation> {
    let mut result = Vec::new();
    let song_end = export.sections.last().map_or(0, |section| section.end_tick);
    let mut omissions = Vec::new();
    for track in &export.tracks {
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
        let Some(declarations) = track
            .source_id()
            .and_then(|id| {
                export
                    .original
                    .as_ref()
                    .and_then(|original| original.articulations(id))
            })
            .and_then(Value::as_object)
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
        let mut active: Option<(usize, u64, u32, KeySwitchMode, bool)> = None;
        let mut previous_end = None;
        let mut restored = false;
        let mut released = BTreeSet::new();
        for (tick, press_order, note, mode, down) in sends {
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
                if let Some((active_position, press, order, _, same_hold_release)) = active.take() {
                    push_interval(
                        &mut result,
                        &mut omissions,
                        track,
                        &named[active_position].2,
                        order,
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
                if let Some((active_position, press, order, _, same_hold_release)) = active.take() {
                    push_interval(
                        &mut result,
                        &mut omissions,
                        track,
                        &named[active_position].2,
                        order,
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
            if let Some((active_position, press, order, _, released_before)) = active.take() {
                push_interval(
                    &mut result,
                    &mut omissions,
                    track,
                    &named[active_position].2,
                    order,
                    export.ppq,
                    press,
                    tick,
                    previous_end,
                    restored,
                    released_before,
                );
                previous_end = Some(tick);
            }
            active = Some((position, tick, press_order, mode, same_hold_release));
            restored = false;
            released.clear();
        }
        if let Some((position, press, order, _, _)) = active {
            push_interval(
                &mut result,
                &mut omissions,
                track,
                &named[position].2,
                order,
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
    order: u32,
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
        order,
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
    harmony_events: &[HarmonyEvent],
    section: &SongSection,
    ppq: u32,
) -> (Option<Vec<String>>, Vec<Value>, Vec<ScoreExportOmission>) {
    let mut omissions = Vec::new();
    let mut events = harmony_events
        .iter()
        .filter_map(|event| {
            let start = event.start_tick.0.max(section.start_tick);
            let end = event.end_tick.0.min(section.end_tick);
            (end > start).then_some((start, end, event))
        })
        .collect::<Vec<_>>();
    if events.is_empty() {
        return (None, Vec::new(), omissions);
    }
    events.sort_by_key(|(start, _, _)| *start);
    let mut written = Vec::new();
    let mut previous: Option<(u64, &str)> = None;
    for (start, end, event) in events {
        if let Some((previous_end, previous_name)) = previous
            && start < previous_end
        {
            omit(
                &mut omissions,
                format!("harmony \"{}\"", event.chord.name),
                format!(
                    "the chord overlaps \"{previous_name}\" and cannot be written at the same time"
                ),
            );
            continue;
        }
        previous = Some((end, &event.chord.name));
        if let Some(chord) = chord(section, event) {
            written.push((start, end, chord));
        } else {
            omit(
                &mut omissions,
                format!("harmony \"{}\"", event.chord.name),
                "the chord cannot be written in Score notation",
            );
        }
    }
    if written.is_empty() {
        return (None, Vec::new(), omissions);
    }
    let bar_start = |tick: u64| section.bar_starts.iter().position(|start| *start == tick);
    let boundary = |tick: u64| {
        if tick == section.end_tick {
            Some(section.bars())
        } else {
            bar_start(tick)
        }
    };
    let per_bar = written[0].0 == section.start_tick
        && written
            .last()
            .is_some_and(|(_, end, _)| *end == section.end_tick)
        && written.windows(2).all(|pair| pair[0].1 == pair[1].0)
        && written
            .iter()
            .all(|(start, end, _)| boundary(*start).is_some() && boundary(*end).is_some());
    if per_bar {
        let mut bars = vec![String::new(); section.bars()];
        for (start, end, chord) in &written {
            let from = boundary(*start).expect("bar start");
            let to = boundary(*end).expect("bar end");
            bars[from..to].fill(chord.clone());
        }
        return (Some(bars), Vec::new(), omissions);
    }
    let mut changes: Vec<(u64, String)> = Vec::new();
    for (index, (start, end, chord)) in written.iter().enumerate() {
        changes.push((*start, chord.clone()));
        let next = written
            .get(index + 1)
            .map_or(section.end_tick, |(start, _, _)| *start);
        if *end < next {
            changes.push((*end, "NC".into()));
        }
    }
    let changes = changes
        .into_iter()
        .map(|(start, chord)| {
            json!({
                "at": section.position(start, ppq),
                "chord": chord,
            })
        })
        .collect();
    (None, changes, omissions)
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
    let bass_interval = (bass - root).rem_euclid(12);
    let mut without_bass = tones.clone();
    without_bass.remove(&bass_interval);
    let quality_of = |intervals: &BTreeSet<i32>| {
        let intervals = intervals.iter().copied().collect::<Vec<_>>();
        QUALITIES
            .iter()
            .find(|(_, semitones)| {
                let mut expected = semitones.to_vec();
                expected.sort_unstable();
                expected == intervals
            })
            .map(|(name, _)| *name)
    };
    let quality = if bass == root {
        quality_of(&tones)
    } else {
        quality_of(&without_bass).or_else(|| quality_of(&tones))
    }?;
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
    use riffra_core::{HarmonyChord, HarmonyEvent, MusicalNoteName, TimelineTick};

    fn note_name(name: &str) -> MusicalNoteName {
        MusicalNoteName::new(name).unwrap()
    }

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
            100,
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
        let grid = drum_grid(&section(), 480, &hits).expect("the hits fall on the grid");
        assert_eq!(grid["kick"], ["x...............", "................"]);

        let mut off = hits;
        off[0].start = 60;
        off[0].length = 60;
        assert!(drum_grid(&section(), 480, &off).is_none());
    }

    #[test]
    fn drum_grids_follow_the_meter_and_keep_every_hit() {
        let mut waltz = section();
        waltz.meter = (3, 8);
        waltz.end_tick = 1440;
        waltz.bar_lengths = vec![720, 720];
        waltz.bar_starts = vec![0, 720];
        let hits = vec![
            Hit {
                part: "kick".into(),
                start: 0,
                length: 120,
                velocity: 96,
            },
            Hit {
                part: "kick".into(),
                start: 480,
                length: 120,
                velocity: 96,
            },
            Hit {
                part: "snare".into(),
                start: 720,
                length: 120,
                velocity: 120,
            },
        ];
        let grid = drum_grid(&waltz, 480, &hits).expect("3/8 has six sixteenth cells");
        assert_eq!(grid["kick"], ["x...x.", "......"]);
        assert_eq!(grid["snare"], ["......", "X....."]);

        let mut repeated = hits.clone();
        repeated.push(Hit {
            part: "kick".into(),
            start: 0,
            length: 120,
            velocity: 64,
        });
        assert!(
            drum_grid(&waltz, 480, &repeated).is_none(),
            "two hits in one cell cannot both be written"
        );

        let mut short = waltz.clone();
        short.bar_lengths = vec![720, 360];
        let short_hits = vec![
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
        let grid = drum_grid(&short, 480, &short_hits).expect("a short bar keeps the cell length");
        assert_eq!(grid["snare"], ["......", "..X"]);

        let mut off = hits;
        off[0].velocity = 100;
        assert!(drum_grid(&waltz, 480, &off).is_none());
    }

    #[test]
    fn automation_runs_coalesce_only_when_the_expansion_matches() {
        let points = [
            (0, 0, Point::value(PointKind::ModWheel, 0.0)),
            (15, 1, Point::value(PointKind::ModWheel, 0.5)),
            (30, 2, Point::value(PointKind::ModWheel, 1.0)),
        ];
        let section = section();
        let events = coalesce(&points, 960, &section, "lead");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2["kind"], "automation");
        assert_eq!(events[0].2["control"], "mod_wheel");

        let broken = [
            (0, 0, Point::value(PointKind::ModWheel, 0.0)),
            (15, 1, Point::value(PointKind::ModWheel, 0.9)),
            (30, 2, Point::value(PointKind::ModWheel, 1.0)),
        ];
        assert_eq!(coalesce(&broken, 960, &section, "lead").len(), 2);

        let cc = [
            (0, 0, Point::cc(11, 0)),
            (15, 1, Point::cc(11, 64)),
            (30, 2, Point::cc(11, 127)),
        ];
        let events = coalesce(&cc, 960, &section, "lead");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2["kind"], "automation");
        assert_eq!(events[0].2["control"], "cc:11");
        assert_eq!(events[0].2["from_value"], 0);
        assert_eq!(events[0].2["to_value"], 127);

        let off = [
            (0, 0, Point::cc(11, 0)),
            (15, 1, Point::cc(11, 63)),
            (30, 2, Point::cc(11, 127)),
        ];
        let events = coalesce(&off, 960, &section, "lead");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].2["from_value"], 0);
        assert_eq!(events[0].2["to_value"], 63);

        let repeated = [
            (0, 0, Point::cc(11, 0)),
            (15, 1, Point::cc(11, 0)),
            (30, 2, Point::cc(11, 127)),
        ];
        let events = coalesce(&repeated, 960, &section, "lead");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].2["kind"], "cc");
        assert_eq!(events[1].2["kind"], "automation");
    }

    fn harmony_event(
        name: &str,
        tones: &[&str],
        root: &str,
        bass: &str,
        start: u64,
        end: u64,
    ) -> HarmonyEvent {
        HarmonyEvent {
            id: format!("harmony:{start}"),
            start_tick: TimelineTick(start),
            end_tick: TimelineTick(end),
            chord: HarmonyChord::from_explicit(
                tones.iter().map(|tone| note_name(tone)).collect(),
                Some(note_name(root)),
                Some(note_name(bass)),
                Some(name.into()),
            )
            .unwrap(),
        }
    }

    #[test]
    fn harmony_uses_bar_chords_only_for_continuous_bar_aligned_events() {
        let section = section();
        let (chords, changes, omissions) = harmony(
            &[
                harmony_event("C", &["C", "E", "G"], "C", "C", 0, 1920),
                harmony_event("G", &["G", "B", "D"], "G", "G", 1920, 3840),
            ],
            &section,
            960,
        );
        assert_eq!(chords.unwrap(), ["1:maj", "5:maj"]);
        assert!(changes.is_empty());
        assert!(omissions.is_empty());

        let (chords, changes, omissions) = harmony(
            &[harmony_event("C", &["C", "E", "G"], "C", "C", 0, 5000)],
            &section,
            960,
        );
        assert_eq!(chords.unwrap(), ["1:maj", "1:maj"]);
        assert!(changes.is_empty());
        assert!(omissions.is_empty());
    }

    #[test]
    fn harmony_reports_overlaps_and_keeps_silent_gaps() {
        let section = section();
        let (chords, changes, omissions) = harmony(
            &[
                harmony_event("C", &["C", "E", "G"], "C", "C", 0, 960),
                harmony_event("G", &["G", "B", "D"], "G", "G", 1920, 2880),
            ],
            &section,
            960,
        );
        assert!(chords.is_none());
        assert!(omissions.is_empty());
        assert_eq!(
            changes
                .iter()
                .map(|change| change["chord"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["1:maj", "NC", "5:maj", "NC"]
        );
        assert_eq!(
            changes
                .iter()
                .map(|change| change["at"].clone())
                .collect::<Vec<_>>(),
            [
                json!({"bar": 1, "step": 1}),
                json!({"bar": 1, "step": 5}),
                json!({"bar": 2, "step": 1}),
                json!({"bar": 2, "step": 5}),
            ]
        );

        let (chords, changes, omissions) = harmony(
            &[
                harmony_event("C", &["C", "E", "G"], "C", "C", 0, 1920),
                harmony_event("G", &["G", "B", "D"], "G", "G", 960, 3840),
            ],
            &section,
            960,
        );
        assert!(chords.is_none());
        assert_eq!(
            changes
                .iter()
                .map(|change| change["chord"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["1:maj", "NC"]
        );
        assert_eq!(omissions.len(), 1);
        assert!(omissions[0].message.contains("overlaps"));
    }

    #[test]
    fn slash_basses_do_not_change_the_chord_quality() {
        let harmony = HarmonyChord::from_explicit(
            ["D", "C", "E", "G"].into_iter().map(note_name).collect(),
            Some(note_name("C")),
            Some(note_name("D")),
            Some("C/D".into()),
        )
        .unwrap();
        let event = HarmonyEvent {
            id: "harmony".into(),
            start_tick: TimelineTick(0),
            end_tick: TimelineTick(3840),
            chord: harmony,
        };
        assert_eq!(chord(&section(), &event), Some("1:maj/2".into()));
    }

    #[test]
    fn long_automation_runs_keep_their_exact_compression() {
        let values = (0..2000)
            .map(|index| {
                (
                    index as u64 * 15,
                    index as u32,
                    Point::value(
                        PointKind::ModWheel,
                        f32::from(u8::try_from(index % 2).unwrap()),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let events = coalesce(&values, 960, &section(), "lead");
        assert_eq!(events.len(), 1000);
        assert!(
            events
                .iter()
                .all(|(_, _, event)| event["kind"] == "automation")
        );

        let linear = (0..4000)
            .map(|index| {
                (
                    index as u64 * 15,
                    index as u32,
                    Point::value(
                        PointKind::ModWheel,
                        ramp_value(0, 3999 * 15, index as u64 * 15, 0.0, 1.0),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let events = coalesce(&linear, 960, &section(), "lead");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2["kind"], "automation");

        let mut broken = (0..500)
            .map(|index| {
                (
                    index as u64 * 15,
                    index as u32,
                    Point::value(
                        PointKind::ModWheel,
                        ramp_value(0, 499 * 15, index as u64 * 15, 0.0, 1.0),
                    ),
                )
            })
            .collect::<Vec<_>>();
        broken.push((500 * 15, 500, Point::value(PointKind::ModWheel, 0.75)));
        let events = coalesce(&broken, 960, &section(), "lead");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].2["kind"], "automation");
        assert_eq!(events[0].2["duration_quarters"], rational(499 * 15, 960));
        assert_eq!(events[1].2["kind"], "mod_wheel");
    }
}
