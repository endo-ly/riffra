//! Session Application Operations: production workflows that change the
//! canonical [`CreativeSession`] and keep it consistent with the Audio Runtime
//! and the Asset registry.
//!
//! The operations use three consistency policies:
//!
//! - Sample-pad operations ([`create_sample_pad`], [`update_sample_pad`],
//!   [`remove_sample_pad`]) touch play state, design context, the Asset
//!   registry (existence check), and the Audio Runtime (pad configuration).
//!   Because the runtime and the persisted session must agree, each operation
//!   applies the new pad set to the runtime, persists the session, and restores
//!   the previous pad set when persistence fails.
//!
//! - Arrangement operations that change plugin topology prepare the proposed
//!   runtime graph before persisting the canonical Session. A failed candidate
//!   is rejected, and a persistence failure restores the previous graph. Other
//!   Arrangement operations commit first and submit a nonblocking projection.
//!
//! - Pure-session operations ([`commit_session`],
//!   [`save_session`], [`import_session`], [`restore_generation`], and
//!   [`open_asset_in_design`]) mutate the session and persist it without
//!   waiting for VST lifecycle work. Workspace navigation is view state: it is
//!   returned as an in-memory snapshot and sends only a nonblocking desired
//!   runtime mode, so navigation never enters the durable Session commit path.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the only compensation is re-applying the previous pad set, which
//! matches the runtime's "reconfigure the whole pad set" capability.

use std::collections::{HashMap, HashSet};
use std::{fs, path::Path};

use crate::asset::{self, AssetId, AssetKind};
use crate::errors::DomainError;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::plugin_catalog;
use crate::rack::{DeviceKind, RackDevice};
use crate::runtime::ports::RuntimeDriver;
use crate::session::{
    AiChangeSet, AiPermission, Arrangement, AudioClip, AudioInputRoute, AudioTakeVariant,
    AutomationLane, AutomationParameter, AutomationPoint, CreativeSession, DesignTool, Marker,
    MidiClip, MidiEvent, MidiEventKind, MidiInputRoute, MidiNote, MonitoringState, ProjectTimebase,
    SamplePad, TakeAudioSource, TimelineTick, Track, TrackKind, Workspace,
};
use crate::storage::{SessionStore, now_ms};

pub(crate) use crate::session::commit::{
    commit_merged_session, commit_session, import_session, next_session_update_timestamp,
    publish_session, restore_generation, save_session,
};
pub(crate) use crate::session::context::{SessionContext, lock_error};
pub(crate) use crate::session::transport::{
    audio_command_succeeded, go_to_start_timeline, play_timeline, prepare_arrangement_candidate,
    resolve_native_pads, restore_sample_pads, runtime_snapshot_for_recording, seek_timeline,
    stop_timeline, switch_workspace, sync_arrangement, sync_arrangement_runtime,
};

/// Rebuilds every Runtime that depends on the active audio device after the
/// Native device has been reopened. The canonical Session remains unchanged;
/// only the device-dependent Sample Pad buffers and Arrangement projection are
/// prepared again.
pub(crate) fn reconcile_runtime_after_audio_device_change(
    context: &SessionContext<'_>,
) -> Result<AudioStatus, String> {
    if !context.runtime.invalidate_for_audio_device_change() {
        return Err(
            "Audio Runtime graph is busy; the audio device change can be retried shortly.".into(),
        );
    }
    restore_sample_pads(context).map_err(|error| {
        format!("Sample Pad restoration failed after the audio device change: {error}")
    })?;
    sync_arrangement_runtime(context).map_err(|error| {
        format!("Arrangement Runtime restoration failed after the audio device change: {error}")
    })?;
    context.audio.refresh_status().map_err(String::from)
}

/// Creates a SamplePad from an existing audio Asset and commits it end-to-end:
/// asset existence + duplicate rules, pad id / MIDI key assignment, slice
/// validation, runtime configuration, session update, and persistence. The
/// design context is aimed at the new pad's asset.
///
/// Runtime configuration happens inside the operation; the caller applies the
/// returned session and audio status and does not sync the runtime separately.
/// If persistence fails after the runtime accepted the new pad set, the
/// previous pad set is re-applied.
pub fn create_sample_pad(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
) -> Result<SessionAudioPair, String> {
    if name.trim().is_empty() {
        return Err("Sample pad name must not be empty.".into());
    }
    let source_asset = asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("Sample pad references an unregistered asset: {asset_id}"))?;
    if source_asset.kind != AssetKind::Audio {
        return Err(format!("Asset {asset_id} is not an audio asset."));
    }
    let duration_ms =
        crate::analysis::analyze(std::path::Path::new(&source_asset.content_location))?
            .duration_ms
            .max(1);

    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    if previous_session
        .play_state
        .sample_instrument
        .pads
        .iter()
        .any(|pad| pad.asset_id == asset_id)
    {
        return Err("This asset is already mapped to a sample pad.".into());
    }

    let index = previous_session.play_state.sample_instrument.pads.len();
    let midi_key = u8::try_from(36 + index)
        .map_err(|_| "The sample instrument is full; no MIDI key is available.".to_string())?;

    let mut session = previous_session.clone();
    session.play_state.sample_instrument.pads.push(SamplePad {
        id: format!("pad:{}", asset_id.as_str()),
        name,
        asset_id: asset_id.clone(),
        start_ms: 0,
        end_ms: duration_ms,
        midi_key,
        gain_db: 0.0,
        loop_enabled: false,
    });
    session.workspace = Workspace::Design;
    session.design_context.active_tool = DesignTool::Sample;
    session.design_context.target_asset_id = Some(asset_id);

    // Apply the new pad set to the runtime first (unless Safe Mode keeps it
    // isolated). A faulted runtime is surfaced without touching the session.
    let runtime_generation = (!context.safe_mode).then(|| context.audio.sidecar_generation());
    let runtime_status = if context.safe_mode {
        None
    } else {
        let native_pads = resolve_native_pads(
            context.data_root,
            &session.play_state.sample_instrument.pads,
        )?;
        let status = context.audio.configure_sample_pads(&native_pads)?;
        if !audio_command_succeeded(&status) {
            // The runtime rejected the new pad set. Leave the session untouched
            // and report the faulted status.
            return Ok(SessionAudioPair {
                session: previous_session,
                audio: status,
            });
        }
        Some(status)
    };

    session.updated_at_ms =
        next_session_update_timestamp(previous_session.updated_at_ms, session.updated_at_ms);
    if let Err(error) = SessionStore::new(context.data_root).save(&session) {
        // Persistence failed after the runtime accepted the new pads. Restore
        // the previous pad set so the runtime and persisted session agree.
        if !context.safe_mode {
            let previous_native = resolve_native_pads(
                context.data_root,
                &previous_session.play_state.sample_instrument.pads,
            )?;
            return match context.audio.configure_sample_pads(&previous_native) {
                Ok(_) => Err(format!(
                    "The sample pad was applied to the runtime but the session could not be \
                     saved; the previous pad set was restored. Persistence error: {error}"
                )),
                Err(rollback_error) => Err(format!(
                    "The sample pad was applied to the runtime but the session could not be \
                     saved, and runtime rollback also failed ({rollback_error}). Persistence \
                     error: {error}"
                )),
            };
        }
        return Err(format!("The sample pad could not be saved: {error}"));
    }

    let committed = publish_session(
        context.session_actor,
        context.data_root,
        context.session,
        session,
        previous_session.workspace,
    )?;

    // In Safe Mode the runtime stayed isolated; report the current status so
    // React reflects the real (muted/offline) engine.
    let status = match reapply_sample_pads_after_generation_change(context, runtime_generation)? {
        Some(status) => status,
        None => match runtime_status {
            Some(status) => status,
            None => context.audio.refresh_status()?,
        },
    };
    Ok(SessionAudioPair {
        session: committed,
        audio: status,
    })
}

/// A partial update to an existing SamplePad. Only supplied fields are applied;
/// the canonical clamp/validation rules live here, not in React.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplePadPatch {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub gain_db: Option<f64>,
    pub loop_enabled: Option<bool>,
}

