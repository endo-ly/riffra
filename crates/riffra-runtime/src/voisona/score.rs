use super::state::{Node, PluginState};
use riffra_core::{CreativeSession, ProjectTimebase};
use std::collections::BTreeMap;

// VoiSona's score clock uses 960 units per quarter note.
const SCORE_PPQ: u64 = 960;
const MAX_LOOP_ITERATIONS: u64 = 100_000;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Note {
    clock: i32,
    duration: i32,
    pitch: u8,
    lyric: String,
}

#[derive(Clone, Debug)]
struct ScoreNote {
    id: String,
    pitch: u8,
    start_tick: u64,
    duration_ticks: u64,
    lyric: Option<String>,
    // Out-of-range notes stay silent so transposing back restores their expression.
    out_of_range: bool,
}

#[derive(Clone, Debug)]
struct ScoreClip {
    id: String,
    name: String,
    start_tick: u64,
    duration_ticks: u64,
    notes: Vec<ScoreNote>,
    muted: bool,
    loop_enabled: bool,
}

#[derive(Clone, Debug)]
struct PlannedNote {
    note: Note,
    expression_from: Option<(String, String)>,
    start_tick: u64,
    end_tick: u64,
}

/// The Score-relevant part of one Track, kept without the rest of the project.
#[derive(Clone, Debug)]
pub(crate) struct TrackScore {
    timebase: ProjectTimebase,
    clips: Vec<ScoreClip>,
    end_tick: u64,
}

impl TrackScore {
    pub(crate) fn from_session(session: &CreativeSession, track_id: &str) -> Self {
        let midi = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .and_then(|track| track.instrument.as_ref())
            .map(|instrument| instrument.midi.clone())
            .unwrap_or_default();
        Self {
            timebase: session.arrangement.timebase.clone(),
            clips: session
                .arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
                .map(|clip| ScoreClip {
                    id: clip.id.clone(),
                    name: clip.name.clone(),
                    start_tick: clip.start_tick.0,
                    duration_ticks: clip.duration_ticks,
                    notes: clip
                        .notes
                        .iter()
                        .map(|note| {
                            let output = midi.output_note(note.note);
                            ScoreNote {
                                id: note.id.clone(),
                                pitch: output.unwrap_or(note.note),
                                start_tick: note.start_tick.0,
                                duration_ticks: note.duration_ticks,
                                lyric: note.lyric.clone(),
                                out_of_range: output.is_none(),
                            }
                        })
                        .collect(),
                    muted: clip.muted,
                    loop_enabled: clip.loop_enabled,
                })
                .collect(),
            end_tick: content_end_tick(session),
        }
    }

    pub(crate) fn clear_clips(&mut self) {
        self.clips.clear();
    }
}

fn content_end_tick(session: &CreativeSession) -> u64 {
    let arrangement = &session.arrangement;
    let timebase = &arrangement.timebase;
    let mut end = session.settings.mixdown.musical_end_tick;
    for clip in &arrangement.audio_clips {
        end =
            end.max(clip.start_tick.0.saturating_add(
                timebase.duration_to_ticks(clip.start_tick, clip.timeline_duration),
            ));
    }
    for clip in &arrangement.midi_clips {
        end = end.max(clip.start_tick.0.saturating_add(clip.duration_ticks));
    }
    for region in &arrangement.regions {
        end = end.max(region.end_tick.0);
    }
    for event in &arrangement.harmony_events {
        end = end.max(event.end_tick.0);
    }
    for marker in &arrangement.markers {
        end = end.max(marker.tick);
    }
    for lane in &arrangement.automation_lanes {
        for point in &lane.points {
            end = end.max(point.tick.0);
        }
    }
    end
}

fn clock(tick: u64, ppq: u32) -> Result<i32, String> {
    if ppq == 0 {
        return Err("project PPQ is zero".into());
    }
    let scaled = u128::from(tick) * u128::from(SCORE_PPQ);
    let rounded = (scaled + u128::from(ppq / 2)) / u128::from(ppq);
    i32::try_from(rounded).map_err(|_| "VoiSona score position is out of range".into())
}

fn loop_iterations(clip: &ScoreClip, end_tick: u64) -> Result<u64, String> {
    if !clip.loop_enabled {
        return Ok(1);
    }
    if clip.duration_ticks == 0 {
        return Err(format!(
            "VoiSona clip '{}' has a zero-length loop",
            clip.name
        ));
    }
    let span = end_tick.saturating_sub(clip.start_tick);
    let iterations = if span == 0 {
        1
    } else {
        span / clip.duration_ticks + u64::from(!span.is_multiple_of(clip.duration_ticks))
    };
    if iterations > MAX_LOOP_ITERATIONS {
        return Err(format!(
            "VoiSona clip '{}' loop exceeds {MAX_LOOP_ITERATIONS} iterations",
            clip.name
        ));
    }
    Ok(iterations)
}

