use super::state::{Node, PluginState};
use riffra_core::CreativeSession;
use std::collections::BTreeMap;

// VoiSona's score clock uses 960 units per quarter note.
const SCORE_PPQ: u64 = 960;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Note {
    clock: i32,
    duration: i32,
    pitch: u8,
    lyric: String,
}

fn clock(tick: u64, ppq: u32) -> Result<i32, String> {
    if ppq == 0 {
        return Err("project PPQ is zero".into());
    }
    let scaled = u128::from(tick) * u128::from(SCORE_PPQ);
    let rounded = (scaled + u128::from(ppq / 2)) / u128::from(ppq);
    i32::try_from(rounded).map_err(|_| "VoiSona score position is out of range".into())
}

fn notes(
    session: &CreativeSession,
    track_id: &str,
) -> Result<BTreeMap<(String, String), Note>, String> {
    let mut result = BTreeMap::new();
    for clip in session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| clip.track_id == track_id && !clip.muted)
    {
        if clip.loop_enabled {
            return Err(format!(
                "VoiSona clip '{}' uses an unsupported loop",
                clip.name
            ));
        }
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
            let start = clip
                .start_tick
                .0
                .checked_add(note.start_tick.0)
                .ok_or("VoiSona note start overflow")?;
            let end = start
                .checked_add(note.duration_ticks)
                .ok_or("VoiSona note end overflow")?;
            let start_clock = clock(start, session.arrangement.timebase.ppq)?;
            let duration = clock(end, session.arrangement.timebase.ppq)? - start_clock;
            if duration <= 0 {
                return Err(format!(
                    "VoiSona note '{}' is shorter than one score tick",
                    note.id
                ));
            }
            let value = Note {
                clock: start_clock,
                duration,
                pitch: note.note,
                lyric: note.lyric.clone().unwrap_or_default(),
            };
            if result
                .insert((clip.id.clone(), note.id.clone()), value)
                .is_some()
            {
                return Err("duplicate VoiSona note ID".into());
            }
        }
    }
    Ok(result)
}

pub(crate) fn has_notes(session: &CreativeSession, track_id: &str) -> Result<bool, String> {
    Ok(!notes(session, track_id)?.is_empty())
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

fn tempo_map(session: &CreativeSession) -> Result<Vec<Tempo>, String> {
    session
        .arrangement
        .timebase
        .tempo_changes
        .iter()
        .map(|change| {
            Ok(Tempo {
                clock: clock(change.tick, session.arrangement.timebase.ppq)?,
                bpm: change.bpm,
            })
        })
        .collect()
}
fn meter_map(session: &CreativeSession) -> Result<Vec<Meter>, String> {
    session
        .arrangement
        .timebase
        .time_signature_changes
        .iter()
        .map(|change| {
            Ok(Meter {
                clock: clock(change.tick, session.arrangement.timebase.ppq)?,
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
    previous: &CreativeSession,
    current: &CreativeSession,
) -> Result<bool, String> {
    let (old_tempos, new_tempos) = (tempo_map(previous)?, tempo_map(current)?);
    let (old_meters, new_meters) = (meter_map(previous)?, meter_map(current)?);
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
    previous: &CreativeSession,
    current: &CreativeSession,
    track_id: &str,
) -> Result<Option<String>, String> {
    let old = notes(previous, track_id)?;
    let new = notes(current, track_id)?;
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
    let mut mapped = BTreeMap::new();
    for (id, note) in old {
        let matches = available
            .iter()
            .enumerate()
            .filter(|(_, (candidate, _))| candidate == &note)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "VoiSona note '{}' in clip '{}' cannot be matched uniquely to its saved state",
                id.1, id.0
            ));
        }
        mapped.insert(id, available.remove(matches[0]).1);
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
            update_score(&encoded, &before, &before, "vocal")
                .unwrap()
                .is_none()
        );
        let mut after = session("＄＠", 62, 240);
        after.arrangement.timebase.ppq = 480;
        after.arrangement.timebase.tempo_changes[0].bpm = 100.0;
        after.arrangement.timebase.time_signature_changes[0].numerator = 3;
        after.arrangement.timebase.time_signature_changes[0].denominator = 8;
        let output = update_score(&encoded, &before, &after, "vocal")
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
            update_score(&encoded, &wrong_tempo, &after, "vocal")
                .unwrap_err()
                .contains("tempo")
        );
        assert!(update_score(&encoded, &session("違う", 60, 960), &after, "vocal").is_err());
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

        let output = update_score(&encoded, &previous, &current, "vocal")
            .unwrap()
            .unwrap();

        let parsed = PluginState::parse(&output).unwrap();
        let score = &parsed.root.children[0].children[2];
        let notes = score
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
            update_score(&output, &current, &current, "vocal")
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
            &session("あ", 60, 960),
            &session("い", 62, 480),
            "vocal",
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