/// Commits the new pad set after a mutation: applies it to the runtime, persists
/// the session, and rolls the runtime back if persistence fails.
fn commit_pad_set(
    context: &SessionContext<'_>,
    previous_session: CreativeSession,
    mut session: CreativeSession,
) -> Result<SessionAudioPair, String> {
    let runtime_generation = (!context.safe_mode).then(|| context.audio.sidecar_generation());
    let runtime_status = if context.safe_mode {
        None
    } else {
        let native_pads = resolve_native_pads(
            context.data_root,
            &session.play_state.sample_instrument.pads,
        )?;
        let status = context.audio.configure_sample_pads(&native_pads)?;
        if !audio_command_succeeded(&status) {
            return Ok(SessionAudioPair {
                session: previous_session,
                audio: status,
            });
        }
        Some(status)
    };

    session.updated_at_ms =
        next_session_update_timestamp(previous_session.updated_at_ms, session.updated_at_ms);
    if let Err(error) = SessionStore::new(context.data_root).save(&session) {
        if !context.safe_mode {
            let previous_native = resolve_native_pads(
                context.data_root,
                &previous_session.play_state.sample_instrument.pads,
            )?;
            return match context.audio.configure_sample_pads(&previous_native) {
                Ok(_) => Err(format!(
                    "The pad change was applied to the runtime but the session could not be \
                     saved; the previous pad set was restored. Persistence error: {error}"
                )),
                Err(rollback_error) => Err(format!(
                    "The pad change was applied to the runtime but the session could not be \
                     saved, and runtime rollback also failed ({rollback_error}). Persistence \
                     error: {error}"
                )),
            };
        }
        return Err(format!("The pad change could not be saved: {error}"));
    }

    let committed = publish_session(
        context.session_actor,
        context.data_root,
        context.session,
        session,
        previous_session.workspace,
    )?;
    let status = match reapply_sample_pads_after_generation_change(context, runtime_generation)? {
        Some(status) => status,
        None => match runtime_status {
            Some(status) => status,
            None => context.audio.refresh_status()?,
        },
    };
    Ok(SessionAudioPair {
        session: committed,
        audio: status,
    })
}

fn reapply_sample_pads_after_generation_change(
    context: &SessionContext<'_>,
    previous_generation: Option<u64>,
) -> Result<Option<AudioStatus>, String> {
    let Some(previous_generation) = previous_generation else {
        return Ok(None);
    };
    if context.audio.sidecar_generation() == previous_generation {
        return Ok(None);
    }
    restore_sample_pads(context).map(Some)
}

/// Updates one SamplePad's slice range, gain, or loop flag through the canonical
/// clamp rules, then synchronizes the runtime and persists.
pub fn update_sample_pad(
    context: &SessionContext<'_>,
    pad_id: &str,
    patch: &SamplePadPatch,
) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let mut session = previous_session.clone();
    let pad = session
        .play_state
        .sample_instrument
        .pads
        .iter_mut()
        .find(|pad| pad.id == pad_id)
        .ok_or_else(|| format!("Sample pad is not registered: {pad_id}"))?;

    if let Some(gain_db) = patch.gain_db {
        pad.gain_db = if gain_db.is_finite() {
            gain_db.clamp(-90.0, 24.0)
        } else {
            0.0
        };
    }
    if let Some(loop_enabled) = patch.loop_enabled {
        pad.loop_enabled = loop_enabled;
    }
    // Apply range edits after scalar fields so the start/end invariant
    // (end > start) is enforced against the final values.
    match (patch.start_ms, patch.end_ms) {
        (Some(start), None) => {
            pad.start_ms = start;
            pad.end_ms = pad.end_ms.max(start + 1);
        }
        (None, Some(end)) => {
            let end = end.max(1);
            pad.end_ms = end;
            pad.start_ms = pad.start_ms.min(end - 1);
        }
        (Some(start), Some(end)) => {
            let end = end.max(start + 1);
            pad.start_ms = start;
            pad.end_ms = end;
        }
        (None, None) => {}
    }

    commit_pad_set(context, previous_session, session)
}

/// Removes a SamplePad, then synchronizes the runtime and persists.
pub fn remove_sample_pad(
    context: &SessionContext<'_>,
    pad_id: &str,
) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    if !previous_session
        .play_state
        .sample_instrument
        .pads
        .iter()
        .any(|pad| pad.id == pad_id)
    {
        return Err(format!("Sample pad is not registered: {pad_id}"));
    }
    let mut session = previous_session.clone();
    session
        .play_state
        .sample_instrument
        .pads
        .retain(|pad| pad.id != pad_id);
    commit_pad_set(context, previous_session, session)
}

// Session commit, Arrangement, and Design/Workspace operations.
//
// These mutate the canonical CreativeSession and persist it without touching
// the Audio Runtime. They share [`commit_session`] as the single
// validate-and-persist boundary so the save path lives in one place.

