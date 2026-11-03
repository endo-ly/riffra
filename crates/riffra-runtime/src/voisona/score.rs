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
}

#[derive(Clone, Debug)]
struct ScoreClip {
    id: String,
    name: String,
    start_tick: u64,
    duration_ticks: u64,
    notes: Vec<ScoreNote>,
    loop_enabled: bool,
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
                        .map(|note| ScoreNote {
                            id: note.id.clone(),
                            pitch: note.note,
                            start_tick: note.start_tick.0,
                            duration_ticks: note.duration_ticks,
                            lyric: note.lyric.clone(),
                        })
                        .collect(),
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

fn notes(score: &TrackScore) -> Result<BTreeMap<(String, String), Note>, String> {
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
                let end = start
                    .checked_add(note.duration_ticks)
                    .ok_or("VoiSona note end overflow")?;
                let start_clock = clock(start, ppq)?;
                let duration = clock(end, ppq)? - start_clock;
                if duration <= 0 {
                    return Err(format!(
                        "VoiSona note '{}' is shorter than one score tick",
                        note.id
                    ));
                }
                let value = Note {
                    clock: start_clock,
                    duration,
                    pitch: note.pitch,
                    lyric: note.lyric.clone().unwrap_or_default(),
                };
                let key = if clip.loop_enabled && iteration > 0 {
                    (format!("{}#{iteration}", clip.id), note.id.clone())
                } else {
                    (clip.id.clone(), note.id.clone())
                };
                if result.insert(key, value).is_some() {
                    return Err("duplicate VoiSona note ID".into());
                }
            }
        }
    }
    Ok(result)
}

pub(crate) fn has_notes(session: &CreativeSession, track_id: &str) -> Result<bool, String> {
    Ok(!notes(&TrackScore::from_session(session, track_id))?.is_empty())
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

pub(crate) fn update_score(
    encoded: &str,
    previous: &TrackScore,
    current: &TrackScore,
) -> Result<Option<String>, String> {
    let old = notes(previous)?;
    let new = notes(current)?;
    let notes_unchanged = old == new;
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
    let mut available = score
        .children
        .iter()
        .filter(|node| node.name == "Note")
        .map(|node| Ok((read_note(node)?, node.clone())))
        .collect::<Result<Vec<_>, String>>()?;
    let mut sources = BTreeMap::<Note, Vec<(String, String)>>::new();
    for (id, note) in old {
        sources.entry(note).or_default().push(id);
    }
    let mut mapped = BTreeMap::new();
    for (note, ids) in sources {
        let matches = available
            .iter()
            .enumerate()
            .filter(|(_, (candidate, _))| candidate == &note)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() != ids.len() {
            return Err("VoiSona score notes do not match the project".into());
        }
        let surviving = ids.iter().filter(|id| new.contains_key(*id)).count();
        if surviving == 0 {
            for index in matches.into_iter().rev() {
                available.remove(index);
            }
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
            let first = new.get(&ids[0]).expect("surviving note exists");
            if ids
                .iter()
                .any(|id| new.get(id).is_some_and(|candidate| candidate != first))
            {
                let (clip, note_id) = &ids[0];
                return Err(format!(
                    "VoiSona notes in clip '{clip}' cannot be matched uniquely: the saved \
                     notes for '{note_id}' and its duplicates were edited differently"
                ));
            }
        }
        for (id, index) in ids.into_iter().rev().zip(matches.into_iter().rev()) {
            mapped.insert(id, available.remove(index).1);
        }
    }
    if !available.is_empty() {
        return Err("VoiSona score contains notes that do not match the project".into());
    }
    if notes_unchanged && !timebase_changed {
        return Ok(None);
    }
    let mut output = Vec::new();
    for (id, note) in new {
        let mut node = mapped.remove(&id).unwrap_or_else(|| Node {
            name: "Note".into(),
            properties: vec![],
            children: vec![],
        });
        if !node.properties.iter().any(|(key, _)| key == "Syllabic") {
            set_int(&mut node, "Syllabic", 0);
        }
        update_note(&mut node, &note);
        output.push(node);
    }
    output.sort_by_key(|node| integer(node, "Clock").unwrap_or_default());
    score.children.retain(|node| node.name != "Note");
    score.children.extend(output);
    state.encode().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{MidiClip, MidiNote, TimelineTick, Track};

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
    fn muting_a_clip_keeps_its_saved_notes() {
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

        assert!(
            update_score(&encoded, &score(&before), &score(&muted))
                .unwrap()
                .is_none()
        );
        assert!(
            update_score(&encoded, &score(&muted), &score(&before))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn identical_notes_in_separate_clips_are_matched_as_a_multiset() {
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

        assert!(
            update_score(&encoded, &score(&session), &score(&session))
                .unwrap()
                .is_none()
        );

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
    fn enabling_a_loop_keeps_the_first_note_expression() {
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
        assert_eq!(
            notes[0]
                .properties
                .iter()
                .find(|(key, _)| key == "Accent")
                .unwrap()
                .1,
            [2]
        );
        assert!(
            notes[1..]
                .iter()
                .all(|note| { !note.properties.iter().any(|(key, _)| key == "Accent") })
        );
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

    #[test]
    #[ignore = "requires a locally saved VoiSona state with the test note and expression"]
    fn changes_a_note_without_losing_its_expression() {
        let path = std::env::var_os("RIFFRA_VOISONA_TEST_STATE").expect("VoiSona state path");
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let encoded = json["stateData"].as_str().unwrap();
        let before = PluginState::parse(encoded).unwrap();
        let updated = update_score(
            encoded,
            &score(&session("あ", 60, 960)),
            &score(&session("い", 62, 480)),
        )
        .unwrap()
        .unwrap();
        let after = PluginState::parse(&updated).unwrap();
        let score = |root: &Node| {
            root.children
                .iter()
                .find(|node| node.name == "Song")
                .unwrap()
                .children
                .iter()
                .find(|node| node.name == "Score")
                .unwrap()
                .children
                .iter()
                .find(|node| node.name == "Note")
                .unwrap()
                .clone()
        };
        let old = score(&before.root);
        let new = score(&after.root);
        assert_eq!(
            read_note(&new).unwrap(),
            Note {
                clock: 0,
                duration: 480,
                pitch: 62,
                lyric: "い".into()
            }
        );
        assert_eq!(
            old.properties.iter().find(|(key, _)| key == "Accent"),
            new.properties.iter().find(|(key, _)| key == "Accent")
        );
        assert!(old.properties.iter().any(|(key, _)| key == "Accent"));
        if let Some(output) = std::env::var_os("RIFFRA_VOISONA_TEST_OUTPUT") {
            json["stateData"] = updated.into();
            std::fs::write(output, serde_json::to_vec(&json).unwrap()).unwrap();
        }
    }
}