fn notes(score: &TrackScore) -> Result<BTreeMap<(String, String), PlannedNote>, String> {
    let ppq = score.timebase.ppq;
    let mut result = BTreeMap::new();
    for clip in &score.clips {
        if clip.notes.is_empty() {
            continue;
        }
        let iterations = loop_iterations(clip, score.end_tick)?;
        for iteration in 0..iterations {
            let iteration_start = iteration
                .checked_mul(clip.duration_ticks)
                .and_then(|offset| clip.start_tick.checked_add(offset))
                .ok_or("VoiSona clip start overflow")?;
            for note in &clip.notes {
                if note
                    .lyric
                    .as_deref()
                    .is_some_and(|lyric| lyric.contains('\0'))
                {
                    return Err(format!(
                        "VoiSona note '{}' contains a null character in its lyric",
                        note.id
                    ));
                }
                let start = iteration_start
                    .checked_add(note.start_tick)
                    .ok_or("VoiSona note start overflow")?;
                let start_clock = clock(start, ppq)?;
                let duration = if clip.muted || note.out_of_range {
                    0
                } else {
                    let end = start
                        .checked_add(note.duration_ticks)
                        .ok_or("VoiSona note end overflow")?;
                    let duration = clock(end, ppq)? - start_clock;
                    if duration <= 0 {
                        return Err(format!(
                            "VoiSona note '{}' is shorter than one score tick",
                            note.id
                        ));
                    }
                    duration
                };
                let key = if clip.loop_enabled && iteration > 0 {
                    (format!("{}#{iteration}", clip.id), note.id.clone())
                } else {
                    (clip.id.clone(), note.id.clone())
                };
                let expression_from = (clip.loop_enabled && iteration > 0)
                    .then(|| (clip.id.clone(), note.id.clone()));
                let value = PlannedNote {
                    note: Note {
                        clock: start_clock,
                        duration,
                        pitch: note.pitch,
                        lyric: note.lyric.clone().unwrap_or_default(),
                    },
                    expression_from,
                    start_tick: start,
                    end_tick: start.saturating_add(note.duration_ticks),
                };
                if result.insert(key, value).is_some() {
                    return Err("duplicate VoiSona note ID".into());
                }
            }
        }
    }
    Ok(result)
}

/// Returns the render-range intersections of the notes VoiSona will sing.
///
/// Uses the same note expansion as score synchronization, so clip loops,
/// mutes, instrument transpose, MIDI range, and the project PPQ cannot diverge
/// from the score written to the plugin. Ranges outside the requested render
/// range are omitted.
pub(crate) fn singing_ranges(
    session: &CreativeSession,
    track: &riffra_core::Track,
    start_tick: u64,
    end_tick: u64,
) -> Result<Vec<(u64, u64)>, String> {
    if track.muted {
        return Ok(Vec::new());
    }
    let mut ranges = notes(&TrackScore::from_session(session, &track.id))?
        .into_values()
        .filter(|planned| planned.note.duration > 0 && !planned.note.lyric.trim().is_empty())
        .filter_map(|planned| {
            let start = planned.start_tick.max(start_tick);
            let end = planned.end_tick.min(end_tick);
            (start < end).then_some((start, end))
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges.dedup();
    Ok(ranges)
}

pub(crate) fn has_sounding_notes(
    session: &CreativeSession,
    track_id: &str,
) -> Result<bool, String> {
    Ok(notes(&TrackScore::from_session(session, track_id))?
        .values()
        .any(|planned| planned.note.duration > 0))
}

fn integer(node: &Node, name: &str) -> Result<i32, String> {
    let bytes = &node
        .properties
        .iter()
        .find(|(key, _)| key == name)
        .ok_or_else(|| format!("VoiSona note has no {name}"))?
        .1;
    match bytes.as_slice() {
        [1, a, b, c, d] => Ok(i32::from_le_bytes([*a, *b, *c, *d])),
        _ => Err(format!("VoiSona note has invalid {name}")),
    }
}

fn lyric(node: &Node) -> Result<String, String> {
    let bytes = &node
        .properties
        .iter()
        .find(|(key, _)| key == "Lyric")
        .ok_or("VoiSona note has no lyric")?
        .1;
    let [5, content @ .., 0] = bytes.as_slice() else {
        return Err("VoiSona note has invalid lyric".into());
    };
    std::str::from_utf8(content)
        .map(str::to_owned)
        .map_err(|_| "VoiSona note has invalid lyric".into())
}

fn note_key(clip_id: &str, note_id: &str) -> String {
    format!("{clip_id}\u{1f}{note_id}")
}

fn read_note_id(node: &Node) -> Option<String> {
    let bytes = &node
        .properties
        .iter()
        .find(|(key, _)| key == "RiffraNoteId")?
        .1;
    let [5, content @ .., 0] = bytes.as_slice() else {
        return None;
    };
    std::str::from_utf8(content).ok().map(str::to_owned)
}

fn set_note_id(node: &mut Node, key: &(String, String)) {
    let mut value = vec![5];
    value.extend_from_slice(note_key(&key.0, &key.1).as_bytes());
    value.push(0);
    set(node, "RiffraNoteId", value);
}

fn read_note(node: &Node) -> Result<Note, String> {
    let pitch = integer(node, "PitchOctave")?
        .checked_add(1)
        .and_then(|octave| octave.checked_mul(12))
        .and_then(|v| v.checked_add(integer(node, "PitchStep").ok()?))
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| *v <= 127)
        .ok_or("VoiSona note pitch is invalid")?;
    Ok(Note {
        clock: integer(node, "Clock")?,
        duration: integer(node, "Duration")?,
        pitch,
        lyric: lyric(node)?,
    })
}

fn set(node: &mut Node, name: &str, value: Vec<u8>) {
    if let Some((_, old)) = node.properties.iter_mut().find(|(key, _)| key == name) {
        *old = value;
    } else {
        node.properties.push((name.into(), value));
    }
}
fn set_int(node: &mut Node, name: &str, value: i32) {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&value.to_le_bytes());
    set(node, name, bytes);
}