/// Applies a Domain-level mutation to the current session's [`Arrangement`],
/// then commits the whole session. Every Arrangement editing command funnels
/// through here so the validate/persist boundary stays in one place.
pub fn apply_arrangement_edit(
    context: &SessionContext<'_>,
    edit: impl FnOnce(&mut Arrangement) -> Result<(), DomainError>,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    edit(&mut session.arrangement).map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
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
    let mut session = context.session.lock().map_err(lock_error)?.clone();
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
            session
                .arrangement
                .tracks
                .push(Track::instrument(id.clone(), "Instrument 1".into()));
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
    session
        .arrangement
        .add_midi_clip(clip)
        .map_err(|error| error.to_string())?;
    session.workspace = Workspace::Arrange;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_timebase(
    context: &SessionContext<'_>,
    timebase: ProjectTimebase,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .update_timebase(timebase)
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_timeline_clips(
    context: &SessionContext<'_>,
    audio_clip_ids: &[String],
    midi_clip_ids: &[String],
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .remove_timeline_clips(audio_clip_ids, midi_clip_ids)
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn paste_timeline_clips(
    context: &SessionContext<'_>,
    audio_clip_ids: &[String],
    midi_clip_ids: &[String],
    start_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let stamp = now_ms();
    let revision = context
        .session
        .lock()
        .map_err(lock_error)?
        .arrangement
        .revision
        .saturating_add(1);
    let audio_ids = audio_clip_ids
        .iter()
        .enumerate()
        .map(|(index, _)| format!("clip:paste:{stamp}:{revision}:{index}"))
        .collect::<Vec<_>>();
    let midi_ids = midi_clip_ids
        .iter()
        .enumerate()
        .map(|(index, _)| format!("midi-clip:paste:{stamp}:{revision}:{index}"))
        .collect::<Vec<_>>();
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .paste_timeline_clips(
            audio_clip_ids,
            midi_clip_ids,
            &audio_ids,
            &midi_ids,
            start_tick,
        )
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn trim_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    source_range: crate::session::FrameRange,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let clip = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Audio clip '{clip_id}' not found."))?;
    let source_asset = asset::load(context.data_root, &clip.asset_id)
        .ok_or_else(|| format!("Audio Asset is not registered: {}", clip.asset_id))?;
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
    let wav = crate::analysis::parse_wav(&bytes)?;
    let frame_bytes = usize::from(wav.bits_per_sample / 8) * usize::from(wav.channels);
    if frame_bytes == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    session
        .arrangement
        .trim_audio_clip(
            clip_id,
            start_tick,
            source_range,
            (wav.data_len / frame_bytes) as u64,
        )
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

/// Adds an audio clip referencing a canonical Asset to the arrangement, then
/// commits the session and switches to the Arrange workspace.
pub fn add_audio_clip(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
) -> Result<CreativeSession, String> {
    if name.trim().is_empty() {
        return Err("Audio clip name must not be empty.".into());
    }
    let source_asset = asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("Audio Asset is not registered: {asset_id}"))?;
    if source_asset.kind != AssetKind::Audio {
        return Err(format!("Asset {asset_id} is not an audio Asset."));
    }
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
    let wav = crate::analysis::parse_wav(&bytes)?;
    let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
    let frame_bytes = bytes_per_sample.saturating_mul(usize::from(wav.channels));
    if frame_bytes == 0 || wav.sample_rate == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    let source_frames = (wav.data_len / frame_bytes) as u64;
    if source_frames == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track_id = track_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.kind == crate::session::TrackKind::Audio)
                .map(|track| track.id.clone())
        })
        .unwrap_or_else(|| {
            let id = format!("track:{}", now_ms());
            session
                .arrangement
                .tracks
                .push(Track::audio(id.clone(), "Audio 1".into()));
            id
        });
    let target_track = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if target_track.kind != crate::session::TrackKind::Audio {
        return Err(format!("Track is not an Audio Track: {track_id}"));
    }
    let append_tick = session
        .arrangement
        .audio_clips
        .iter()
        .map(|clip| {
            let duration = session.arrangement.timebase.milliseconds_to_ticks(
                clip.timeline_duration.frames as f64 * 1000.0
                    / f64::from(clip.timeline_duration.sample_rate),
            );
            clip.start_tick.0.saturating_add(duration.0)
        })
        .max()
        .unwrap_or(0);
    let clip = crate::session::AudioClip::full_source(
        format!("clip:{}:{}", asset_id.as_str(), now_ms()),
        name,
        track_id,
        asset_id,
        start_tick.unwrap_or(TimelineTick(append_tick)),
        wav.sample_rate,
        source_frames,
    );
    session
        .arrangement
        .add_audio_clip(clip, |id| asset::load(context.data_root, id).is_some())
        .map_err(|error| error.to_string())?;
    session.workspace = Workspace::Arrange;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

/// Opens a canonical Asset in the Design workspace with the given tool. One
/// user intent updates workspace, active tool, and target asset together
/// instead of three separate setters. The Asset must be registered.
pub fn open_asset_in_design(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    tool: DesignTool,
) -> Result<CreativeSession, String> {
    if asset::load(context.data_root, &asset_id).is_none() {
        return Err(format!(
            "Design target is not a registered asset: {asset_id}"
        ));
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session.workspace = Workspace::Design;
    session.design_context.active_tool = tool;
    session.design_context.target_asset_id = Some(asset_id);
    commit_session(context, session)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettingsPatch {
    pub project_name: Option<Option<String>>,
    pub loop_enabled: Option<bool>,
    pub count_in_beats: Option<u8>,
    pub metronome_enabled: Option<bool>,
    pub note: Option<String>,
    pub ai_permission: Option<AiPermission>,
    pub ai_context: Option<Vec<String>>,
}

pub fn update_session_settings(
    context: &SessionContext<'_>,
    patch: SessionSettingsPatch,
) -> Result<CreativeSession, String> {
    let metronome_changed = patch.metronome_enabled.is_some();
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    if let Some(project_name) = patch.project_name {
        session.project_name = project_name
            .map(|value| value.trim().chars().take(160).collect::<String>())
            .filter(|value| !value.is_empty());
    }
    if let Some(loop_enabled) = patch.loop_enabled {
        session.settings.loop_enabled = loop_enabled;
    }
    if let Some(count_in_beats) = patch.count_in_beats {
        session.settings.count_in_beats = count_in_beats.min(8);
    }
    if let Some(metronome_enabled) = patch.metronome_enabled {
        session.settings.metronome_enabled = metronome_enabled;
    }
    if let Some(note) = patch.note {
        session.settings.note = note.chars().take(16_384).collect();
    }
    if let Some(permission) = patch.ai_permission {
        session.settings.ai_permission = permission;
    }
    if let Some(context_items) = patch.ai_context {
        session.settings.ai_context = context_items;
    }
    let committed = commit_session(context, session)?;
    if metronome_changed {
        sync_arrangement(context)?;
    }
    Ok(committed)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitoring: Option<MonitoringState>,
}

fn commit_structural_arrangement(
    context: &SessionContext<'_>,
    candidate: CreativeSession,
) -> Result<CreativeSession, String> {
    let committed = commit_session(context, candidate)?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

fn repair_previous_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    original_error: String,
) -> String {
    if !context.runtime.reset_for_repair() {
        return format!(
            "Arrangement Runtime rejected the VST candidate and could not be reset for the canonical Session: {original_error}"
        );
    }
    match sync_arrangement_runtime(context) {
        Ok(_) => format!(
            "Arrangement Runtime rejected the VST candidate; the canonical Session was restored: {original_error}"
        ),
        Err(restore_error) => format!(
            "Arrangement Runtime rejected the VST candidate and the previous graph could not be restored ({restore_error}): {original_error}"
        ),
    }
}

/// Validates a plugin-bearing candidate against the real Arrangement Runtime
/// before persisting it. A failed candidate never becomes part of the
/// canonical Session, and a persistence failure repairs the previous graph.
fn commit_plugin_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    candidate: CreativeSession,
    base_sequence: u64,
) -> Result<CreativeSession, String> {
    let candidate = candidate.validate_and_normalize()?;
    if let Err(error) = prepare_arrangement_candidate(context, &candidate, base_sequence) {
        return Err(repair_previous_arrangement(context, error));
    }
    let commit_result = {
        let _session_operation = context.session_actor.enter()?;
        let current = context
            .session_actor
            .capture_projection_while_held(context.session)?;
        if current.sequence != base_sequence {
            Err("Canonical Session changed while the VST candidate was being prepared.".into())
        } else {
            commit_session(context, candidate)
        }
    };
    let committed = match commit_result {
        Ok(committed) => committed,
        Err(error) => {
            return Err(repair_previous_arrangement(context, error));
        }
    };
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

fn track_device_mut<'a>(track: &'a mut Track, device_id: &str) -> Option<&'a mut RackDevice> {
    if track
        .instrument
        .as_ref()
        .is_some_and(|device| device.id == device_id)
    {
        return track.instrument.as_mut();
    }
    track
        .rack
        .devices
        .iter_mut()
        .find(|device| device.id == device_id)
}

fn plugin_device(path: &str, id: String) -> Result<RackDevice, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("VST3 path must not be empty.".into());
    }
    let name = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned();
    Ok(RackDevice {
        id,
        name,
        kind: DeviceKind::Plugin,
        path: Some(path.to_owned()),
        bypassed: false,
        gain_db: 0.0,
        parameter_values: Vec::new(),
        state_data: None,
        disabled_placeholder: false,
    })
}

pub fn set_track_audio_input(
    context: &SessionContext<'_>,
    track_id: &str,
    channel_index: Option<u32>,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if track.kind != TrackKind::Audio {
        return Err("Only Audio Tracks can route a physical Audio Input.".into());
    }
    track.audio_input = channel_index.map(|channel_index| AudioInputRoute { channel_index });
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn set_track_midi_input(
    context: &SessionContext<'_>,
    track_id: &str,
    route: MidiInputRoute,
) -> Result<CreativeSession, String> {
    if route
        .channel
        .is_some_and(|channel| !(1..=16).contains(&channel))
    {
        return Err("MIDI channel must be between 1 and 16.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if track.kind != TrackKind::Instrument {
        return Err("Only Instrument Tracks can route MIDI Input.".into());
    }
    track.midi_input = route;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn set_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
) -> Result<CreativeSession, String> {
    if context.safe_mode {
        return Err("Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect instruments.".into());
    }
    let (name, validated_path) =
        plugin_catalog::validated_plugin(context.data_root, Path::new(path))?;
    let previous = context.session_actor.capture_projection(context.session)?;
    let mut session = previous.session;
    let revision = session.arrangement.revision;
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if track.kind != TrackKind::Instrument {
        return Err("Only Instrument Tracks can host an Instrument.".into());
    }
    let id = track
        .instrument
        .as_ref()
        .map(|device| device.id.clone())
        .unwrap_or_else(|| format!("device:instrument:{}:{}", now_ms(), revision));
    track.instrument = Some(plugin_device(&validated_path.to_string_lossy(), id)?);
    track.instrument.as_mut().unwrap().name = name;
    session.arrangement.revision = revision.saturating_add(1);
    commit_plugin_arrangement(context, session, previous.sequence)
}

pub fn clear_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if track.kind != TrackKind::Instrument {
        return Err("Only Instrument Tracks can host an Instrument.".into());
    }
    track.instrument = None;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn add_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
) -> Result<CreativeSession, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect effects."
                .into(),
        );
    }
    let (name, validated_path) =
        plugin_catalog::validated_plugin(context.data_root, Path::new(path))?;
    let previous = context.session_actor.capture_projection(context.session)?;
    let mut session = previous.session;
    let revision = session.arrangement.revision;
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    let id = format!("device:effect:{}:{}", now_ms(), revision);
    let mut device = plugin_device(&validated_path.to_string_lossy(), id)?;
    device.name = name;
    track.rack.devices.push(device);
    session.arrangement.revision = revision.saturating_add(1);
    commit_plugin_arrangement(context, session, previous.sequence)
}

