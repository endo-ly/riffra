//! Session Application Operations: production workflows that change the
//! canonical [`CreativeSession`] and keep it consistent with the Audio Runtime
//! and the Asset registry.
//!
//! The operations use three consistency policies:
//!
//! - Sample-pad operations ([`create_sample_pad`], [`update_sample_pad`],
//!   [`remove_sample_pad`]) touch play state, view state, the Asset
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
//! - Pure-session operations ([`import_session`] and [`restore_generation`])
//!   mutate the session and persist it without waiting for VST lifecycle work.
//!   Design navigation and workspace navigation are view state: they are
//!   returned as in-memory snapshots and send only a nonblocking desired
//!   runtime mode, so navigation never enters the durable Session commit path.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the only compensation is re-applying the previous pad set, which
//! matches the runtime's "reconfigure the whole pad set" capability.

use std::collections::HashMap;
use std::{fs, path::Path};

use crate::asset::{self, AssetId, AssetKind};
use crate::model::{AudioStatus, SessionAudioPair};
use crate::plugin_catalog;
use crate::presentation::{DesignTool, DesktopViewState, Workspace};
use crate::rack::{DeviceKind, RackDevice};
use crate::runtime::ports::RuntimeDriver;
use crate::session::{
    AudioClipMove, AudioClipPatch, AudioTakeVariant, AutomationParameter, AutomationPoint,
    CreativeSession, MidiClip, MidiClipMove, MidiClipPatch, MidiEvent, MidiEventKind,
    MidiInputRoute, MidiNote, ProjectTimebase, SamplePad, TimelineTick, Track, TrackKind,
};
use crate::storage::{SessionStore, now_ms};

pub(crate) use crate::session::commit::{
    commit_core_application, commit_recording_session, import_session, restore_generation,
};
pub(crate) use crate::session::context::{SessionContext, current_session, lock_error};
pub(crate) use crate::session::transport::{
    SamplePadRestoreOutcome, audio_command_succeeded, go_to_start_timeline, play_timeline,
    prepare_arrangement_candidate, resolve_native_pads, restore_sample_pads,
    runtime_snapshot_for_recording, seek_timeline, stop_timeline, switch_workspace,
    sync_arrangement, sync_arrangement_runtime,
};
pub use riffra_core::application::{
    MarkerPatch, MidiNotePatch, MidiNoteUpdate, SamplePadPatch, SessionSettingsPatch,
};
pub use riffra_core::session::TrackPatch;