#[derive(Clone, Debug, PartialEq)]
struct Tempo {
    clock: i32,
    bpm: f64,
}
#[derive(Clone, Debug, PartialEq)]
struct Meter {
    clock: i32,
    numerator: i32,
    denominator: i32,
}

fn tempo_map(timebase: &ProjectTimebase) -> Result<Vec<Tempo>, String> {
    timebase
        .tempo_changes
        .iter()
        .map(|change| {
            Ok(Tempo {
                clock: clock(change.tick, timebase.ppq)?,
                bpm: change.bpm,
            })
        })
        .collect()
}
fn meter_map(timebase: &ProjectTimebase) -> Result<Vec<Meter>, String> {
    timebase
        .time_signature_changes
        .iter()
        .map(|change| {
            Ok(Meter {
                clock: clock(change.tick, timebase.ppq)?,
                numerator: i32::from(change.numerator),
                denominator: i32::from(change.denominator),
            })
        })
        .collect()
}
fn read_tempo(node: &Node) -> Result<Tempo, String> {
    let value = node
        .properties
        .iter()
        .find(|(key, _)| key == "Tempo")
        .ok_or("VoiSona tempo is missing")?
        .1
        .as_slice();
    let [4, bytes @ ..] = value else {
        return Err("VoiSona tempo is invalid".into());
    };
    let bpm = f64::from_le_bytes(bytes.try_into().map_err(|_| "VoiSona tempo is invalid")?);
    if !bpm.is_finite() {
        return Err("VoiSona tempo is invalid".into());
    }
    Ok(Tempo {
        clock: integer(node, "Clock")?,
        bpm,
    })
}
fn read_meter(node: &Node) -> Result<Meter, String> {
    Ok(Meter {
        clock: integer(node, "Clock")?,
        numerator: integer(node, "Beats")?,
        denominator: integer(node, "BeatType")?,
    })
}
fn fresh_map_node(name: &str) -> Node {
    Node {
        name: name.into(),
        properties: vec![],
        children: vec![],
    }
}

fn update_timebase(
    song: &mut Node,
    previous: &TrackScore,
    current: &TrackScore,
) -> Result<bool, String> {
    let (old_tempos, new_tempos) = (
        tempo_map(&previous.timebase)?,
        tempo_map(&current.timebase)?,
    );
    let (old_meters, new_meters) = (
        meter_map(&previous.timebase)?,
        meter_map(&current.timebase)?,
    );
    let tempo = song
        .children
        .iter_mut()
        .find(|node| node.name == "Tempo")
        .ok_or("VoiSona tempo map is missing")?;
    let existing = tempo
        .children
        .iter()
        .filter(|node| node.name == "Sound")
        .map(read_tempo)
        .collect::<Result<Vec<_>, _>>()?;
    if existing != old_tempos {
        return Err("VoiSona tempo map does not match the project".into());
    }
    if old_tempos != new_tempos {
        if tempo
            .children
            .iter()
            .filter(|node| node.name == "Sound")
            .any(|node| {
                !node.children.is_empty()
                    || node
                        .properties
                        .iter()
                        .any(|(key, _)| key != "Clock" && key != "Tempo")
            })
        {
            return Err("VoiSona tempo map contains unsupported additional data".into());
        }
        tempo.children.retain(|node| node.name != "Sound");
        for point in &new_tempos {
            let mut node = fresh_map_node("Sound");
            set_int(&mut node, "Clock", point.clock);
            let mut value = vec![4];
            value.extend_from_slice(&point.bpm.to_le_bytes());
            set(&mut node, "Tempo", value);
            tempo.children.push(node);
        }
    }
    let meter = song
        .children
        .iter_mut()
        .find(|node| node.name == "Beat")
        .ok_or("VoiSona meter map is missing")?;
    let existing = meter
        .children
        .iter()
        .filter(|node| node.name == "Time")
        .map(read_meter)
        .collect::<Result<Vec<_>, _>>()?;
    if existing != old_meters {
        return Err("VoiSona meter map does not match the project".into());
    }
    if old_meters != new_meters {
        if meter
            .children
            .iter()
            .filter(|node| node.name == "Time")
            .any(|node| {
                !node.children.is_empty()
                    || node
                        .properties
                        .iter()
                        .any(|(key, _)| key != "Clock" && key != "Beats" && key != "BeatType")
            })
        {
            return Err("VoiSona meter map contains unsupported additional data".into());
        }
        meter.children.retain(|node| node.name != "Time");
        for point in &new_meters {
            let mut node = fresh_map_node("Time");
            set_int(&mut node, "Clock", point.clock);
            set_int(&mut node, "Beats", point.numerator);
            set_int(&mut node, "BeatType", point.denominator);
            meter.children.push(node);
        }
    }
    Ok(old_tempos != new_tempos || old_meters != new_meters)
}
fn update_note(node: &mut Node, note: &Note) {
    set_int(node, "Clock", note.clock);
    set_int(node, "Duration", note.duration);
    set_int(node, "PitchStep", i32::from(note.pitch % 12));
    set_int(node, "PitchOctave", i32::from(note.pitch / 12) - 1);
    let mut value = vec![5];
    value.extend_from_slice(note.lyric.as_bytes());
    value.push(0);
    set(node, "Lyric", value);
}