pub fn remove_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    let before = track.rack.devices.len();
    track.rack.devices.retain(|device| device.id != device_id);
    if before == track.rack.devices.len() {
        return Err(format!("Track Effect is not registered: {device_id}"));
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn reorder_track_effects(
    context: &SessionContext<'_>,
    track_id: &str,
    ordered_device_ids: &[String],
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    let unique_ids: HashSet<&str> = ordered_device_ids.iter().map(String::as_str).collect();
    if ordered_device_ids.len() != track.rack.devices.len()
        || unique_ids.len() != ordered_device_ids.len()
        || ordered_device_ids
            .iter()
            .any(|id| !track.rack.devices.iter().any(|device| &device.id == id))
    {
        return Err("Effect order must contain every Track Effect exactly once.".into());
    }
    let mut reordered = Vec::with_capacity(track.rack.devices.len());
    for id in ordered_device_ids {
        let index = track
            .rack
            .devices
            .iter()
            .position(|device| &device.id == id)
            .ok_or_else(|| format!("Track Effect is not registered: {id}"))?;
        reordered.push(track.rack.devices.remove(index));
    }
    track.rack.devices = reordered;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn set_track_device_bypassed(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    bypassed: bool,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let device = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .and_then(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    let previous = device.bypassed;
    context
        .audio
        .set_track_device_bypassed(track_id, device_id, bypassed)?;
    device.bypassed = bypassed;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    match commit_session(context, session) {
        Ok(committed) => Ok(committed),
        Err(error) => {
            let _ = context
                .audio
                .set_track_device_bypassed(track_id, device_id, previous);
            Err(error)
        }
    }
}

pub fn set_track_device_parameter(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_index: u32,
    value: f32,
) -> Result<CreativeSession, String> {
    if !value.is_finite() {
        return Err("Track Device parameter value must be finite.".into());
    }
    let value = value.clamp(0.0, 1.0);
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let device = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .and_then(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Device parameter index is invalid.".to_string())?;
    let previous = device.parameter_values.get(index).copied().unwrap_or(0.0);
    context
        .audio
        .set_track_device_parameter(track_id, device_id, parameter_index, value)?;
    if device.parameter_values.len() <= index {
        device.parameter_values.resize(index + 1, 0.0);
    }
    device.parameter_values[index] = value;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    match commit_session(context, session) {
        Ok(committed) => Ok(committed),
        Err(error) => {
            let _ = context.audio.set_track_device_parameter(
                track_id,
                device_id,
                parameter_index,
                previous,
            );
            Err(error)
        }
    }
}

pub fn open_track_plugin_editor(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<(), String> {
    let session = context.session.lock().map_err(lock_error)?;
    let registered = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .is_some_and(|track| {
            track
                .instrument
                .as_ref()
                .is_some_and(|device| device.id == device_id)
                || track
                    .rack
                    .devices
                    .iter()
                    .any(|device| device.id == device_id)
        });
    if !registered {
        return Err(format!("Track Device is not registered: {device_id}"));
    }
    drop(session);
    context
        .audio
        .open_track_plugin_editor(track_id, device_id)
        .map_err(String::from)
}

/// Persists state captured from the native Track Plugin Editor into the
/// canonical Session. The editor already owns the playback instance and the
/// Native Runtime mirrors the state into the live instance, so this operation
/// deliberately does not rebuild or reapply the plugin graph.
pub fn persist_track_plugin_state(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
) -> Result<CreativeSession, String> {
    if parameter_values.iter().any(|value| !value.is_finite()) {
        return Err("Track Plugin Editor returned a non-finite parameter value.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    apply_track_plugin_state(
        &mut session,
        track_id,
        device_id,
        parameter_values,
        state_data,
        bypassed,
    )?;
    commit_session(context, session)
}

/// Persists one editor-originated parameter without routing it back through
/// Native. The playback instance has already changed and the live instance
/// receives the same value through its block-boundary queue.
pub fn persist_track_plugin_parameter(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    parameter_index: i32,
    value: f32,
) -> Result<CreativeSession, String> {
    if parameter_index < 0 || !value.is_finite() {
        return Err("Track Plugin Editor returned an invalid parameter change.".into());
    }
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Plugin Editor returned an invalid parameter index.".to_string())?;
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let device = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .and_then(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    if device.parameter_values.len() <= index {
        device.parameter_values.resize(index + 1, 0.0);
    }
    device.parameter_values[index] = value;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_session(context, session)
}

fn apply_track_plugin_state(
    session: &mut CreativeSession,
    track_id: &str,
    device_id: &str,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
) -> Result<(), String> {
    let device = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .and_then(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    device.parameter_values = parameter_values;
    device.state_data = state_data.filter(|value| !value.is_empty());
    device.bypassed = bypassed;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    Ok(())
}

pub fn add_track(
    context: &SessionContext<'_>,
    name: String,
    kind: TrackKind,
) -> Result<CreativeSession, String> {
    let name = name.trim().chars().take(80).collect::<String>();
    if name.is_empty() {
        return Err("Track name must not be empty.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session.arrangement.tracks.push(Track {
        id: format!("track:{}", now_ms()),
        name,
        kind,
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: MonitoringState::Off,
        audio_input: None,
        midi_input: crate::session::MidiInputRoute::default(),
        instrument: None,
        rack: crate::rack::RackInstance {
            devices: Vec::new(),
            macros: Vec::new(),
        },
    });
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

pub fn update_track<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    patch: TrackPatch,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    if let Some(value) = patch.name {
        let name = value.trim().chars().take(80).collect::<String>();
        if name.is_empty() {
            return Err("Track name must not be empty.".into());
        }
        track.name = name;
    }
    if let Some(value) = patch.gain_db {
        track.gain_db = if value.is_finite() {
            value.clamp(-90.0, 24.0)
        } else {
            0.0
        };
    }
    if let Some(value) = patch.pan {
        track.pan = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
    if let Some(value) = patch.muted {
        track.muted = value;
    }
    if let Some(value) = patch.solo {
        track.solo = value;
    }
    if let Some(value) = patch.armed {
        track.armed = value;
    }
    if let Some(value) = patch.monitoring {
        track.monitoring = value;
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

/// Replaces one Track Automation Lane in a single canonical edit.
///
/// The UI previews pointer movement locally and calls this once on pointer-up.
/// An empty point list removes the lane so the Track's regular value applies.
pub fn set_track_automation(
    context: &SessionContext<'_>,
    track_id: &str,
    parameter: AutomationParameter,
    mut points: Vec<AutomationPoint>,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    if !session
        .arrangement
        .tracks
        .iter()
        .any(|track| track.id == track_id)
    {
        return Err(format!("Track is not registered: {track_id}"));
    }
    points.sort_by_key(|point| point.tick);
    session
        .arrangement
        .automation_lanes
        .retain(|lane| lane.track_id != track_id || lane.parameter != parameter);
    if !points.is_empty() {
        let parameter_name = match parameter {
            AutomationParameter::Volume => "volume",
            AutomationParameter::Pan => "pan",
        };
        session.arrangement.automation_lanes.push(AutomationLane {
            id: format!("automation:{track_id}:{parameter_name}"),
            track_id: track_id.to_owned(),
            parameter,
            points,
        });
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

/// Removes a Track and its Clips without deleting any referenced Asset.
pub fn remove_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .remove_track(track_id)
        .map_err(|error| error.to_string())?;
    commit_structural_arrangement(context, session)
}

/// Duplicates a Track and its non-destructive Clip references.
pub fn duplicate_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let source_index = session
        .arrangement
        .tracks
        .iter()
        .position(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    let operation_id = now_ms();
    let mut duplicate = session.arrangement.tracks[source_index].clone();
    duplicate.id = format!("track:{operation_id}");
    duplicate.name = format!("{} copy", duplicate.name);
    let duplicate_id = duplicate.id.clone();
    session
        .arrangement
        .tracks
        .insert(source_index + 1, duplicate);

    let clips = session
        .arrangement
        .audio_clips
        .iter()
        .filter(|clip| clip.track_id == track_id)
        .cloned()
        .enumerate()
        .map(|(index, mut clip)| {
            clip.id = format!("clip:{operation_id}:{index}");
            clip.track_id = duplicate_id.clone();
            clip
        })
        .collect::<Vec<_>>();
    session.arrangement.audio_clips.extend(clips);
    let midi_clips = session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| clip.track_id == track_id)
        .cloned()
        .enumerate()
        .map(|(index, mut clip)| {
            clip.id = format!("midi-clip:{operation_id}:{index}");
            clip.track_id = duplicate_id.clone();
            clip
        })
        .collect::<Vec<_>>();
    session.arrangement.midi_clips.extend(midi_clips);
    let automation_lanes = session
        .arrangement
        .automation_lanes
        .iter()
        .filter(|lane| lane.track_id == track_id)
        .cloned()
        .enumerate()
        .map(|(index, mut lane)| {
            lane.id = format!("automation:{duplicate_id}:{index}");
            lane.track_id = duplicate_id.clone();
            for (point_index, point) in lane.points.iter_mut().enumerate() {
                point.id = format!("automation-point:{operation_id}:{index}:{point_index}");
            }
            lane
        })
        .collect::<Vec<_>>();
    session
        .arrangement
        .automation_lanes
        .extend(automation_lanes);
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_structural_arrangement(context, session)
}

/// Moves a Track to a zero-based position while preserving Clip ownership.
pub fn reorder_track(
    context: &SessionContext<'_>,
    track_id: &str,
    target_index: usize,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .reorder_track(track_id, target_index)
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

// Marker operations. Markers are timeline authoring metadata with no audio
// runtime impact, so they skip the audio sync and go straight through
// `commit_session`.

pub fn add_marker(
    context: &SessionContext<'_>,
    tick: TimelineTick,
    name: String,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let trimmed: String = name.trim().chars().take(80).collect();
    session.arrangement.markers.push(Marker {
        id: format!("marker:{}", now_ms()),
        name: if trimmed.is_empty() {
            "Marker".into()
        } else {
            trimmed
        },
        tick: tick.0,
    });
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_session(context, session)
}

pub fn update_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
    name: Option<String>,
    tick: Option<TimelineTick>,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let marker = session
        .arrangement
        .markers
        .iter_mut()
        .find(|marker| marker.id == marker_id)
        .ok_or_else(|| format!("Marker is not registered: {marker_id}"))?;
    if let Some(name) = name {
        let trimmed: String = name.trim().chars().take(80).collect();
        marker.name = if trimmed.is_empty() {
            marker.name.clone()
        } else {
            trimmed
        };
    }
    if let Some(tick) = tick {
        marker.tick = tick.0;
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_session(context, session)
}

pub fn remove_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let before = session.arrangement.markers.len();
    session
        .arrangement
        .markers
        .retain(|marker| marker.id != marker_id);
    if session.arrangement.markers.len() == before {
        return Err(format!("Marker is not registered: {marker_id}"));
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_session(context, session)
}

/// Adds a single MIDI note to an existing MIDI clip. The note id is minted by
/// the Application layer so the React side never invents identity.
pub fn add_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    pitch: u8,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
) -> Result<CreativeSession, String> {
    if pitch > 127 {
        return Err("MIDI pitch must be between 0 and 127.".into());
    }
    if velocity > 127 {
        return Err("MIDI velocity must be between 0 and 127.".into());
    }
    if channel == 0 || channel > 16 {
        return Err("MIDI channel must be between 1 and 16.".into());
    }
    let duration_ticks = duration_ticks.max(1);
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let clip = session
        .arrangement
        .midi_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("MIDI clip is not registered: {clip_id}"))?;
    clip.notes.push(MidiNote {
        id: format!("note:{}", now_ms()),
        note: pitch,
        start_tick,
        duration_ticks,
        velocity,
        channel,
    });
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_id: &str,
    patch: MidiNotePatch,
) -> Result<CreativeSession, String> {
    update_midi_notes(
        context,
        clip_id,
        vec![MidiNoteUpdate {
            note_id: note_id.to_owned(),
            patch,
        }],
    )
}

pub fn update_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    updates: Vec<MidiNoteUpdate>,
) -> Result<CreativeSession, String> {
    if updates.is_empty() {
        return Err("At least one MIDI Note update is required.".into());
    }
    let mut ids = std::collections::HashSet::new();
    if updates
        .iter()
        .any(|update| !ids.insert(update.note_id.as_str()))
    {
        return Err("Each MIDI Note may be updated only once per operation.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let clip = session
        .arrangement
        .midi_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("MIDI clip is not registered: {clip_id}"))?;
    for update in updates {
        let note = clip
            .notes
            .iter_mut()
            .find(|note| note.id == update.note_id)
            .ok_or_else(|| format!("MIDI note is not registered: {}", update.note_id))?;
        if let Some(pitch) = update.patch.note {
            note.note = pitch.min(127);
        }
        if let Some(start_tick) = update.patch.start_tick {
            note.start_tick = start_tick;
        }
        if let Some(duration) = update.patch.duration_ticks {
            note.duration_ticks = duration.max(1);
        }
        if let Some(velocity) = update.patch.velocity {
            note.velocity = velocity.min(127);
        }
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let clip = session
        .arrangement
        .midi_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("MIDI clip is not registered: {clip_id}"))?;
    clip.notes.retain(|note| note.id != note_id);
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn quantize_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    grid_ticks: u64,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .quantize_midi_notes(clip_id, note_ids, grid_ticks)
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    offset_ticks: u64,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session
        .arrangement
        .duplicate_midi_notes(clip_id, note_ids, offset_ticks)
        .map_err(|error| error.to_string())?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn set_audio_clip_take_variant(
    context: &SessionContext<'_>,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    apply_audio_clip_take_variant(&mut session, clip_id, variant)?;
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

fn apply_audio_clip_take_variant(
    session: &mut CreativeSession,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<(), String> {
    let take_id = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .and_then(|clip| clip.recording_take_id.clone())
        .ok_or_else(|| format!("Audio Clip has no Recording Take: {clip_id}"))?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let source = take
        .audio_source(variant)
        .cloned()
        .ok_or_else(|| "The requested Take variant is not available.".to_string())?;
    let clip = session
        .arrangement
        .audio_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Audio Clip is not registered: {clip_id}"))?;
    // Variant data can contain an older latency/tail-inclusive range. Keep the
    // clip's logical span stable while choosing the matching source window;
    // modern captures already have equal performance ranges for both variants.
    let prior_duration = clip.timeline_duration;
    let target_frames = if prior_duration.sample_rate > 0 && source.sample_rate > 0 {
        ((prior_duration.frames as f64 * f64::from(source.sample_rate)
            / f64::from(prior_duration.sample_rate))
        .round() as u64)
            .max(1)
    } else {
        source
            .source_end_sample
            .saturating_sub(source.source_start_sample)
    };
    let mut selected_source = source;
    if selected_source
        .source_end_sample
        .saturating_sub(selected_source.source_start_sample)
        != target_frames
        && selected_source
            .source_start_sample
            .saturating_add(target_frames)
            <= selected_source.source_end_sample
    {
        selected_source.source_end_sample = selected_source
            .source_start_sample
            .saturating_add(target_frames);
    }
    apply_audio_source_to_clip(clip, &selected_source);
    clip.take_variant = variant;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    Ok(())
}

fn apply_audio_source_to_clip(clip: &mut AudioClip, source: &TakeAudioSource) {
    clip.asset_id = source.asset_id.clone();
    clip.source_range.start = source.source_start_sample;
    clip.source_range.end = source.source_end_sample;
    clip.timeline_duration.frames = source
        .source_end_sample
        .saturating_sub(source.source_start_sample);
    clip.source_sample_rate = source.sample_rate;
    clip.timeline_duration.sample_rate = source.sample_rate;
    clip.fade_in.sample_rate = source.sample_rate;
    clip.fade_out.sample_rate = source.sample_rate;
    clip.normalize_fields();
}

pub fn start_take_comparison(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<AudioStatus, String> {
    let session = context.session.lock().map_err(lock_error)?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let raw_source = take
        .raw_audio
        .as_ref()
        .ok_or_else(|| "Take comparison requires a Raw Asset.".to_string())?;
    let processed_source = take
        .processed_audio
        .as_ref()
        .ok_or_else(|| "Take comparison requires a Processed Asset.".to_string())?;
    let raw = asset::load(context.data_root, &raw_source.asset_id)
        .ok_or_else(|| "Take Raw Asset is unavailable.".to_string())?;
    let processed = asset::load(context.data_root, &processed_source.asset_id)
        .ok_or_else(|| "Take Processed Asset is unavailable.".to_string())?;
    let raw_start_frame = raw_source.source_start_sample;
    let raw_end_frame = raw_source.source_end_sample;
    let processed_start_frame = processed_source.source_start_sample;
    let processed_end_frame = processed_source.source_end_sample;
    drop(session);
    context
        .audio
        .start_take_comparison(
            Path::new(&raw.content_location),
            Path::new(&processed.content_location),
            raw_start_frame,
            raw_end_frame,
            processed_start_frame,
            processed_end_frame,
        )
        .map_err(String::from)
}

pub fn switch_take_comparison_variant(
    context: &SessionContext<'_>,
    variant: AudioTakeVariant,
) -> Result<AudioStatus, String> {
    context
        .audio
        .switch_take_comparison_variant(variant)
        .map_err(String::from)
}

pub fn stop_take_comparison(context: &SessionContext<'_>) -> Result<AudioStatus, String> {
    context.audio.stop_take_comparison().map_err(String::from)
}

pub fn activate_take(
    context: &SessionContext<'_>,
    session_id: &str,
    take_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let target_take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.session_id == session_id && take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let slot = session
        .arrangement
        .recording_sessions
        .iter_mut()
        .find(|recording| recording.id == session_id)
        .and_then(|recording| {
            recording
                .track_slots
                .iter_mut()
                .find(|slot| slot.track_id == target_take.track_id)
        })
        .ok_or_else(|| {
            format!(
                "Recording Session has no Track Slot for {}",
                target_take.track_id
            )
        })?;
    let timeline_clip_id = slot.timeline_clip_id.clone();
    slot.active_take_id = take_id.to_owned();
    if let Some(clip) = session
        .arrangement
        .audio_clips
        .iter_mut()
        .find(|clip| clip.id == timeline_clip_id)
    {
        let source = target_take
            .preferred_audio_source(clip.take_variant)
            .cloned()
            .ok_or_else(|| "The selected Take has no Audio Asset.".to_string())?;
        apply_audio_source_to_clip(clip, &source);
        clip.recording_take_id = Some(take_id.to_owned());
    } else if target_take.midi_asset_id.is_some() {
        let source = crate::recording::application::midi_clip_for_take(
            context.data_root,
            &target_take,
            session.arrangement.timebase,
            timeline_clip_id.clone(),
        )?;
        let clip = session
            .arrangement
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == timeline_clip_id)
            .ok_or_else(|| "Recording Take slot has no MIDI Clip.".to_string())?;
        clip.asset_id = target_take.midi_asset_id.clone();
        clip.notes = source.notes;
        clip.events = source.events;
        clip.duration_ticks = target_take.duration_ticks;
        clip.recording_take_id = Some(take_id.to_owned());
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn place_take_as_separate_clip(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let new_clip_id = format!(
        "clip:take-place:{}:{}",
        now_ms(),
        session.arrangement.revision
    );
    if let Some(source) = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| clip.recording_take_id.as_deref() == Some(take_id))
        .cloned()
    {
        let mut clip = source;
        clip.id = new_clip_id;
        clip.muted = false;
        session.arrangement.audio_clips.push(clip);
    } else if take.raw_audio.is_some() || take.processed_audio.is_some() {
        let slot_clip_id = session
            .arrangement
            .recording_sessions
            .iter()
            .find(|recording| recording.id == take.session_id)
            .and_then(|recording| {
                recording
                    .track_slots
                    .iter()
                    .find(|slot| slot.track_id == take.track_id)
            })
            .map(|slot| slot.timeline_clip_id.clone())
            .ok_or_else(|| "Recording Take Track Slot is unavailable.".to_string())?;
        let mut clip = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == slot_clip_id)
            .cloned()
            .ok_or_else(|| "Recording Take slot has no Audio Clip.".to_string())?;
        clip.id = new_clip_id;
        clip.start_tick = take.start_tick;
        let source = take
            .preferred_audio_source(clip.take_variant)
            .cloned()
            .ok_or_else(|| "Recording Take has no usable Audio Asset.".to_string())?;
        apply_audio_source_to_clip(&mut clip, &source);
        clip.recording_take_id = Some(take.id);
        clip.muted = false;
        session.arrangement.audio_clips.push(clip);
    } else if take.midi_asset_id.is_some() {
        let clip = crate::recording::application::midi_clip_for_take(
            context.data_root,
            &take,
            session.arrangement.timebase,
            new_clip_id,
        )?;
        session.arrangement.midi_clips.push(clip);
    } else {
        return Err(format!("Recording Take has no Timeline Clip: {take_id}"));
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement(context)?;
    Ok(committed)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNotePatch {
    pub note: Option<u8>,
    pub start_tick: Option<TimelineTick>,
    pub duration_ticks: Option<u64>,
    pub velocity: Option<u8>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdate {
    pub note_id: String,
    pub patch: MidiNotePatch,
}

pub fn apply_ai_suggestion(
    context: &SessionContext<'_>,
    clip_id: &str,
    proposed_gain_db: f64,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    if session.settings.ai_permission != AiPermission::Apply {
        return Err("AI suggestion application requires Apply permission.".into());
    }
    let clip = session
        .arrangement
        .audio_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Audio clip is not registered: {clip_id}"))?;
    let current_gain_db = clip.gain_db;
    clip.gain_db = if proposed_gain_db.is_finite() {
        proposed_gain_db.clamp(-90.0, 24.0)
    } else {
        0.0
    };
    let applied_gain_db = clip.gain_db;
    session.settings.ai_history.push(AiChangeSet {
        id: format!("ai:{}", now_ms()),
        created_at_ms: now_ms(),
        permission: session.settings.ai_permission,
        target: clip_id.to_owned(),
        current_gain_db,
        proposed_gain_db: applied_gain_db,
        reason: "Match the selected reference RMS without changing the source WAV.".into(),
        expected_effect:
            "A closer perceived level while clip position and source remain unchanged.".into(),
        risk: "Low · reversible".into(),
        context: session.settings.ai_context.clone(),
        applied: true,
    });
    if session.settings.ai_history.len() > 128 {
        let excess = session.settings.ai_history.len() - 128;
        session.settings.ai_history.drain(..excess);
    }
    commit_session(context, session)
}

// Audio + Session coupling operations.
//
// `set_master_gain_db` changes an Audio Runtime setting and a session preference
// at the same time. Audio-device preferences are application settings and live
// outside the CreativeSession.

/// Sets the master gain on the Audio Runtime and persists the clamped value in
/// the session settings so a reload reproduces the same loudness.
pub fn set_master_gain_db(
    context: &SessionContext<'_>,
    gain_db: f64,
) -> Result<SessionAudioPair, String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    let audio = context.audio.set_master_gain_db(gain_db)?;
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session.settings.master_db = gain_db.clamp(-90.0, 0.0);
    let committed = commit_session(context, session)?;
    Ok(SessionAudioPair {
        session: committed,
        audio,
    })
}

// Missing-dependency recovery operations.
//
// Relink and disable both mutate the canonical session (asset references or
// the rack's disabled-placeholder flag) and persist through the canonical
// commit. The Asset layer's `content_location` is rewritten when relinking so
// the canonical row follows the user's new file.

/// Rewrites every canonical Asset reference pointed to by `asset_id` to the
/// user's new file and persists the updated session. The Asset's
/// `content_location` is also updated so future operations resolve to the new
/// path.
pub fn relink_missing_dependency(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    new_path: &str,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session = crate::missing::relink(context.data_root, &session, &asset_id, new_path)?;
    commit_session(context, session)
}

/// Marks a missing plugin device as a disabled placeholder so it no longer
/// surfaces as a missing dependency. The session is persisted through the
/// canonical commit.
pub fn disable_missing_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
) -> Result<CreativeSession, String> {
    let session = context.session.lock().map_err(lock_error)?.clone();
    let candidate = crate::missing::mark_disabled_placeholder(&session, device_id);
    if candidate == session {
        return Err(format!(
            "Missing Plugin Device is not registered: {device_id}"
        ));
    }
    if candidate.arrangement.revision != session.arrangement.revision {
        commit_structural_arrangement(context, candidate)
    } else {
        commit_session(context, candidate)
    }
}

/// Replaces an unresolved Track Device in place so its chain position and id
/// remain stable while the plugin binary and plugin state are refreshed.
pub fn replace_missing_track_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
    new_path: &str,
) -> Result<CreativeSession, String> {
    let path = Path::new(new_path.trim());
    if !path.exists() {
        return Err("Replacement VST3 path does not exist.".into());
    }
    let previous = context.session_actor.capture_projection(context.session)?;
    let mut session = previous.session;
    let device = session
        .arrangement
        .tracks
        .iter_mut()
        .find_map(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    *device = plugin_device(&path.to_string_lossy(), device_id.to_owned())?;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_plugin_arrangement(context, session, previous.sequence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::RecordingTakeRecord;
    use crate::session::actor::SessionActor;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    struct BarrierCommitDriver {
        commit_started: Arc<Barrier>,
        release_commit: Arc<Barrier>,
        commit_gate_used: AtomicBool,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        generation: AtomicU64,
    }

    impl BarrierCommitDriver {
        fn new() -> Self {
            Self {
                commit_started: Arc::new(Barrier::new(2)),
                release_commit: Arc::new(Barrier::new(2)),
                commit_gate_used: AtomicBool::new(false),
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                generation: AtomicU64::new(1),
            }
        }
    }

    impl crate::runtime::ports::ProjectionDriver for BarrierCommitDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if self
                .commit_gate_used
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.commit_started.wait();
                self.release_commit.wait();
            }
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                crate::runtime::error::RuntimeError::NativeRejected(
                    "No prepared timeline snapshot is available.".into(),
                )
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            self.pending.lock().unwrap().take();
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl crate::runtime::ports::TransportDriver for BarrierCommitDriver {
        fn play_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            self.stop_timeline()
        }
    }

    struct CandidateRuntimeDriver {
        fail_prepare: AtomicBool,
        generation: AtomicU64,
        pending: Mutex<Option<u64>>,
        loaded: Mutex<Vec<u64>>,
        commit_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl CandidateRuntimeDriver {
        fn new(fail_prepare: bool) -> Self {
            Self {
                fail_prepare: AtomicBool::new(fail_prepare),
                generation: AtomicU64::new(1),
                pending: Mutex::new(None),
                loaded: Mutex::new(Vec::new()),
                commit_hook: Mutex::new(None),
            }
        }

        fn set_commit_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
            *self.commit_hook.lock().unwrap() = Some(hook);
        }
    }

    impl crate::runtime::ports::ProjectionDriver for CandidateRuntimeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if self.fail_prepare.swap(false, Ordering::AcqRel) {
                return Err(crate::runtime::error::RuntimeError::NativeRejected(
                    "Candidate graph was rejected.".into(),
                ));
            }
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            if let Some(hook) = self.commit_hook.lock().unwrap().take() {
                hook();
            }
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                crate::runtime::error::RuntimeError::NativeRejected(
                    "No prepared timeline snapshot is available.".into(),
                )
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(
            &self,
            _timeout: Duration,
        ) -> Result<(), crate::runtime::error::RuntimeError> {
            self.pending.lock().unwrap().take();
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    impl crate::runtime::ports::TransportDriver for CandidateRuntimeDriver {
        fn play_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), crate::runtime::error::RuntimeError> {
            self.stop_timeline()
        }
    }

    fn plugin_candidate_session() -> CreativeSession {
        let mut candidate = CreativeSession::new(1);
        let mut track = Track::audio("track:plugin".into(), "Plugin Track".into());
        track.rack.devices.push(RackDevice {
            id: "device:candidate".into(),
            name: "Candidate".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\plugins\Candidate.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        candidate.arrangement.tracks.push(track);
        candidate.arrangement.revision = 1;
        candidate
    }

    fn candidate_context<'a>(
        root: &'a Path,
        session: &'a Mutex<CreativeSession>,
        runtime: &'a crate::runtime::RuntimeReconciler<CandidateRuntimeDriver>,
        actor: &'a SessionActor,
        audio: &'a crate::native_audio::AudioSupervisor,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        SessionContext {
            audio,
            runtime,
            session_actor: actor,
            data_root: root,
            session,
            safe_mode: false,
        }
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate() {
                return;
            }
            if Instant::now() >= deadline {
                panic!("condition was not met within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn rejected_plugin_candidate_restores_the_canonical_runtime() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-rejected-{}",
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = Arc::new(Mutex::new(CreativeSession::new(1)));
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let actor = SessionActor::default();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let context = candidate_context(&root, &session, &runtime, &actor, &audio);

        // Act
        let result = commit_plugin_arrangement(&context, plugin_candidate_session(), 0);

        // Assert
        assert!(result.is_err());
        assert!(session.lock().unwrap().arrangement.tracks.is_empty());
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejected_plugin_candidate_is_not_requeued_after_runtime_restart() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-restart-{}",
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = Arc::new(Mutex::new(CreativeSession::new(1)));
        let driver = Arc::new(CandidateRuntimeDriver::new(true));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let actor = SessionActor::default();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let context = candidate_context(&root, &session, &runtime, &actor, &audio);
        assert!(commit_plugin_arrangement(&context, plugin_candidate_session(), 0).is_err());
        let loaded_before_restart = driver.loaded.lock().unwrap().len();
        driver.generation.store(2, Ordering::Release);

        // Act
        let requeued = runtime.requeue_after_runtime_restart(2);

        // Assert
        assert!(requeued);
        wait_until(Duration::from_secs(1), || {
            driver.loaded.lock().unwrap().len() > loaded_before_restart
        });
        assert_eq!(driver.loaded.lock().unwrap().last(), Some(&0));
        assert!(!driver.loaded.lock().unwrap().contains(&1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn candidate_sequence_conflict_restores_the_newer_canonical_session() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-conflict-{}",
            crate::storage::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session = Arc::new(Mutex::new(CreativeSession::new(1)));
        let actor = Arc::new(SessionActor::default());
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let hook_session = Arc::clone(&session);
        let hook_actor = Arc::clone(&actor);
        driver.set_commit_hook(Arc::new(move || {
            let _operation = hook_actor.enter().unwrap();
            hook_actor.begin_commit();
            hook_session.lock().unwrap().arrangement.revision = 7;
            hook_actor.mark_committed();
        }));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let context = candidate_context(&root, &session, &runtime, &actor, &audio);

        // Act
        let result = commit_plugin_arrangement(&context, plugin_candidate_session(), 0);

        // Assert
        assert!(result.is_err());
        let current = session.lock().unwrap();
        assert_eq!(current.arrangement.revision, 7);
        assert!(current.arrangement.tracks.is_empty());
        assert_eq!(driver.loaded.lock().unwrap().last(), Some(&7));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_persistence_failure_restores_the_previous_graph() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-candidate-persistence-{}",
            crate::storage::now_ms()
        ));
        std::fs::write(&root, b"not a directory").unwrap();
        let session = Arc::new(Mutex::new(CreativeSession::new(1)));
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let actor = SessionActor::default();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let context = candidate_context(&root, &session, &runtime, &actor, &audio);

        // Act
        let result = commit_plugin_arrangement(&context, plugin_candidate_session(), 0);

        // Assert
        assert!(result.is_err());
        assert!(session.lock().unwrap().arrangement.tracks.is_empty());
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), [1, 0]);
        let _ = std::fs::remove_file(&root);
    }

    #[test]
    fn update_track_returns_while_runtime_commit_is_blocked() {
        // Arrange
        let root =
            std::env::temp_dir().join(format!("riffra-barrier-{}", crate::storage::now_ms()));
        let session = Arc::new(Mutex::new({
            let mut session = CreativeSession::new(1);
            session
                .arrangement
                .tracks
                .push(Track::audio("track:a".into(), "Audio".into()));
            session
                .arrangement
                .tracks
                .push(Track::audio("track:b".into(), "Audio".into()));
            session
        }));
        let store = crate::storage::SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let driver = Arc::new(BarrierCommitDriver::new());
        let runtime =
            Arc::new(crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap());
        let actor = Arc::new(SessionActor::default());
        let audio = Arc::new(crate::native_audio::AudioSupervisor::offline("test"));
        let context = SessionContext {
            audio: audio.as_ref(),
            runtime: runtime.as_ref(),
            session_actor: actor.as_ref(),
            data_root: &root,
            session: session.as_ref(),
            safe_mode: false,
        };

        update_track(
            &context,
            "track:a",
            TrackPatch {
                muted: Some(true),
                ..Default::default()
            },
        )
        .expect("initial update_track must succeed");
        driver.commit_started.wait();

        let (update_result_tx, update_result_rx) = mpsc::channel();
        let update_context = {
            let session = Arc::clone(&session);
            let runtime = Arc::clone(&runtime);
            let actor = Arc::clone(&actor);
            let audio = Arc::clone(&audio);
            let root = root.clone();
            thread::spawn(move || {
                let context = SessionContext {
                    audio: audio.as_ref(),
                    runtime: runtime.as_ref(),
                    session_actor: actor.as_ref(),
                    data_root: &root,
                    session: session.as_ref(),
                    safe_mode: false,
                };
                let result = update_track(
                    &context,
                    "track:b",
                    TrackPatch {
                        muted: Some(true),
                        ..Default::default()
                    },
                );
                update_result_tx.send(result).unwrap();
            })
        };
        let update_result = update_result_rx.recv_timeout(Duration::from_secs(1));

        // Act
        driver.release_commit.wait();
        update_context.join().unwrap();

        // Assert
        update_result
            .expect("update_track must return while commit is blocked")
            .expect("update_track must succeed while commit is blocked");
        let expected_revision = session.lock().unwrap().arrangement.revision;
        wait_until(Duration::from_secs(1), || {
            driver.loaded.lock().unwrap().last() == Some(&expected_revision)
        });
        assert_eq!(
            driver.loaded.lock().unwrap().last(),
            Some(&expected_revision)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_editor_state_survives_canonical_session_round_trip() {
        let mut session = CreativeSession::new(1);
        let mut track = Track::audio("track:guitar".into(), "Guitar".into());
        track.rack.devices.push(RackDevice {
            id: "device:amp".into(),
            name: "Amp".into(),
            kind: DeviceKind::Plugin,
            path: Some(r"C:\plugins\Amp.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        session.arrangement.tracks.push(track);

        apply_track_plugin_state(
            &mut session,
            "track:guitar",
            "device:amp",
            vec![0.25, 0.75],
            Some("opaque-state".into()),
            true,
        )
        .unwrap();
        let restored =
            crate::session::deserialize_session(&serde_json::to_vec(&session).unwrap()).unwrap();
        let device = &restored.arrangement.tracks[0].rack.devices[0];
        assert_eq!(device.parameter_values, [0.25, 0.75]);
        assert_eq!(device.state_data.as_deref(), Some("opaque-state"));
        assert!(device.bypassed);
    }

    #[test]
    fn take_variant_is_applied_only_to_the_selected_clip() {
        let raw_id = asset::mint_asset_id();
        let processed_id = asset::mint_asset_id();
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        session.arrangement.takes.push(RecordingTakeRecord {
            id: "take:1".into(),
            session_id: "recording:1".into(),
            pass_id: "pass:1".into(),
            track_id: "track:audio".into(),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            source_start_sample: 0,
            source_end_sample: 1_000,
            raw_audio: Some(TakeAudioSource {
                asset_id: raw_id.clone(),
                source_start_sample: 0,
                source_end_sample: 1_000,
                tail_end_sample: 1_000,
                sample_rate: 48_000,
            }),
            processed_audio: Some(TakeAudioSource {
                asset_id: processed_id.clone(),
                source_start_sample: 128,
                source_end_sample: 1_256,
                tail_end_sample: 1_256,
                sample_rate: 48_000,
            }),
            raw_audio_asset_id: None,
            processed_audio_asset_id: None,
            midi_asset_id: None,
        });
        for id in ["clip:a", "clip:b"] {
            let mut clip = AudioClip::full_source(
                id.into(),
                id.into(),
                "track:audio".into(),
                raw_id.clone(),
                TimelineTick(0),
                48_000,
                1_000,
            );
            clip.recording_take_id = Some("take:1".into());
            session.arrangement.audio_clips.push(clip);
        }

        apply_audio_clip_take_variant(&mut session, "clip:a", AudioTakeVariant::Processed).unwrap();

        let selected = &session.arrangement.audio_clips[0];
        let untouched = &session.arrangement.audio_clips[1];
        assert_eq!(selected.asset_id, processed_id);
        assert_eq!(
            selected.source_range,
            crate::session::FrameRange {
                start: 128,
                end: 1_128
            }
        );
        assert_eq!(selected.timeline_duration.frames, 1_000);
        assert_eq!(untouched.asset_id, raw_id);
        assert_eq!(untouched.take_variant, AudioTakeVariant::Raw);
    }

    #[test]
    fn applying_partial_and_full_take_sources_updates_length_and_clamps_fades() {
        let mut clip = AudioClip::full_source(
            "clip:1".into(),
            "Take".into(),
            "track:audio".into(),
            asset::mint_asset_id(),
            TimelineTick(960),
            48_000,
            48_000,
        );
        clip.fade_in.frames = 24_000;
        clip.fade_out.frames = 24_000;
        let clip_id = clip.id.clone();
        let start_tick = clip.start_tick;

        let partial = TakeAudioSource {
            asset_id: asset::mint_asset_id(),
            source_start_sample: 4_000,
            source_end_sample: 10_000,
            tail_end_sample: 10_000,
            sample_rate: 48_000,
        };
        apply_audio_source_to_clip(&mut clip, &partial);
        assert_eq!(clip.timeline_duration.frames, 6_000);
        assert_eq!(clip.source_sample_rate, 48_000);
        assert_eq!(clip.timeline_duration.sample_rate, 48_000);
        assert_eq!(clip.fade_in.sample_rate, 48_000);
        assert_eq!(clip.fade_out.sample_rate, 48_000);
        assert_eq!(clip.fade_in.frames, 6_000);
        assert_eq!(clip.fade_out.frames, 6_000);

        let full = TakeAudioSource {
            asset_id: asset::mint_asset_id(),
            source_start_sample: 0,
            source_end_sample: 48_000,
            tail_end_sample: 48_000,
            sample_rate: 48_000,
        };
        apply_audio_source_to_clip(&mut clip, &full);
        assert_eq!(clip.timeline_duration.frames, 48_000);
        assert_eq!(clip.id, clip_id);
        assert_eq!(clip.start_tick, start_tick);
    }
}