/// Rebuilds every Runtime that depends on the active audio device after the
/// Native device has been reopened. The canonical Session remains unchanged;
/// only the device-dependent Sample Pad buffers and Arrangement projection are
/// prepared again.
pub(crate) fn reconcile_runtime_after_audio_device_change(
    context: &SessionContext<'_>,
) -> Result<AudioStatus, String> {
    context
        .audio
        .mark_runtime_recovery_mute()
        .map_err(|error| format!("Runtime recovery mute could not be recorded: {error}"))?;
    if !context.runtime.invalidate_for_audio_device_change() {
        return Err(
            "Audio Runtime graph is busy; the audio device change can be retried shortly.".into(),
        );
    }
    let (pad_warning, pad_error) = match restore_sample_pads(context) {
        Ok(SamplePadRestoreOutcome::Restored(_)) => (None, None),
        Ok(SamplePadRestoreOutcome::Disabled { warning, .. }) => (Some(warning), None),
        Err(error) => (
            None,
            Some(format!(
                "Sample Pad restoration failed after the audio device change: {error}"
            )),
        ),
    };
    let arrangement_error = sync_arrangement_runtime(context).err().map(|error| {
        format!("Arrangement Runtime restoration failed after the audio device change: {error}")
    });
    let mut status = context.audio.refresh_status().map_err(String::from)?;
    let errors = [pad_error, arrangement_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if let Some(warning) = pad_warning {
        status.message = if status.message.is_empty() {
            warning
        } else {
            format!("{} {warning}", status.message)
        };
    }
    Ok(status)
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

    let previous_session = current_session(context)?;
    let midi_key = (36_u8..=127)
        .find(|key| {
            !previous_session
                .play_state
                .sample_instrument
                .pads
                .iter()
                .any(|pad| pad.midi_key == *key)
        })
        .ok_or_else(|| "The sample instrument is full; no MIDI key is available.".to_string())?;
    let pad = SamplePad {
        id: format!("pad:{}", asset_id.as_str()),
        name,
        asset_id: asset_id.clone(),
        start_ms: 0,
        end_ms: duration_ms,
        midi_key,
        gain_db: 0.0,
        loop_enabled: false,
    };
    let proposed_pads = {
        let store = SessionStore::new(context.data_root);
        context
            .core
            .application(&store)
            .prepare_sample_pad_add(&pad)
            .map_err(|error| error.to_string())?
    };

    let result = commit_pad_set(context, previous_session.clone(), proposed_pads, || {
        commit_core_application(context, |core, store| {
            core.application(store).add_sample_pad(pad)
        })
    })?;
    if result.session != previous_session {
        let mut view_state = context.view_state.lock().map_err(lock_error)?;
        view_state.workspace = Workspace::Design;
        view_state.design_context.active_tool = DesignTool::Sample;
        view_state.design_context.target_asset_id = Some(asset_id);
    }
    Ok(result)
}

/// Commits the new pad set after a mutation: applies it to the runtime, persists
/// the session, and rolls the runtime back if persistence fails.
fn commit_pad_set<F>(
    context: &SessionContext<'_>,
    previous_session: CreativeSession,
    proposed_pads: Vec<SamplePad>,
    operation: F,
) -> Result<SessionAudioPair, String>
where
    F: FnOnce() -> Result<CreativeSession, String>,
{
    let runtime_generation = (!context.safe_mode).then(|| context.audio.sidecar_generation());
    let runtime_status = if context.safe_mode {
        None
    } else {
        let native_pads = resolve_native_pads(context.data_root, &proposed_pads)?;
        let status = context.audio.configure_sample_pads(&native_pads)?;
        if !audio_command_succeeded(&status) {
            return Ok(SessionAudioPair {
                session: previous_session,
                audio: status,
            });
        }
        Some(status)
    };

    let committed = match operation() {
        Ok(committed) => committed,
        Err(error) => {
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
    };

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
    restore_sample_pads(context)
        .map(SamplePadRestoreOutcome::into_status)
        .map(Some)
}

/// Updates one SamplePad's slice range, gain, or loop flag through the canonical
/// clamp rules, then synchronizes the runtime and persists.
pub fn update_sample_pad(
    context: &SessionContext<'_>,
    pad_id: &str,
    patch: &SamplePadPatch,
) -> Result<SessionAudioPair, String> {
    let previous_session = current_session(context)?;
    let proposed_pads = {
        let store = SessionStore::new(context.data_root);
        context
            .core
            .application(&store)
            .prepare_sample_pad_update(pad_id, patch)
            .map_err(|error| error.to_string())?
    };
    let patch = patch.clone();
    commit_pad_set(context, previous_session, proposed_pads, || {
        commit_core_application(context, |core, store| {
            core.application(store).update_sample_pad(pad_id, patch)
        })
    })
}

/// Removes a SamplePad, then synchronizes the runtime and persists.
pub fn remove_sample_pad(
    context: &SessionContext<'_>,
    pad_id: &str,
) -> Result<SessionAudioPair, String> {
    let previous_session = current_session(context)?;
    let proposed_pads = {
        let store = SessionStore::new(context.data_root);
        context
            .core
            .application(&store)
            .prepare_sample_pad_removal(pad_id)
            .map_err(|error| error.to_string())?
    };
    commit_pad_set(context, previous_session, proposed_pads, || {
        commit_core_application(context, |core, store| {
            core.application(store).remove_sample_pad(pad_id)
        })
    })
}

// Session commit, Arrangement, and Design/Workspace operations.
//
// These mutate the canonical CreativeSession and persist it without touching
// the Audio Runtime. They share the Core Application commit boundary so the
// save path lives in one place.

pub fn update_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    patch: AudioClipPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_audio_clip(clip_id, patch)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn split_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    split_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let revision = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session
        .arrangement
        .revision;
    let committed = commit_core_application(context, |core, store| {
        core.application(store).split_audio_clip(
            clip_id,
            split_tick,
            format!("clip:split:{}:{}", now_ms(), revision.saturating_add(1)),
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
) -> Result<CreativeSession, String> {
    let revision = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session
        .arrangement
        .revision;
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_audio_clip(
            clip_id,
            format!("clip:duplicate:{}:{}", now_ms(), revision.saturating_add(1)),
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn move_audio_clips(
    context: &SessionContext<'_>,
    moves: Vec<AudioClipMove>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).move_audio_clips(moves)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    patch: MidiClipPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_midi_clip(clip_id, patch)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn move_midi_clips(
    context: &SessionContext<'_>,
    moves: Vec<MidiClipMove>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).move_midi_clips(moves)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn trim_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    duration_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .trim_midi_clip(clip_id, start_tick, duration_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn split_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    split_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let revision = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session
        .arrangement
        .revision;
    let committed = commit_core_application(context, |core, store| {
        core.application(store).split_midi_clip(
            clip_id,
            split_tick,
            format!(
                "midi-clip:split:{}:{}",
                now_ms(),
                revision.saturating_add(1)
            ),
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
) -> Result<CreativeSession, String> {
    let revision = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session
        .arrangement
        .revision;
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_midi_clip(
            clip_id,
            format!(
                "midi-clip:duplicate:{}:{}",
                now_ms(),
                revision.saturating_add(1)
            ),
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn crossfade_audio_clips(
    context: &SessionContext<'_>,
    first_id: &str,
    second_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .crossfade_audio_clips(first_id, second_id)
    })?;
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

pub fn update_timebase(
    context: &SessionContext<'_>,
    timebase: ProjectTimebase,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_timebase(timebase)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_loop_range(
    context: &SessionContext<'_>,
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .update_loop_range(enabled, start_tick, end_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_punch_range(
    context: &SessionContext<'_>,
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .update_punch_range(enabled, start_tick, end_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_timeline_clips(
    context: &SessionContext<'_>,
    audio_clip_ids: &[String],
    midi_clip_ids: &[String],
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .remove_timeline_clips(audio_clip_ids.to_owned(), midi_clip_ids.to_owned())
    })?;
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
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store).paste_timeline_clips(
            audio_clip_ids.to_owned(),
            midi_clip_ids.to_owned(),
            audio_ids,
            midi_ids,
            start_tick,
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn trim_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    source_range: crate::session::FrameRange,
) -> Result<CreativeSession, String> {
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store).trim_audio_clip(
            clip_id,
            start_tick,
            source_range,
            (wav.data_len / frame_bytes) as u64,
        )
    })?;
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
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let mut create_track = None;
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
            create_track = Some("Audio 1".to_owned());
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .add_audio_clip_with_track(clip, create_track, |id| {
                asset::load(context.data_root, id).is_some()
            })
    })?;
    context.view_state.lock().map_err(lock_error)?.workspace = Workspace::Arrange;
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
) -> Result<DesktopViewState, String> {
    if asset::load(context.data_root, &asset_id).is_none() {
        return Err(format!(
            "Design target is not a registered asset: {asset_id}"
        ));
    }
    let mut view_state = context.view_state.lock().map_err(lock_error)?;
    view_state.workspace = Workspace::Design;
    view_state.design_context.active_tool = tool;
    view_state.design_context.target_asset_id = Some(asset_id);
    Ok(view_state.clone())
}

pub fn update_session_settings(
    context: &SessionContext<'_>,
    patch: SessionSettingsPatch,
) -> Result<CreativeSession, String> {
    let metronome_changed = patch.metronome_enabled.is_some();
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_session_settings(patch)
    })?;
    if metronome_changed {
        sync_arrangement(context)?;
    }
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
    operation: impl FnOnce(CreativeSession) -> Result<CreativeSession, String>,
) -> Result<CreativeSession, String> {
    let candidate = candidate.validate_and_normalize()?;
    if let Err(error) = prepare_arrangement_candidate(context, &candidate, base_sequence) {
        return Err(repair_previous_arrangement(context, error));
    }
    let current = context.core.snapshot().map_err(|error| error.to_string())?;
    if current.sequence != base_sequence
        || current.session.arrangement.revision.saturating_add(1) != candidate.arrangement.revision
    {
        return Err(repair_previous_arrangement(
            context,
            "Canonical Session changed while the VST candidate was being prepared.".into(),
        ));
    }
    let committed = match operation(candidate) {
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_audio_input(track_id, channel_index)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn set_track_midi_input(
    context: &SessionContext<'_>,
    track_id: &str,
    route: MidiInputRoute,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_midi_input(track_id, route)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
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
    let previous = context.core.snapshot().map_err(|error| error.to_string())?;
    let revision = previous.session.arrangement.revision;
    let track = previous
        .session
        .arrangement
        .tracks
        .iter()
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
    let mut device = plugin_device(&validated_path.to_string_lossy(), id)?;
    device.name = name;
    let mut candidate = previous.session.clone();
    candidate
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?
        .instrument = Some(device.clone());
    candidate.arrangement.revision = revision.saturating_add(1);
    commit_plugin_arrangement(context, candidate, previous.sequence, |_candidate| {
        commit_core_application(context, |core, store| {
            core.application(store).set_track_instrument_at_sequence(
                track_id,
                Some(device),
                previous.sequence,
            )
        })
    })
}

pub fn clear_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).set_track_instrument(track_id, None)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
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
    let previous = context.core.snapshot().map_err(|error| error.to_string())?;
    let revision = previous.session.arrangement.revision;
    previous
        .session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?;
    let id = format!("device:effect:{}:{}", now_ms(), revision);
    let mut device = plugin_device(&validated_path.to_string_lossy(), id)?;
    device.name = name;
    let mut candidate = previous.session.clone();
    candidate
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| format!("Track is not registered: {track_id}"))?
        .rack
        .devices
        .push(device.clone());
    candidate.arrangement.revision = revision.saturating_add(1);
    commit_plugin_arrangement(context, candidate, previous.sequence, |_candidate| {
        commit_core_application(context, |core, store| {
            core.application(store).add_track_effect_at_sequence(
                track_id,
                device,
                previous.sequence,
            )
        })
    })
}

pub fn remove_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .remove_track_effect(track_id, device_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn reorder_track_effects(
    context: &SessionContext<'_>,
    track_id: &str,
    ordered_device_ids: &[String],
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .reorder_track_effects(track_id, ordered_device_ids.to_owned())
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn set_track_device_bypassed(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    bypassed: bool,
) -> Result<CreativeSession, String> {
    let session = current_session(context)?;
    let device = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| {
            track
                .instrument
                .as_ref()
                .filter(|device| device.id == device_id)
                .or_else(|| {
                    track
                        .rack
                        .devices
                        .iter()
                        .find(|device| device.id == device_id)
                })
        })
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    let previous = device.bypassed;
    context
        .audio
        .set_track_device_bypassed(track_id, device_id, bypassed)?;
    let result = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_device_bypassed(track_id, device_id, bypassed)
    });
    match result {
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
    let session = current_session(context)?;
    let device = session
        .arrangement
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .and_then(|track| {
            track
                .instrument
                .as_ref()
                .filter(|device| device.id == device_id)
                .or_else(|| {
                    track
                        .rack
                        .devices
                        .iter()
                        .find(|device| device.id == device_id)
                })
        })
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))?;
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Device parameter index is invalid.".to_string())?;
    let previous = device.parameter_values.get(index).copied().unwrap_or(0.0);
    context
        .audio
        .set_track_device_parameter(track_id, device_id, parameter_index, value)?;
    let result = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_device_parameter(track_id, device_id, index, value)
    });
    match result {
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
    let session = current_session(context)?;
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
    commit_core_application(context, |core, store| {
        core.application(store).persist_track_plugin_state(
            track_id,
            device_id,
            parameter_values,
            state_data,
            bypassed,
        )
    })
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
    commit_core_application(context, |core, store| {
        core.application(store)
            .persist_track_plugin_parameter(track_id, device_id, index, value)
    })
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store).add_track(name, kind)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_track<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    patch: TrackPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_track(track_id, patch)
    })?;
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
    points: Vec<AutomationPoint>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_automation(track_id, parameter, points)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Removes a Track and its Clips without deleting any referenced Asset.
