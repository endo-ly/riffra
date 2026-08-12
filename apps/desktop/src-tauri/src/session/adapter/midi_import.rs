//! MIDI file import adapter.

use super::*;

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

/// Parses a Standard MIDI File (SMF) into the duration in project ticks plus
/// the normalized note and event lists. Shared by Asset import (validating an
/// incoming SMF) and MIDI clip placement (expanding an Asset into a clip).
///
/// # Errors
/// Returns a string error when the bytes are not a valid SMF (missing header,
/// truncated track, invalid variable-length value, non-ticks-per-quarter
/// division).
pub(crate) fn parse_midi_asset(
    bytes: &[u8],
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

pub fn add_midi_clip(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
) -> Result<CreativeSession, String> {
    if name.trim().is_empty() {
        return Err("MIDI clip name must not be empty.".into());
    }
    let source_asset = asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("MIDI Asset is not registered: {asset_id}"))?;
    if source_asset.kind != AssetKind::Midi {
        return Err(format!("Asset {asset_id} is not a MIDI Asset."));
    }
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("MIDI Asset could not be read: {error}"))?;
    let (duration_ticks, notes, events) = parse_midi_asset(&bytes)?;
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let mut create_track = None;
    let target_track_id = track_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.kind == TrackKind::Instrument)
                .map(|track| track.id.clone())
        })
        .unwrap_or_else(|| {
            let id = format!("track:{}", now_ms());
            create_track = Some("Instrument 1".to_owned());
            id
        });
    let target_track = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == target_track_id)
        .ok_or_else(|| format!("Track is not registered: {target_track_id}"))?;
    if target_track.kind != TrackKind::Instrument {
        return Err(format!(
            "Track is not an Instrument Track: {target_track_id}"
        ));
    }
    let clip = MidiClip {
        id: format!("midi-clip:{}:{}", asset_id.as_str(), now_ms()),
        name,
        track_id: target_track_id,
        asset_id: Some(asset_id),
        start_tick: start_tick.unwrap_or(TimelineTick(0)),
        duration_ticks,
        notes,
        events,
        muted: false,
        loop_enabled: false,
        recording_take_id: None,
    };
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .add_midi_clip_with_track(clip, create_track)
    })?;
    context.view_state.lock().map_err(lock_error)?.workspace = Workspace::Arrange;
    sync_arrangement(context)?;
    Ok(committed)
}