fn apply_note(node: &mut Node, note: &Note) -> Result<(), String> {
    if let Ok(old_clock) = integer(node, "Clock") {
        let offset = note
            .clock
            .checked_sub(old_clock)
            .ok_or("VoiSona expression time is out of range")?;
        if offset != 0 {
            for child in &mut node.children {
                shift_expression_times(child, offset)?;
            }
        }
    }
    if !node.properties.iter().any(|(key, _)| key == "Syllabic") {
        set_int(node, "Syllabic", 0);
    }
    update_note(node, note);
    Ok(())
}

fn copy_expression(source: &Node, target: &mut Node, clock_offset: i32) -> Result<(), String> {
    for (name, value) in &source.properties {
        if !matches!(
            name.as_str(),
            "Clock" | "Duration" | "PitchStep" | "PitchOctave" | "Lyric"
        ) {
            set(target, name, value.clone());
        }
    }
    for child in &source.children {
        let mut child = child.clone();
        shift_expression_times(&mut child, clock_offset)?;
        target.children.push(child);
    }
    Ok(())
}

fn shift_expression_times(node: &mut Node, clock_offset: i32) -> Result<(), String> {
    for (name, value) in &mut node.properties {
        if !matches!(name.as_str(), "Clock" | "StartTime") {
            continue;
        }
        if let [1, a, b, c, d] = value.as_slice() {
            let current = i32::from_le_bytes([*a, *b, *c, *d]);
            let shifted = current
                .checked_add(clock_offset)
                .ok_or("VoiSona expression time is out of range")?;
            *value = vec![1];
            value.extend_from_slice(&shifted.to_le_bytes());
        }
    }
    for child in &mut node.children {
        shift_expression_times(child, clock_offset)?;
    }
    Ok(())
}

