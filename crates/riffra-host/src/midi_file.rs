//! Standard MIDI File parsing and encoding.

use riffra_core::{MidiEvent, MidiEventKind, MidiNote, TimelineTick};
use std::collections::{HashMap, VecDeque};

/// Parses an SMF into project-tick duration, notes, and non-note events.
///
/// Uses the source PPQ when no target PPQ is supplied.
///
/// # Errors
/// Returns an error for malformed input or tick conversion overflow.
pub fn parse_smf(
    bytes: &[u8],
    target_ppq: Option<u32>,
) -> Result<(u64, Vec<MidiNote>, Vec<MidiEvent>), String> {
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        return Err("MIDI Asset does not contain a standard MIDI header.".into());
    }
    let mut header_cursor = 4;
    let header_len = read_be_u32(bytes, &mut header_cursor)? as usize;
    if header_len < 6 || header_cursor.saturating_add(header_len) > bytes.len() {
        return Err("MIDI header length is invalid.".into());
    }
    let _format = read_be_u16(bytes, &mut header_cursor)?;
    let track_count = read_be_u16(bytes, &mut header_cursor)?;
    let source_ppq = read_be_u16(bytes, &mut header_cursor)?;
    if source_ppq == 0 || source_ppq & 0x8000 != 0 || track_count == 0 {
        return Err("MIDI Asset must use a positive ticks-per-quarter division.".into());
    }

    let target_ppq = target_ppq.unwrap_or(u32::from(source_ppq));
    if target_ppq == 0 {
        return Err("MIDI target PPQ must be positive.".into());
    }
    let convert_tick = |tick: u64| -> Result<u64, String> {
        let scaled = (u128::from(tick) * u128::from(target_ppq) + u128::from(source_ppq) / 2)
            / u128::from(source_ppq);
        u64::try_from(scaled).map_err(|_| "MIDI tick conversion overflowed.".into())
    };
    let mut cursor = 8 + header_len;
    let mut notes: Vec<MidiNote> = Vec::new();
    let mut events = Vec::new();
    // Keep the source tick and event order until lyrics have been matched.
    let mut note_positions = Vec::new();
    let mut lyrics = Vec::new();
    let mut last_tick = 0_u64;
    for track_index in 0..track_count {
        let mut note_starts: HashMap<(u8, u8), VecDeque<usize>> = HashMap::new();
        if bytes.get(cursor..cursor.saturating_add(4)) != Some(b"MTrk".as_slice()) {
            return Err("MIDI track header is missing.".into());
        }
        cursor += 4;
        let track_len = read_be_u32(bytes, &mut cursor)? as usize;
        let track_end = cursor
            .checked_add(track_len)
            .ok_or_else(|| "MIDI track length overflowed.".to_string())?;
        if track_end > bytes.len() {
            return Err("MIDI track is truncated.".into());
        }

        let mut tick = 0_u64;
        let mut running_status = 0_u8;
        let mut event_order = 0_usize;
        let track_bytes = &bytes[..track_end];
        while cursor < track_end {
            event_order += 1;
            tick = tick
                .checked_add(read_vlq(track_bytes, &mut cursor)?)
                .ok_or("MIDI tick overflowed.")?;
            last_tick = last_tick.max(convert_tick(tick)?);
            let first = *track_bytes
                .get(cursor)
                .ok_or_else(|| "MIDI event is truncated.".to_string())?;
            let status = if first & 0x80 != 0 {
                cursor += 1;
                if first < 0xf0 {
                    running_status = first;
                }
                first
            } else if running_status >= 0x80 {
                running_status
            } else {
                return Err("MIDI event has no running status.".into());
            };

            if status == 0xff {
                let meta_type = *track_bytes
                    .get(cursor)
                    .ok_or_else(|| "MIDI meta event is truncated.".to_string())?;
                cursor += 1;
                let length = read_vlq(track_bytes, &mut cursor)? as usize;
                let data_start = cursor;
                cursor = cursor
                    .checked_add(length)
                    .ok_or_else(|| "MIDI meta event length overflowed.".to_string())?;
                if cursor > track_end {
                    return Err("MIDI meta event is truncated.".into());
                }
                if meta_type == 0x05
                    && let Ok(lyric) = std::str::from_utf8(&track_bytes[data_start..cursor])
                {
                    lyrics.push((track_index, tick, event_order, lyric.to_owned()));
                }
                continue;
            }
            if status == 0xf0 || status == 0xf7 {
                let length = read_vlq(track_bytes, &mut cursor)? as usize;
                cursor = cursor
                    .checked_add(length)
                    .ok_or_else(|| "MIDI system event length overflowed.".to_string())?;
                if cursor > track_end {
                    return Err("MIDI system event is truncated.".into());
                }
                continue;
            }

            let channel = (status & 0x0f) + 1;
            let kind = status >> 4;
            let data1 = *track_bytes
                .get(cursor)
                .ok_or_else(|| "MIDI channel event is truncated.".to_string())?;
            cursor += 1;
            let data2 = if matches!(kind, 0xc | 0xd) {
                0
            } else {
                let value = *track_bytes
                    .get(cursor)
                    .ok_or_else(|| "MIDI channel event is truncated.".to_string())?;
                cursor += 1;
                value
            };
            let project_tick = convert_tick(tick)?;
            match kind {
                0x8 | 0x9 if kind == 0x8 || data2 == 0 => {
                    if let Some(index) = note_starts
                        .get_mut(&(channel, data1))
                        .and_then(VecDeque::pop_front)
                    {
                        let start = notes[index].start_tick.0;
                        let end = project_tick
                            .max(start.checked_add(1).ok_or("MIDI note end overflowed.")?);
                        notes[index].duration_ticks = end - start;
                        last_tick = last_tick.max(end);
                    }
                }
                0x9 => {
                    let index = notes.len();
                    notes.push(MidiNote {
                        lyric: None,
                        id: format!("note:asset:{track_index}:{event_order}"),
                        note: data1,
                        start_tick: TimelineTick(project_tick),
                        duration_ticks: 1,
                        velocity: data2.max(1),
                        channel,
                    });
                    note_positions.push((track_index, tick, event_order, index));
                    note_starts
                        .entry((channel, data1))
                        .or_default()
                        .push_back(index);
                }
                0xb => events.push(MidiEvent {
                    id: format!("event:cc:{channel}:{project_tick}:{data1}"),
                    kind: MidiEventKind::ControlChange,
                    tick: TimelineTick(project_tick),
                    channel,
                    data1,
                    data2,
                }),
                0xd => events.push(MidiEvent {
                    id: format!("event:pressure:{channel}:{project_tick}"),
                    kind: MidiEventKind::ChannelPressure,
                    tick: TimelineTick(project_tick),
                    channel,
                    data1,
                    data2,
                }),
                0xe => events.push(MidiEvent {
                    id: format!("event:pitch:{channel}:{project_tick}"),
                    kind: MidiEventKind::PitchBend,
                    tick: TimelineTick(project_tick),
                    channel,
                    data1,
                    data2,
                }),
                _ => {}
            }
        }
        let track_end_tick = convert_tick(tick)?;
        for index in note_starts.into_values().flatten() {
            let start = notes[index].start_tick.0;
            let end = track_end_tick.max(start.checked_add(1).ok_or("MIDI note end overflowed.")?);
            notes[index].duration_ticks = end - start;
        }
        cursor = track_end;
    }

    // Count and XOR unassigned indices to find a unique candidate in constant time.
    // Source ticks remain distinct even when PPQ conversion merges positions.
    let mut by_track_tick: HashMap<(u16, u64), (usize, usize)> = HashMap::new();
    let mut by_tick: HashMap<u64, (usize, usize)> = HashMap::new();
    let mut by_order = HashMap::new();
    let mut note_tracks = Vec::with_capacity(notes.len());
    for (track, tick, order, index) in note_positions {
        let local = by_track_tick.entry((track, tick)).or_default();
        local.0 += 1;
        local.1 ^= index;
        let global = by_tick.entry(tick).or_default();
        global.0 += 1;
        global.1 ^= index;
        by_order.insert((track, tick, order), index);
        note_tracks.push(track);
    }
    for (track, tick, order, lyric) in lyrics {
        let key = (track, tick);
        let local = by_track_tick.get(&key).copied().unwrap_or_default();
        let global = by_tick.get(&tick).copied().unwrap_or_default();
        let adjacent = by_order
            .get(&(track, tick, order + 1))
            .copied()
            .filter(|index| notes[*index].lyric.is_none());
        let index = adjacent.or(match (local, global) {
            ((1, index), _) | ((0, _), (1, index)) => Some(index),
            _ => None,
        });
        if let Some(index) = index {
            notes[index].lyric = Some(lyric);
            let local = by_track_tick
                .get_mut(&(note_tracks[index], tick))
                .expect("note group exists");
            local.0 -= 1;
            local.1 ^= index;
            let global = by_tick.get_mut(&tick).expect("note tick exists");
            global.0 -= 1;
            global.1 ^= index;
        }
    }

    let duration = notes
        .iter()
        .map(|note| note.start_tick.0.saturating_add(note.duration_ticks))
        .chain(events.iter().map(|event| event.tick.0.saturating_add(1)))
        .chain(std::iter::once(last_tick))
        .max()
        .unwrap_or(1)
        .max(1);
    notes.sort_by_key(|note| note.start_tick.0);
    events.sort_by_key(|event| event.tick.0);
    Ok((duration, notes, events))
}

