//! Pure Standard MIDI File parsing.

use riffra_core::{MidiEvent, MidiEventKind, MidiNote, TimelineTick};
use std::collections::HashMap;

/// Parses an SMF into project-tick duration, notes, and non-note events.
///
/// Source PPQ is normalized to Riffra's 960 PPQ timeline. The parser performs
/// validation only; it does not read or write the Asset store.
pub fn parse_smf(bytes: &[u8]) -> Result<(u64, Vec<MidiNote>, Vec<MidiEvent>), String> {
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

    let mut cursor = 8 + header_len;
    let mut notes = Vec::new();
    let mut events = Vec::new();
    let mut last_tick = 0_u64;
    let mut note_starts: HashMap<(u8, u8), (u64, u8)> = HashMap::new();

    for _ in 0..track_count {
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
        while cursor < track_end {
            tick = tick.saturating_add(read_vlq(bytes, &mut cursor)?);
            last_tick = last_tick.max(tick);
            let first = *bytes
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
                let _meta_type = *bytes
                    .get(cursor)
                    .ok_or_else(|| "MIDI meta event is truncated.".to_string())?;
                cursor += 1;
                let length = read_vlq(bytes, &mut cursor)? as usize;
                cursor = cursor
                    .checked_add(length)
                    .ok_or_else(|| "MIDI meta event length overflowed.".to_string())?;
                if cursor > track_end {
                    return Err("MIDI meta event is truncated.".into());
                }
                continue;
            }
            if status == 0xf0 || status == 0xf7 {
                let length = read_vlq(bytes, &mut cursor)? as usize;
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
            let data1 = *bytes
                .get(cursor)
                .ok_or_else(|| "MIDI channel event is truncated.".to_string())?;
            cursor += 1;
            let data2 = if matches!(kind, 0xc | 0xd) {
                0
            } else {
                let value = *bytes
                    .get(cursor)
                    .ok_or_else(|| "MIDI channel event is truncated.".to_string())?;
                cursor += 1;
                value
            };
            let project_tick = (tick * 960 + u64::from(source_ppq) / 2) / u64::from(source_ppq);
            match kind {
                0x8 | 0x9 if kind == 0x8 || data2 == 0 => {
                    if let Some((start, velocity)) = note_starts.remove(&(channel, data1)) {
                        let end = project_tick.max(start + 1);
                        notes.push(MidiNote {
                            id: format!("note:asset:{channel}:{data1}:{start}"),
                            note: data1,
                            start_tick: TimelineTick(start),
                            duration_ticks: end - start,
                            velocity,
                            channel,
                        });
                        last_tick = last_tick.max(end);
                    }
                }
                0x9 => {
                    note_starts.insert((channel, data1), (project_tick, data2.max(1)));
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
        cursor = track_end;
    }

    for ((channel, note), (start, velocity)) in note_starts {
        let end = last_tick.max(start + 1);
        notes.push(MidiNote {
            id: format!("note:asset:{channel}:{note}:{start}"),
            note,
            start_tick: TimelineTick(start),
            duration_ticks: end - start,
            velocity,
            channel,
        });
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

    #[test]
    fn parses_valid_smf() {
        let (duration, notes, events) = parse_smf(&minimal_smf(480)).unwrap();
        assert_eq!(duration, 960);
        assert_eq!(notes.len(), 1);
        assert!(events.is_empty());
    }

    #[test]
    fn rejects_malformed_smf() {
        assert!(parse_smf(b"not midi").is_err());
    }
}