pub(crate) fn update_score(
    encoded: &str,
    previous: &TrackScore,
    current: &TrackScore,
) -> Result<Option<String>, String> {
    let mut old = notes(previous)?;
    let new = notes(current)?;
    let mut state = PluginState::parse(encoded)?;
    let song = state
        .root
        .children
        .iter_mut()
        .find(|node| node.name == "Song")
        .ok_or("VoiSona song state is missing")?;
    let timebase_changed = update_timebase(song, previous, current)?;
    let score = song
        .children
        .iter_mut()
        .find(|node| node.name == "Score")
        .ok_or("VoiSona score state is missing")?;
    let available = score
        .children
        .iter()
        .filter(|node| node.name == "Note")
        .map(|node| Ok((read_note(node)?, node.clone(), read_note_id(node))))
        .collect::<Result<Vec<_>, String>>()?;
    if available.is_empty() && old.values().all(|planned| planned.note.duration == 0) {
        old.clear();
    }
    let notes_equal = old.len() == new.len()
        && old
            .iter()
            .zip(new.iter())
            .all(|((old_key, old), (new_key, new))| old_key == new_key && old.note == new.note);
    let mut available_ids = BTreeMap::new();
    for (index, (_, _, id)) in available.iter().enumerate() {
        if let Some(id) = id
            && available_ids.insert(id.as_str(), index).is_some()
        {
            return Err("VoiSona score contains duplicate note IDs".into());
        }
    }
    let ids_current = old.len() == available.len()
        && old.iter().all(|(key, planned)| {
            let expected = note_key(&key.0, &key.1);
            available_ids
                .get(expected.as_str())
                .is_some_and(|index| available[*index].0 == planned.note)
        });
    let notes_unchanged = notes_equal && ids_current;
    let mut used = vec![false; available.len()];
    let mut mapped = BTreeMap::new();
    let mut remaining = BTreeMap::new();
    for (key, planned) in old {
        let expected = note_key(&key.0, &key.1);
        let Some(index) = available_ids.get(expected.as_str()).copied() else {
            remaining.insert(key, planned);
            continue;
        };
        if used[index] {
            return Err("VoiSona score contains duplicate note IDs".into());
        }
        if available[index].0 != planned.note {
            return Err("VoiSona score notes do not match the project".into());
        }
        used[index] = true;
        if new.contains_key(&key) {
            mapped.insert(key, available[index].1.clone());
        }
    }
    let mut remaining_available = BTreeMap::<Note, Vec<usize>>::new();
    for (index, (note, _, _)) in available.iter().enumerate() {
        if !used[index] {
            remaining_available
                .entry(note.clone())
                .or_default()
                .push(index);
        }
    }
    let mut sources = BTreeMap::<Note, Vec<(String, String)>>::new();
    for (id, planned) in remaining {
        sources.entry(planned.note).or_default().push(id);
    }
    for (note, ids) in sources {
        let matches = remaining_available.remove(&note).unwrap_or_default();
        if matches.len() != ids.len() {
            return Err("VoiSona score notes do not match the project".into());
        }
        let surviving = ids.iter().filter(|id| new.contains_key(*id)).count();
        if surviving == 0 {
            continue;
        }
        if surviving < ids.len() {
            let (clip, note_id) = &ids[0];
            return Err(format!(
                "VoiSona note '{note_id}' in clip '{clip}' cannot be matched uniquely: \
                 {} saved notes share its pitch, position, duration, and lyric but only \
                 {surviving} remain",
                ids.len()
            ));
        }
        if ids.len() > 1 {
            let first = &new.get(&ids[0]).expect("surviving note exists").note;
            if ids.iter().any(|id| {
                new.get(id)
                    .is_some_and(|candidate| &candidate.note != first)
            }) {
                let (clip, note_id) = &ids[0];
                return Err(format!(
                    "VoiSona notes in clip '{clip}' cannot be matched uniquely: the saved \
                     notes for '{note_id}' and its duplicates were edited differently"
                ));
            }
        }
        for (id, index) in ids.into_iter().rev().zip(matches.into_iter().rev()) {
            mapped.insert(id, available[index].1.clone());
        }
    }
    if !remaining_available.is_empty() {
        return Err("VoiSona score contains notes that do not match the project".into());
    }
    if notes_unchanged && !timebase_changed {
        return Ok(None);
    }
    let mut output = BTreeMap::new();
    for (id, planned) in &new {
        if let Some(mut node) = mapped.remove(id) {
            apply_note(&mut node, &planned.note)?;
            set_note_id(&mut node, id);
            output.insert(id.clone(), node);
        }
    }
    for (id, planned) in &new {
        if output.contains_key(id) {
            continue;
        }
        let mut node = Node {
            name: "Note".into(),
            properties: vec![],
            children: vec![],
        };
        if let Some(source_id) = &planned.expression_from
            && let Some(source) = new.get(source_id)
            && let Some(source_node) = output.get(source_id)
        {
            let offset = planned
                .note
                .clock
                .checked_sub(source.note.clock)
                .ok_or("VoiSona expression time is out of range")?;
            copy_expression(source_node, &mut node, offset)?;
        }
        apply_note(&mut node, &planned.note)?;
        set_note_id(&mut node, id);
        output.insert(id.clone(), node);
    }
    let mut output = output.into_values().collect::<Vec<_>>();
    output.sort_by_key(|node| integer(node, "Clock").unwrap_or_default());
    score.children.retain(|node| node.name != "Note");
    score.children.extend(output);
    state.encode().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{MidiClip, MidiNote, TimelineTick, Track, TrackInstrument};

    fn note(id: &str, pitch: u8, start: u64, duration: u64, lyric: &str) -> MidiNote {
        MidiNote {
            id: id.into(),
            note: pitch,
            start_tick: TimelineTick(start),
            duration_ticks: duration,
            velocity: 100,
            channel: 1,
            lyric: Some(lyric.into()),
        }
    }

    fn clip(id: &str, start: u64, notes: Vec<MidiNote>) -> MidiClip {
        MidiClip {
            id: id.into(),
            name: id.into(),
            track_id: "vocal".into(),
            asset_id: None,
            start_tick: TimelineTick(start),
            duration_ticks: 3840,
            notes,
            events: vec![],
            instrument_control_events: vec![],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        }
    }

    fn session_with(clips: Vec<MidiClip>) -> CreativeSession {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::instrument("vocal".into(), "Vocal".into()));
        session.arrangement.midi_clips = clips;
        session
    }

    fn session(lyric: &str, pitch: u8, duration: u64) -> CreativeSession {
        session_with(vec![clip(
            "clip",
            0,
            vec![note("note", pitch, 0, duration, lyric)],
        )])
    }

    fn score(session: &CreativeSession) -> TrackScore {
        TrackScore::from_session(session, "vocal")
    }

    fn score_note(clock: i32, duration: i32, pitch: u8, lyric: &str) -> Node {
        let mut node = fresh_map_node("Note");
        set_int(&mut node, "Syllabic", 0);
        update_note(
            &mut node,
            &Note {
                clock,
                duration,
                pitch,
                lyric: lyric.into(),
            },
        );
        node
    }

    fn score_note_with_id(
        clock: i32,
        duration: i32,
        pitch: u8,
        lyric: &str,
        clip_id: &str,
        note_id: &str,
    ) -> Node {
        let mut node = score_note(clock, duration, pitch, lyric);
        set_note_id(&mut node, &(clip_id.into(), note_id.into()));
        node
    }

    fn state(notes: Vec<Node>) -> Node {
        let mut tempo = fresh_map_node("Sound");
        set_int(&mut tempo, "Clock", 0);
        let mut bpm = vec![4];
        bpm.extend_from_slice(&120.0_f64.to_le_bytes());
        set(&mut tempo, "Tempo", bpm);
        let mut meter = fresh_map_node("Time");
        set_int(&mut meter, "Clock", 0);
        set_int(&mut meter, "Beats", 4);
        set_int(&mut meter, "BeatType", 4);
        Node {
            name: "StateInformation".into(),
            properties: vec![],
            children: vec![Node {
                name: "Song".into(),
                properties: vec![],
                children: vec![
                    Node {
                        name: "Tempo".into(),
                        properties: vec![],
                        children: vec![tempo],
                    },
                    Node {
                        name: "Beat".into(),
                        properties: vec![],
                        children: vec![meter],
                    },
                    Node {
                        name: "Score".into(),
                        properties: vec![],
                        children: notes,
                    },
                ],
            }],
        }
    }

    #[test]
    fn updates_notes_and_timing_without_discarding_expression() {
        let mut note = fresh_map_node("Note");
        update_note(
            &mut note,
            &Note {
                clock: 0,
                duration: 960,
                pitch: 60,
                lyric: "あ".into(),
            },
        );
        set(&mut note, "Accent", vec![2]);
        set_note_id(&mut note, &("clip".into(), "note".into()));
        let encoded = PluginState::for_test(state(vec![note])).encode().unwrap();
        let before = session("あ", 60, 960);
        assert!(
            update_score(&encoded, &score(&before), &score(&before))
                .unwrap()
                .is_none()
        );
        let mut after = session("＄＠", 62, 240);
        after.arrangement.timebase.ppq = 480;
        after.arrangement.timebase.tempo_changes[0].bpm = 100.0;
        after.arrangement.timebase.time_signature_changes[0].numerator = 3;
        after.arrangement.timebase.time_signature_changes[0].denominator = 8;
        let output = update_score(&encoded, &score(&before), &score(&after))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let song = &parsed.root.children[0];
        assert_eq!(
            read_tempo(&song.children[0].children[0]).unwrap().bpm,
            100.0
        );
        assert_eq!(
            read_meter(&song.children[1].children[0]).unwrap(),
            Meter {
                clock: 0,
                numerator: 3,
                denominator: 8
            }
        );
        let note = &song.children[2].children[0];
        assert_eq!(
            read_note(note).unwrap(),
            Note {
                clock: 0,
                duration: 480,
                pitch: 62,
                lyric: "＄＠".into()
            }
        );
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );
        let mut wrong_tempo = before.clone();
        wrong_tempo.arrangement.timebase.tempo_changes[0].bpm = 90.0;
        assert!(
            update_score(&encoded, &score(&wrong_tempo), &score(&after))
                .unwrap_err()
                .contains("tempo")
        );
        assert!(update_score(&encoded, &score(&session("違う", 60, 960)), &score(&after)).is_err());
    }

    #[test]
    fn synchronizes_multiple_clips_and_drops_removed_notes() {
        let mut previous = session_with(vec![
            clip("first", 0, vec![note("a", 60, 0, 480, "＄")]),
            clip("second", 960, vec![note("b", 62, 0, 480, "＠")]),
        ]);
        previous.arrangement.timebase.ppq = 480;
        let encoded = PluginState::for_test(state(vec![
            score_note(0, 960, 60, "＄"),
            score_note(1920, 960, 62, "＠"),
        ]))
        .encode()
        .unwrap();
        let mut current = session_with(vec![clip(
            "first",
            0,
            vec![note("a", 60, 240, 480, "＄"), note("c", 64, 960, 480, "＠")],
        )]);
        current.arrangement.timebase.ppq = 480;

        let output = update_score(&encoded, &score(&previous), &score(&current))
            .unwrap()
            .unwrap();

        let parsed = PluginState::parse(&output).unwrap();
        let score_node = &parsed.root.children[0].children[2];
        let notes = score_node
            .children
            .iter()
            .filter(|node| node.name == "Note")
            .map(|node| read_note(node).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            notes,
            vec![
                Note {
                    clock: 480,
                    duration: 960,
                    pitch: 60,
                    lyric: "＄".into()
                },
                Note {
                    clock: 1920,
                    duration: 960,
                    pitch: 64,
                    lyric: "＠".into()
                },
            ]
        );
        assert!(
            update_score(&output, &score(&current), &score(&current))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn applies_instrument_midi_transpose_to_the_voisona_score() {
        let mut transposed = session("あ", 60, 480);
        transposed.arrangement.tracks[0].instrument = Some(
            TrackInstrument::vst3("voice".into(), "VoiSona".into(), "C:/VoiSona.vst3".into())
                .unwrap(),
        );
        transposed.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .midi
            .transpose_semitones = 12;
        let encoded = PluginState::for_test(state(vec![score_note_with_id(
            0, 480, 72, "あ", "clip", "note",
        )]))
        .encode()
        .unwrap();
        assert!(
            update_score(&encoded, &score(&transposed), &score(&transposed))
                .unwrap()
                .is_none()
        );

        let mut original = transposed.clone();
        original.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .midi
            .transpose_semitones = 0;
        let output = update_score(&encoded, &score(&transposed), &score(&original))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        assert_eq!(
            read_note(&parsed.root.children[0].children[2].children[0]).unwrap(),
            Note {
                clock: 0,
                duration: 480,
                pitch: 60,
                lyric: "あ".into(),
            }
        );
    }

    #[test]
    fn transposing_a_note_out_of_range_preserves_its_expression() {
        let mut transposed = session("あ", 120, 480);
        transposed.arrangement.tracks[0].instrument = Some(
            TrackInstrument::vst3("voice".into(), "VoiSona".into(), "C:/VoiSona.vst3".into())
                .unwrap(),
        );
        transposed.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .midi
            .transpose_semitones = 12;
        let mut stored = score_note_with_id(0, 480, 120, "あ", "clip", "note");
        set(&mut stored, "Accent", vec![2]);
        let encoded = PluginState::for_test(state(vec![stored])).encode().unwrap();
        let mut original = transposed.clone();
        original.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .midi
            .transpose_semitones = 0;

        assert!(
            singing_ranges(&transposed, &transposed.arrangement.tracks[0], 0, 480)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            singing_ranges(&original, &original.arrangement.tracks[0], 0, 480).unwrap(),
            [(0, 480)]
        );

        let output = update_score(&encoded, &score(&original), &score(&transposed))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(read_note(note).unwrap().duration, 0);
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );

        let output = update_score(&output, &score(&transposed), &score(&original))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(
            read_note(note).unwrap(),
            Note {
                clock: 0,
                duration: 480,
                pitch: 120,
                lyric: "あ".into(),
            }
        );
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );
    }

    #[test]
    fn muted_notes_can_be_added_to_an_empty_score() {
        let mut session = session("あ", 60, 480);
        session.arrangement.midi_clips[0].muted = true;
        let encoded = PluginState::for_test(state(vec![])).encode().unwrap();

        let output = update_score(&encoded, &score(&session), &score(&session))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(read_note(note).unwrap().duration, 0);
    }

    #[test]
    fn moving_a_note_moves_its_timed_expression() {
        let mut node = fresh_map_node("Note");
        update_note(
            &mut node,
            &Note {
                clock: 0,
                duration: 480,
                pitch: 60,
                lyric: "ら".into(),
            },
        );
        let mut expression = fresh_map_node("Expression");
        set(&mut node, "Accent", vec![2]);
        set_int(&mut expression, "Clock", 0);
        set_int(&mut expression, "Value", 7);
        node.children.push(expression);
        let encoded = PluginState::for_test(state(vec![node])).encode().unwrap();
        let before = session("ら", 60, 480);
        let mut moved = before.clone();
        moved.arrangement.midi_clips[0].notes[0].start_tick = TimelineTick(960);
        moved.arrangement.midi_clips[0].notes[0].note = 62;
        moved.arrangement.midi_clips[0].notes[0].lyric = Some("い".into());

        let output = update_score(&encoded, &score(&before), &score(&moved))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(integer(note, "Clock").unwrap(), 960);
        assert_eq!(integer(&note.children[0], "Clock").unwrap(), 960);
        assert_eq!(read_note(note).unwrap().pitch, 62);
        assert_eq!(read_note(note).unwrap().lyric, "い");
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );
    }

    #[test]
    fn muting_a_clip_silences_its_note_and_preserves_its_expression() {
        let mut node = fresh_map_node("Note");
        update_note(
            &mut node,
            &Note {
                clock: 0,
                duration: 960,
                pitch: 60,
                lyric: "あ".into(),
            },
        );
        set(&mut node, "Accent", vec![2]);
        let encoded = PluginState::for_test(state(vec![node])).encode().unwrap();
        let before = session("あ", 60, 960);
        let mut muted = before.clone();
        muted.arrangement.midi_clips[0].muted = true;

        let output = update_score(&encoded, &score(&before), &score(&muted))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(
            read_note(note).unwrap(),
            Note {
                clock: 0,
                duration: 0,
                pitch: 60,
                lyric: "あ".into(),
            }
        );
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );

        let output = update_score(&output, &score(&muted), &score(&before))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let note = &parsed.root.children[0].children[2].children[0];
        assert_eq!(
            read_note(note).unwrap(),
            Note {
                clock: 0,
                duration: 960,
                pitch: 60,
                lyric: "あ".into(),
            }
        );
        assert_eq!(
            note.properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );
    }

    #[test]
    fn muting_one_of_two_identical_notes_keeps_both_notes() {
        let mut session = session_with(vec![
            clip("first", 0, vec![note("a", 60, 0, 480, "あ")]),
            clip("second", 0, vec![note("b", 60, 0, 480, "あ")]),
        ]);
        let encoded = PluginState::for_test(state(vec![
            score_note_with_id(0, 480, 60, "あ", "first", "a"),
            score_note_with_id(0, 480, 60, "あ", "second", "b"),
        ]))
        .encode()
        .unwrap();
        session.arrangement.midi_clips[0].muted = true;

        let output = update_score(
            &encoded,
            &score(&session_with(vec![
                clip("first", 0, vec![note("a", 60, 0, 480, "あ")]),
                clip("second", 0, vec![note("b", 60, 0, 480, "あ")]),
            ])),
            &score(&session),
        )
        .unwrap()
        .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let mut durations = parsed.root.children[0].children[2]
            .children
            .iter()
            .filter(|node| node.name == "Note")
            .map(|node| integer(node, "Duration").unwrap())
            .collect::<Vec<_>>();
        durations.sort_unstable();
        assert_eq!(durations, [0, 480]);
    }

    #[test]
    fn identical_notes_in_separate_clips_are_matched_by_note_id() {
        let session = session_with(vec![
            clip("first", 0, vec![note("a", 60, 0, 480, "あ")]),
            clip("second", 0, vec![note("b", 60, 0, 480, "あ")]),
        ]);
        let encoded = PluginState::for_test(state(vec![
            score_note_with_id(0, 480, 60, "あ", "first", "a"),
            score_note_with_id(0, 480, 60, "あ", "second", "b"),
        ]))
        .encode()
        .unwrap();

        assert!(
            update_score(&encoded, &score(&session), &score(&session))
                .unwrap()
                .is_none()
        );

        let mut removed = session.clone();
        removed.arrangement.midi_clips.pop();
        let output = update_score(&encoded, &score(&session), &score(&removed))
            .unwrap()
            .unwrap();
        let parsed = PluginState::parse(&output).unwrap();
        let notes = parsed.root.children[0].children[2]
            .children
            .iter()
            .filter(|node| node.name == "Note")
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1);
        assert_eq!(read_note_id(notes[0]).as_deref(), Some("first\u{1f}a"));
    }

    #[test]
    fn legacy_duplicate_notes_without_ids_cannot_be_matched_after_removal() {
        let session = session_with(vec![
            clip("first", 0, vec![note("a", 60, 0, 480, "あ")]),
            clip("second", 0, vec![note("b", 60, 0, 480, "あ")]),
        ]);
        let encoded = PluginState::for_test(state(vec![
            score_note(0, 480, 60, "あ"),
            score_note(0, 480, 60, "あ"),
        ]))
        .encode()
        .unwrap();

        let mut removed = session.clone();
        removed.arrangement.midi_clips.pop();
        let error = update_score(&encoded, &score(&session), &score(&removed)).unwrap_err();
        assert!(error.contains("cannot be matched uniquely"));
    }

    #[test]
    fn looped_clips_expand_to_their_timeline_positions() {
        let mut looping = clip("loop", 0, vec![note("a", 60, 0, 480, "ら")]);
        looping.duration_ticks = 1_920;
        looping.loop_enabled = true;
        let mut current = session_with(vec![looping]);
        current.settings.mixdown.musical_end_tick = 5_760;
        assert_eq!(
            singing_ranges(&current, &current.arrangement.tracks[0], 1_000, 4_000).unwrap(),
            [(1_920, 2_400), (3_840, 4_000)]
        );
        let encoded = PluginState::for_test(state(vec![])).encode().unwrap();

        let output = update_score(&encoded, &score(&session_with(vec![])), &score(&current))
            .unwrap()
            .unwrap();

        let parsed = PluginState::parse(&output).unwrap();
        let notes = parsed.root.children[0].children[2]
            .children
            .iter()
            .filter(|node| node.name == "Note")
            .map(|node| read_note(node).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            notes,
            vec![
                Note {
                    clock: 0,
                    duration: 480,
                    pitch: 60,
                    lyric: "ら".into(),
                },
                Note {
                    clock: 1_920,
                    duration: 480,
                    pitch: 60,
                    lyric: "ら".into(),
                },
                Note {
                    clock: 3_840,
                    duration: 480,
                    pitch: 60,
                    lyric: "ら".into(),
                },
            ]
        );
        assert!(
            update_score(&output, &score(&current), &score(&current))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn enabling_a_loop_copies_note_expression_to_every_iteration() {
        let mut node = fresh_map_node("Note");
        update_note(
            &mut node,
            &Note {
                clock: 0,
                duration: 480,
                pitch: 60,
                lyric: "ら".into(),
            },
        );
        set(&mut node, "Accent", vec![2]);
        let mut expression = fresh_map_node("Expression");
        set_int(&mut expression, "Clock", 0);
        set_int(&mut expression, "Value", 7);
        node.children.push(expression);
        let encoded = PluginState::for_test(state(vec![node])).encode().unwrap();
        let before = session("ら", 60, 480);
        let mut current = before.clone();
        current.arrangement.midi_clips[0].duration_ticks = 1_920;
        current.arrangement.midi_clips[0].loop_enabled = true;
        current.settings.mixdown.musical_end_tick = 5_760;

        let output = update_score(&encoded, &score(&before), &score(&current))
            .unwrap()
            .unwrap();

        let parsed = PluginState::parse(&output).unwrap();
        let notes = parsed.root.children[0].children[2]
            .children
            .iter()
            .filter(|node| node.name == "Note")
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 3);
        for note in &notes {
            assert_eq!(
                note.properties
                    .iter()
                    .find(|(key, _)| key == "Accent")
                    .unwrap()
                    .1,
                [2]
            );
        }
        let clocks = notes
            .iter()
            .map(|note| integer(&note.children[0], "Clock").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(clocks, [0, 1_920, 3_840]);
    }

    #[test]
    fn empty_looped_clips_are_ignored() {
        let mut empty = clip("empty", 0, Vec::new());
        empty.duration_ticks = 1_920;
        empty.loop_enabled = true;
        let mut current = session_with(vec![empty]);
        current.settings.mixdown.musical_end_tick = 1_000_000_000;
        let encoded = PluginState::for_test(state(vec![])).encode().unwrap();

        assert!(
            update_score(&encoded, &score(&session_with(vec![])), &score(&current))
                .unwrap()
                .is_none()
        );
    }
}