fn read_vlq(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut value = 0_u64;
    for _ in 0..4 {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| "MIDI file ended inside a variable-length value.".to_string())?;
        *cursor += 1;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("MIDI variable-length value is too long.".into())
}

fn read_be_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, String> {
    let value = u16::from_be_bytes(
        bytes
            .get(*cursor..cursor.saturating_add(2))
            .ok_or_else(|| "MIDI file ended inside a 16-bit value.".to_string())?
            .try_into()
            .map_err(|_| "MIDI 16-bit value is truncated.".to_string())?,
    );
    *cursor += 2;
    Ok(value)
}

fn read_be_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let value = u32::from_be_bytes(
        bytes
            .get(*cursor..cursor.saturating_add(4))
            .ok_or_else(|| "MIDI file ended inside a 32-bit value.".to_string())?
            .try_into()
            .map_err(|_| "MIDI 32-bit value is truncated.".to_string())?,
    );
    *cursor += 4;
    Ok(value)
}

/// Encodes MIDI Clips and the project musical clock as an SMF format 1 file.
/// Clip offsets and the optional end are expressed in project ticks.
///
/// # Errors
/// Returns an error when the timebase or an event cannot be represented in SMF.
pub fn encode_smf(
    timebase: &riffra_core::ProjectTimebase,
    clips: &[(&riffra_core::MidiClip, u64)],
    origin: u64,
    end: Option<u64>,
) -> Result<Vec<u8>, String> {
    timebase.validate()?;
    if timebase.ppq > 0x7fff {
        return Err("project PPQ exceeds the SMF limit of 32767".into());
    }
    let mut clock = Vec::new();
    let tempo = timebase
        .tempo_changes
        .iter()
        .rev()
        .find(|item| item.tick <= origin)
        .ok_or("no tempo at export origin")?;
    let signature = timebase
        .time_signature_changes
        .iter()
        .rev()
        .find(|item| item.tick <= origin)
        .ok_or("no time signature at export origin")?;
    let tempo_event = |bpm: f64| -> Result<Vec<u8>, String> {
        let micros = (60_000_000.0 / bpm).round();
        if !micros.is_finite() || !(1.0..=16_777_215.0).contains(&micros) {
            return Err("tempo is outside the SMF range".into());
        }
        let bytes = (micros as u32).to_be_bytes();
        Ok(vec![0xff, 0x51, 3, bytes[1], bytes[2], bytes[3]])
    };
    let signature_event = |numerator: u8, denominator: u8| -> Result<Vec<u8>, String> {
        if !denominator.is_power_of_two() {
            return Err("time signature denominator is not representable in SMF".into());
        }
        Ok(vec![
            0xff,
            0x58,
            4,
            numerator,
            denominator.trailing_zeros() as u8,
            24,
            8,
        ])
    };
    clock.push((0, 0, tempo_event(tempo.bpm)?));
    clock.push((
        0,
        0,
        signature_event(signature.numerator, signature.denominator)?,
    ));
    for change in timebase
        .tempo_changes
        .iter()
        .filter(|c| c.tick > origin && end.is_none_or(|end| c.tick < end))
    {
        clock.push((change.tick - origin, 0, tempo_event(change.bpm)?));
    }
    for change in timebase
        .time_signature_changes
        .iter()
        .filter(|c| c.tick > origin && end.is_none_or(|end| c.tick < end))
    {
        clock.push((
            change.tick - origin,
            0,
            signature_event(change.numerator, change.denominator)?,
        ));
    }
    let mut performance = Vec::new();
    let mut note_ranges = Vec::new();
    for (clip, offset) in clips {
        for note in &clip.notes {
            if note.note > 127 || note.velocity == 0 || note.velocity > 127 {
                return Err("note pitch or velocity cannot be represented in SMF".into());
            }
            if note.duration_ticks == 0 {
                return Err(format!("MIDI note '{}' has zero duration", note.id));
            }
            let tick = offset
                .checked_add(note.start_tick.0)
                .ok_or("note tick overflow")?;
            let finish = tick
                .checked_add(note.duration_ticks)
                .ok_or("note end overflow")?;
            let channel = note
                .channel
                .checked_sub(1)
                .filter(|value| *value < 16)
                .ok_or("invalid MIDI channel")?;
            note_ranges.push((channel, note.note, tick, finish, &clip.id, &note.id));
            let mut on = Vec::new();
            if let Some(lyric) = &note.lyric {
                on.extend_from_slice(&[0xff, 0x05]);
                encode_vlq(lyric.len() as u64, &mut on)?;
                on.extend_from_slice(lyric.as_bytes());
                on.push(0);
            }
            on.extend_from_slice(&[0x90 | channel, note.note, note.velocity]);
            performance.push((tick, 1, on));
            performance.push((finish, 0, vec![0x80 | channel, note.note, 0]));
        }
        for event in &clip.events {
            if event.data1 > 127 || event.data2 > 127 {
                return Err("MIDI event data cannot be represented in SMF".into());
            }
            let tick = offset
                .checked_add(event.tick.0)
                .ok_or("event tick overflow")?;
            let channel = event
                .channel
                .checked_sub(1)
                .filter(|value| *value < 16)
                .ok_or("invalid MIDI channel")?;
            let bytes = match event.kind {
                MidiEventKind::ControlChange => vec![0xb0 | channel, event.data1, event.data2],
                MidiEventKind::PitchBend => vec![0xe0 | channel, event.data1, event.data2],
                MidiEventKind::ChannelPressure => vec![0xd0 | channel, event.data1],
            };
            performance.push((tick, 1, bytes));
        }
    }
    note_ranges.sort_unstable_by_key(|(channel, pitch, start, end, _, _)| {
        (*channel, *pitch, *start, *end)
    });
    for pair in note_ranges.windows(2) {
        let (channel, pitch, _, previous_end, previous_clip, previous_note) = pair[0];
        let (next_channel, next_pitch, next_start, _, next_clip, next_note) = pair[1];
        if channel == next_channel && pitch == next_pitch && next_start < previous_end {
            return Err(format!(
                "overlapping MIDI notes '{previous_note}' in clip '{previous_clip}' and '{next_note}' in clip '{next_clip}' (channel {}, pitch {pitch}) cannot be exported",
                channel + 1
            ));
        }
    }
    let mut output = b"MThd\0\0\0\x06\0\x01\0\x02".to_vec();
    output.extend_from_slice(&(timebase.ppq as u16).to_be_bytes());
    encode_track(clock, &mut output)?;
    encode_track(performance, &mut output)?;
    Ok(output)
}