pub fn remove_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).remove_track(track_id)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Duplicates a Track and its non-destructive Clip references.
pub fn duplicate_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_track(track_id)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Moves a Track to a zero-based position while preserving Clip ownership.
pub fn reorder_track(
    context: &SessionContext<'_>,
    track_id: &str,
    target_index: usize,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .reorder_track(track_id, target_index)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

// Marker operations. Markers are timeline authoring metadata with no audio
// runtime impact, so they skip the audio sync and go straight through Core.

pub fn add_marker(
    context: &SessionContext<'_>,
    tick: TimelineTick,
    name: String,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store).add_marker(tick, name)
    })
}

pub fn update_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
    name: Option<String>,
    tick: Option<TimelineTick>,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .update_marker(marker_id, MarkerPatch { name, tick })
    })
}

pub fn remove_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store).remove_marker(marker_id)
    })
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store).add_midi_note(
            clip_id,
            start_tick,
            pitch,
            duration_ticks,
            velocity,
            channel,
        )
    })?;
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
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_midi_notes(clip_id, updates)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).remove_midi_note(clip_id, note_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn quantize_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    grid_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .quantize_midi_notes(clip_id, note_ids.to_owned(), grid_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    offset_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .duplicate_midi_notes(clip_id, note_ids.to_owned(), offset_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn set_audio_clip_take_variant(
    context: &SessionContext<'_>,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_audio_clip_take_variant(clip_id, variant)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn start_take_comparison(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<AudioStatus, String> {
    let session = current_session(context)?;
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
    let session = current_session(context)?;
    let target_take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.session_id == session_id && take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let midi_clip = target_take
        .midi_asset_id
        .is_some()
        .then(|| {
            crate::recording::application::midi_clip_for_take(
                context.data_root,
                &target_take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .activate_take(session_id, take_id, midi_clip)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn place_take_as_separate_clip(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<CreativeSession, String> {
    let session = current_session(context)?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let midi_clip = take
        .midi_asset_id
        .is_some()
        .then(|| {
            crate::recording::application::midi_clip_for_take(
                context.data_root,
                &take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .place_take_as_separate_clip(take_id, midi_clip)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn apply_ai_suggestion(
    context: &SessionContext<'_>,
    clip_id: &str,
    proposed_gain_db: f64,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .apply_ai_suggestion(clip_id, proposed_gain_db)
    })
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
    let previous_gain_db = current_session(context)?.settings.master_db;
    let audio = context.audio.set_master_gain_db(gain_db)?;
    let committed = match commit_core_application(context, |core, store| {
        core.application(store)
            .update_session_settings(SessionSettingsPatch {
                master_db: Some(gain_db),
                ..SessionSettingsPatch::default()
            })
    }) {
        Ok(committed) => committed,
        Err(error) => {
            let _ = context.audio.set_master_gain_db(previous_gain_db);
            return Err(error);
        }
    };
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
    let session = current_session(context)?;
    if !session
        .arrangement
        .audio_clips
        .iter()
        .any(|clip| clip.asset_id == asset_id)
        && !session
            .play_state
            .sample_instrument
            .pads
            .iter()
            .any(|pad| pad.asset_id == asset_id)
    {
        return Err(format!(
            "Asset is not referenced by the project: {asset_id}"
        ));
    }
    let name = Path::new(new_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio");
    let new_asset_id = asset::register(
        context.data_root,
        AssetKind::Audio,
        name,
        new_path,
        Some(crate::asset::Provenance::imported()),
    )?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .replace_asset_references(&asset_id, new_asset_id)
    })
}

/// Marks a missing plugin device as a disabled placeholder so it no longer
/// surfaces as a missing dependency. The session is persisted through the
/// canonical commit.
pub fn disable_missing_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).disable_missing_plugin(device_id)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
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
    let previous = context.core.snapshot().map_err(|error| error.to_string())?;
    if !previous.session.arrangement.tracks.iter().any(|track| {
        track
            .instrument
            .as_ref()
            .is_some_and(|device| device.id == device_id)
            || track
                .rack
                .devices
                .iter()
                .any(|device| device.id == device_id)
    }) {
        return Err(format!("Track Device is not registered: {device_id}"));
    }
    let replacement = plugin_device(&path.to_string_lossy(), device_id.to_owned())?;
    let mut candidate = previous.session.clone();
    *candidate
        .arrangement
        .tracks
        .iter_mut()
        .find_map(|track| track_device_mut(track, device_id))
        .ok_or_else(|| format!("Track Device is not registered: {device_id}"))? =
        replacement.clone();
    candidate.arrangement.revision = previous.session.arrangement.revision.saturating_add(1);
    commit_plugin_arrangement(context, candidate, previous.sequence, |_candidate| {
        commit_core_application(context, |core, store| {
            core.application(store).replace_track_plugin_at_sequence(
                device_id,
                replacement,
                previous.sequence,
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{AudioClip, RecordingPassRecord, RecordingTakeRecord, TakeAudioSource};
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier, Mutex, OnceLock, mpsc};
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

    fn test_view_state() -> &'static Mutex<crate::presentation::DesktopViewState> {
        static VIEW_STATE: OnceLock<Mutex<crate::presentation::DesktopViewState>> = OnceLock::new();
        VIEW_STATE.get_or_init(|| Mutex::new(Default::default()))
    }

    fn candidate_context<'a>(
        root: &'a Path,
        runtime: &'a crate::runtime::RuntimeReconciler<CandidateRuntimeDriver>,
        audio: &'a crate::native_audio::AudioSupervisor,
        core: &'a riffra_core::AppCore<crate::native_audio::AudioSupervisor>,
    ) -> SessionContext<'a, CandidateRuntimeDriver> {
        SessionContext {
            core,
            view_state: test_view_state(),
            audio,
            runtime,
            data_root: root,
            safe_mode: false,
        }
    }

    fn persist_candidate<D: RuntimeDriver>(
        context: &SessionContext<'_, D>,
        candidate: CreativeSession,
    ) -> Result<CreativeSession, String> {
        commit_core_application(context, |core, store| {
            core.application(store).import_project(candidate)
        })
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
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::from_shared_session(
            root.clone(),
            Arc::clone(&session),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let result =
            commit_plugin_arrangement(&context, plugin_candidate_session(), 0, |candidate| {
                persist_candidate(&context, candidate)
            });

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
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::from_shared_session(
            root.clone(),
            Arc::clone(&session),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context(&root, &runtime, &audio, &core);
        assert!(
            commit_plugin_arrangement(&context, plugin_candidate_session(), 0, |candidate| {
                persist_candidate(&context, candidate)
            })
            .is_err()
        );
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
        let driver = Arc::new(CandidateRuntimeDriver::new(false));
        let hook_session = Arc::clone(&session);
        driver.set_commit_hook(Arc::new(move || {
            hook_session.lock().unwrap().arrangement.revision = 7;
        }));
        let runtime = crate::runtime::RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::from_shared_session(
            root.clone(),
            Arc::clone(&session),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let result =
            commit_plugin_arrangement(&context, plugin_candidate_session(), 0, |candidate| {
                persist_candidate(&context, candidate)
            });

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
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::from_shared_session(
            root.clone(),
            Arc::clone(&session),
            audio.clone(),
            false,
            false,
        );
        let context = candidate_context(&root, &runtime, &audio, &core);

        // Act
        let result =
            commit_plugin_arrangement(&context, plugin_candidate_session(), 0, |candidate| {
                persist_candidate(&context, candidate)
            });

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
        let audio = Arc::new(crate::native_audio::AudioSupervisor::offline("test"));
        let core = Arc::new(riffra_core::AppCore::from_shared_session(
            root.clone(),
            Arc::clone(&session),
            audio.as_ref().clone(),
            false,
            false,
        ));
        let context = SessionContext {
            core: core.as_ref(),
            view_state: test_view_state(),
            audio: audio.as_ref(),
            runtime: runtime.as_ref(),
            data_root: &root,
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
            let runtime = Arc::clone(&runtime);
            let audio = Arc::clone(&audio);
            let core = Arc::clone(&core);
            let root = root.clone();
            thread::spawn(move || {
                let context = SessionContext {
                    core: core.as_ref(),
                    view_state: test_view_state(),
                    audio: audio.as_ref(),
                    runtime: runtime.as_ref(),
                    data_root: &root,
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
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-state-round-trip-{}",
            crate::storage::now_ms()
        ));
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
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = crate::storage::SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let saved = core
            .application(&store)
            .persist_track_plugin_state(
                "track:guitar",
                "device:amp",
                vec![0.25, 0.75],
                Some("opaque-state".into()),
                true,
            )
            .unwrap();
        let restored =
            crate::session::deserialize_session(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let device = &restored.arrangement.tracks[0].rack.devices[0];
        assert_eq!(device.parameter_values, [0.25, 0.75]);
        assert_eq!(device.state_data.as_deref(), Some("opaque-state"));
        assert!(device.bypassed);
        let _ = std::fs::remove_dir_all(root);
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
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: "pass:1".into(),
                session_id: "recording:1".into(),
                ordinal: 1,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: vec!["take:1".into()],
            });
        session
            .arrangement
            .recording_sessions
            .push(crate::session::RecordingSessionRecord {
                id: "recording:1".into(),
                start_tick: TimelineTick(0),
                track_slots: vec![crate::session::RecordingSessionTrackSlot {
                    track_id: "track:audio".into(),
                    active_take_id: "take:1".into(),
                    timeline_clip_id: "clip:a".into(),
                }],
                pass_ids: vec!["pass:1".into()],
            });
        let root =
            std::env::temp_dir().join(format!("riffra-take-variant-{}", crate::storage::now_ms()));
        struct MemoryStorage;
        impl riffra_core::SessionStorage for MemoryStorage {
            fn save(&self, _session: &CreativeSession) -> Result<(), riffra_core::PortError> {
                Ok(())
            }
        }
        let audio = crate::native_audio::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = MemoryStorage;
        let changed = core
            .application(&store)
            .set_audio_clip_take_variant("clip:a", AudioTakeVariant::Processed)
            .unwrap();

        let selected = &changed.arrangement.audio_clips[0];
        let untouched = &changed.arrangement.audio_clips[1];
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
        let _ = std::fs::remove_dir_all(root);
    }
}