fn encode_track(mut events: Vec<(u64, u8, Vec<u8>)>, output: &mut Vec<u8>) -> Result<(), String> {
    events.sort_by_key(|(tick, order, _)| (*tick, *order));
    let mut track = Vec::new();
    let mut previous = 0;
    for (tick, _, bytes) in events {
        encode_vlq(tick - previous, &mut track)?;
        track.extend_from_slice(&bytes);
        previous = tick;
    }
    track.extend_from_slice(&[0, 0xff, 0x2f, 0]);
    let length = u32::try_from(track.len()).map_err(|_| "SMF track is too long")?;
    output.extend_from_slice(b"MTrk");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(&track);
    Ok(())
}

fn encode_vlq(mut value: u64, output: &mut Vec<u8>) -> Result<(), String> {
    if value > 0x0fff_ffff {
        return Err("SMF variable-length value is too large".into());
    }
    let mut bytes = [0; 4];
    let mut index = 3;
    bytes[index] = (value & 0x7f) as u8;
    value >>= 7;
    while value != 0 {
        index -= 1;
        bytes[index] = (value as u8 & 0x7f) | 0x80;
        value >>= 7;
    }
    output.extend_from_slice(&bytes[index..]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_smf;

    fn write_vlq(value: u32, output: &mut Vec<u8>) {
        let mut bytes = [0_u8; 5];
        let mut index = bytes.len() - 1;
        bytes[index] = (value & 0x7f) as u8;
        let mut value = value >> 7;
        while value != 0 {
            index -= 1;
            bytes[index] = ((value & 0x7f) | 0x80) as u8;
            value >>= 7;
        }
        output.extend_from_slice(&bytes[index..]);
    }

    fn minimal_smf(ppq: u16) -> Vec<u8> {
        let mut track = Vec::new();
        write_vlq(0, &mut track);
        track.extend_from_slice(&[0x90, 60, 100]);
        write_vlq(u32::from(ppq), &mut track);
        track.extend_from_slice(&[0x80, 60, 0]);
        write_vlq(0, &mut track);
        track.extend_from_slice(&[0xff, 0x2f, 0x00]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MThd");
        bytes.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1]);
        bytes.extend_from_slice(&ppq.to_be_bytes());
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        bytes
    }

    fn smf_tracks(ppq: u16, tracks: &[&[u8]]) -> Vec<u8> {
        let mut bytes = b"MThd\0\0\0\x06\0\x01".to_vec();
        bytes.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&ppq.to_be_bytes());
        for track in tracks {
            bytes.extend_from_slice(b"MTrk");
            bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
            bytes.extend_from_slice(track);
        }
        bytes
    }

    fn clock_events(bytes: &[u8]) -> Vec<(u64, u8, Vec<u8>)> {
        let mut cursor = 22;
        let mut tick = 0;
        let mut events = Vec::new();
        loop {
            tick += super::read_vlq(bytes, &mut cursor).unwrap();
            assert_eq!(bytes[cursor], 0xff);
            let kind = bytes[cursor + 1];
            cursor += 2;
            let length = super::read_vlq(bytes, &mut cursor).unwrap() as usize;
            if kind == 0x2f {
                break;
            }
            events.push((tick, kind, bytes[cursor..cursor + length].to_vec()));
            cursor += length;
        }
        events
    }

    #[test]
    fn parses_valid_smf() {
        for (source, target, expected) in [
            (480, None, 480),
            (480, Some(960), 960),
            (960, Some(3360), 3360),
        ] {
            let (duration, notes, events) = parse_smf(&minimal_smf(source), target).unwrap();
            assert_eq!(duration, expected);
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].duration_ticks, expected);
            assert!(events.is_empty());
        }
        let track = [1, 0x90, 60, 100, 1, 0x80, 60, 0, 0, 0xff, 0x2f, 0];
        let mut bytes = minimal_smf(960);
        bytes.truncate(14);
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        let (duration, notes, _) = parse_smf(&bytes, Some(3360)).unwrap();
        assert_eq!(
            (notes[0].start_tick.0, notes[0].duration_ticks, duration),
            (4, 3, 7)
        );
    }

    #[test]
    fn tracks_do_not_share_note_on_state() {
        let mut bytes = minimal_smf(480);
        bytes[11] = 2;
        let note_off = bytes.iter().position(|byte| *byte == 0x80).unwrap();
        bytes[note_off + 1] = 61;
        let second_track = minimal_smf(480);
        bytes.extend_from_slice(&second_track[14..]);

        let (duration, notes, _) = parse_smf(&bytes, Some(960)).unwrap();

        assert_eq!(duration, 960);
        assert_eq!(notes.len(), 2);
        assert!(
            notes
                .iter()
                .all(|note| note.start_tick.0 == 0 && note.duration_ticks == 960)
        );
        assert_ne!(notes[0].id, notes[1].id);

        let track = [
            0, 0x90, 60, 100, 0x81, 0x70, 0x90, 60, 80, 120, 0x80, 60, 0, 0x81, 0x70, 0x90, 60, 0,
            0, 0x90, 62, 90, 120, 0xff, 0x2f, 0,
        ];
        let (duration, notes, _) = parse_smf(&smf_tracks(960, &[&track]), Some(960)).unwrap();
        assert_eq!(duration, 720);
        assert_eq!(
            notes
                .iter()
                .map(|n| (n.note, n.start_tick.0, n.duration_ticks))
                .collect::<Vec<_>>(),
            vec![(60, 0, 360), (60, 240, 360), (62, 600, 120)]
        );
    }

    #[test]
    fn matches_utf8_lyrics_before_converting_ticks() {
        let track = [
            0, 0x90, 60, 100, 0, 0xff, 5, 3, 0xe3, 0x81, 0x8d, 0, 0x90, 64, 90, 0x83, 0x60, 0x80,
            60, 0, 0, 0x80, 64, 0, 0, 0xff, 0x2f, 0,
        ];
        let mut bytes = minimal_smf(480);
        bytes.truncate(14);
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);

        let (_, notes, _) = parse_smf(&bytes, Some(960)).unwrap();
        assert_eq!(notes[0].lyric, None);
        assert_eq!(notes[1].lyric.as_deref(), Some("き"));
        assert_eq!(notes[1].duration_ticks, 960);

        let mut invalid = bytes.clone();
        let index = invalid.iter().position(|byte| *byte == 0xe3).unwrap();
        invalid[index] = 0xff;
        let (_, notes, _) = parse_smf(&invalid, None).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|note| note.lyric.is_none()));

        // A controller between the lyric and the only Note On still leaves one candidate.
        let single = [
            0, 0xff, 5, 1, b'a', 0, 0xb0, 1, 20, 0, 0x90, 60, 100, 1, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ];
        let (_, notes, events) = parse_smf(&smf_tracks(480, &[&single]), None).unwrap();
        assert_eq!(notes[0].lyric.as_deref(), Some("a"));
        assert_eq!(events.len(), 1);

        // An intervening controller and two candidate notes make the lyric ambiguous.
        let ambiguous = [
            0, 0xff, 5, 1, b'a', 0, 0xb0, 1, 20, 0, 0x90, 60, 100, 0, 0x90, 64, 100, 1, 0x80, 60,
            0, 0, 0x80, 64, 0, 0, 0xff, 0x2f, 0,
        ];
        let (_, notes, _) = parse_smf(&smf_tracks(480, &[&ambiguous]), None).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|note| note.lyric.is_none()));

        let unmatched = [
            0, 0xff, 5, 1, b'a', 1, 0x90, 60, 100, 1, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ];
        let (_, notes, _) = parse_smf(&smf_tracks(480, &[&unmatched]), None).unwrap();
        assert_eq!(notes[0].lyric, None);

        // A skipped lyric does not prevent an adjacent lyric from identifying a chord note.
        let mixed = [
            0, 0xff, 5, 1, b'a', 0, 0xff, 5, 1, b'b', 0, 0x90, 60, 100, 0, 0x90, 64, 100, 1, 0x80,
            60, 0, 0, 0x80, 64, 0, 0, 0xff, 0x2f, 0,
        ];
        let (_, notes, _) = parse_smf(&smf_tracks(480, &[&mixed]), None).unwrap();
        assert_eq!(
            (notes[0].lyric.as_deref(), notes[1].lyric.as_deref()),
            (Some("b"), None)
        );

        // An already assigned note is never overwritten by a second lyric.
        let duplicate = [
            0, 0xff, 5, 1, b'a', 0, 0x90, 60, 100, 0, 0xff, 5, 1, b'b', 1, 0x80, 60, 0, 0, 0xff,
            0x2f, 0,
        ];
        let (_, notes, _) = parse_smf(&smf_tracks(480, &[&duplicate]), None).unwrap();
        assert_eq!(notes[0].lyric.as_deref(), Some("a"));
    }

    #[test]
    fn matches_separate_lyric_track_only_when_note_is_unique() {
        let mut bytes = minimal_smf(480);
        bytes[11] = 2;
        let lyric_track = [0, 0xff, 5, 3, 0xe3, 0x81, 0x8d, 0, 0xff, 0x2f, 0];
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(lyric_track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&lyric_track);
        let (_, notes, _) = parse_smf(&bytes, None).unwrap();
        assert_eq!(notes[0].lyric.as_deref(), Some("き"));

        let mut ambiguous = bytes.clone();
        ambiguous[11] = 3;
        ambiguous.extend_from_slice(&minimal_smf(480)[14..]);
        let (_, notes, _) = parse_smf(&ambiguous, None).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|note| note.lyric.is_none()));
    }

    #[test]
    fn exports_clock_and_round_trips_chord_lyrics() {
        use riffra_core::{
            MidiClip, MidiEvent, MidiEventKind, MidiNote, ProjectTimebase, TimelineTick,
        };
        let mut timebase = ProjectTimebase::default();
        timebase.tempo_changes.push(riffra_core::TempoChange {
            tick: 960,
            bpm: 90.0,
        });
        timebase
            .time_signature_changes
            .push(riffra_core::TimeSignatureChange {
                tick: 960,
                numerator: 3,
                denominator: 4,
            });
        let note = |pitch, lyric| MidiNote {
            id: format!("note:{pitch}"),
            note: pitch,
            start_tick: TimelineTick(120),
            duration_ticks: 240,
            velocity: 100,
            channel: 1,
            lyric,
        };
        let clip = MidiClip {
            id: "clip".into(),
            name: "clip".into(),
            track_id: "track".into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 1920,
            notes: vec![note(60, None), note(64, Some("き".into()))],
            events: vec![
                MidiEvent {
                    id: "cc".into(),
                    kind: MidiEventKind::ControlChange,
                    tick: TimelineTick(250),
                    channel: 2,
                    data1: 64,
                    data2: 127,
                },
                MidiEvent {
                    id: "bend".into(),
                    kind: MidiEventKind::PitchBend,
                    tick: TimelineTick(251),
                    channel: 3,
                    data1: 12,
                    data2: 65,
                },
                MidiEvent {
                    id: "pressure".into(),
                    kind: MidiEventKind::ChannelPressure,
                    tick: TimelineTick(252),
                    channel: 4,
                    data1: 55,
                    data2: 0,
                },
            ],
            instrument_control_events: vec![],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };
        let bytes = super::encode_smf(&timebase, &[(&clip, 0)], 0, None).unwrap();
        assert_eq!(
            clock_events(&bytes),
            vec![
                (0, 0x51, vec![0x07, 0xa1, 0x20]),
                (0, 0x58, vec![4, 2, 24, 8]),
                (960, 0x51, vec![0x0a, 0x2c, 0x2b]),
                (960, 0x58, vec![3, 2, 24, 8]),
            ]
        );
        let (_, notes, events) = parse_smf(&bytes, Some(timebase.ppq)).unwrap();
        assert_eq!(
            notes
                .iter()
                .map(|note| (
                    note.note,
                    note.lyric.as_deref(),
                    note.start_tick.0,
                    note.duration_ticks
                ))
                .collect::<Vec<_>>(),
            vec![(60, None, 120, 240), (64, Some("き"), 120, 240)]
        );
        assert_eq!(
            (
                events[0].tick.0,
                events[0].channel,
                events[0].data1,
                events[0].data2
            ),
            (250, 2, 64, 127)
        );
        assert_eq!(
            (
                events[1].kind,
                events[1].tick.0,
                events[1].channel,
                events[1].data1,
                events[1].data2
            ),
            (MidiEventKind::PitchBend, 251, 3, 12, 65)
        );
        assert_eq!(
            (
                events[2].kind,
                events[2].tick.0,
                events[2].channel,
                events[2].data1
            ),
            (MidiEventKind::ChannelPressure, 252, 4, 55)
        );
        let clip_bytes = super::encode_smf(&timebase, &[(&clip, 0)], 960, Some(1920)).unwrap();
        assert_eq!(
            clock_events(&clip_bytes),
            vec![
                (0, 0x51, vec![0x0a, 0x2c, 0x2b]),
                (0, 0x58, vec![3, 2, 24, 8]),
            ]
        );
        let mut other = clip.clone();
        other.id = "other".into();
        other.notes.truncate(1);
        other.notes[0].id = "overlap".into();
        let error =
            super::encode_smf(&timebase, &[(&clip, 0), (&other, 100)], 0, None).unwrap_err();
        assert!(error.contains("note:60") && error.contains("overlap"));
        other.notes[0].start_tick = TimelineTick(360);
        let adjacent = super::encode_smf(&timebase, &[(&clip, 0), (&other, 0)], 0, None).unwrap();
        let (_, notes, _) = parse_smf(&adjacent, None).unwrap();
        let same_pitch: Vec<_> = notes
            .iter()
            .filter(|note| note.note == 60)
            .map(|note| (note.start_tick.0, note.duration_ticks))
            .collect();
        assert_eq!(same_pitch, [(120, 240), (360, 240)]);
        other.notes[0].start_tick = TimelineTick(120);
        other.notes[0].channel = 2;
        assert!(super::encode_smf(&timebase, &[(&clip, 0), (&other, 0)], 0, None).is_ok());
        other.notes[0].channel = 1;
        other.notes.push(clip.notes[0].clone());
        assert!(super::encode_smf(&timebase, &[(&other, 0)], 0, None).is_err());
        timebase.ppq = 3360;
        let bytes = super::encode_smf(&timebase, &[(&clip, 0)], 0, None).unwrap();
        assert_eq!(u16::from_be_bytes([bytes[12], bytes[13]]), 3360);
        let (_, notes, _) = parse_smf(&bytes, Some(3360)).unwrap();
        assert_eq!(
            (
                notes[1].start_tick.0,
                notes[1].duration_ticks,
                notes[1].channel,
                notes[1].velocity
            ),
            (120, 240, 1, 100)
        );
        assert_eq!(notes[1].lyric.as_deref(), Some("き"));
        let mut vlq = Vec::new();
        assert!(super::encode_vlq(0x0fff_ffff, &mut vlq).is_ok());
        assert!(super::encode_vlq(0x1000_0000, &mut vlq).is_err());
        assert!(super::encode_smf(&timebase, &[(&clip, 0x1000_0000)], 0, None).is_err());
        let mut silent = clip.clone();
        silent.notes[0].velocity = 0;
        assert!(super::encode_smf(&timebase, &[(&silent, 0)], 0, None).is_err());
        timebase.ppq = 32768;
        assert!(super::encode_smf(&timebase, &[(&clip, 0)], 0, None).is_err());
    }

    #[test]
    fn rejects_malformed_smf() {
        assert!(parse_smf(b"not midi", None).is_err());
        let next = [0, 0xff, 0x2f, 0];
        for truncated in [&[0, 0x90, 60][..], &[0, 0xff, 5, 4, b'a'][..]] {
            assert!(parse_smf(&smf_tracks(480, &[truncated, &next]), None).is_err());
        }
    }
}
