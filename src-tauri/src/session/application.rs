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
//! - Arrangement operations commit the canonical Session first, then prepare
//!   and exchange a resolved runtime Timeline Snapshot. Runtime failure never
//!   rolls back a successful save; the next explicit sync or play rebuilds the
//!   latest revision.
//!
//! - Pure-session operations ([`commit_session`],
//!   [`save_session`], [`import_session`], [`restore_generation`],
//!   [`open_asset_in_design`], [`switch_workspace`]) mutate the session and
//!   persist it without touching the Audio Runtime, so they reuse
//!   [`commit_session`] as the single validate-and-persist boundary and need no
//!   rollback compensation.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the only compensation is re-applying the previous pad set, which
//! matches the runtime's "reconfigure the whole pad set" capability.

use std::sync::Mutex;
use std::{fs, path::Path};

use crate::asset::{self, AssetId, AssetKind};
use crate::errors::DomainError;
use crate::model::{AudioState, AudioStatus, SessionAudioPair};
use crate::native_audio::{AudioSupervisor, NativeSamplePad};
use crate::session::{
    AiChangeSet, Arrangement, CreativeSession, DesignTool, SamplePad, TimelineTick, Track,
    TrackKind, Workspace,
};
use crate::storage::{SessionStore, now_ms};

/// Concrete dependencies a Session Application Operation needs.
pub struct SessionContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}

fn audio_command_succeeded(status: &AudioStatus) -> bool {
    status.state != AudioState::Faulted && status.state != AudioState::Offline
}

/// Resolves the session's pad set into the runtime's native pad shape, failing
/// on any invalid slice or unresolved asset. Shared by the create workflow and
/// the direct `configure_sample_pads` command.
pub fn resolve_native_pads(
    data_root: &Path,
    pads: &[SamplePad],
) -> Result<Vec<NativeSamplePad>, String> {
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    let mut native_pads = Vec::with_capacity(pads.len());
    for pad in pads {
        if pad.end_ms <= pad.start_ms {
            return Err(format!("Sample pad '{}' has an invalid slice.", pad.name));
        }
        let content_location = asset::resolve_content_location(data_root, &pad.asset_id)
            .ok_or_else(|| format!("Sample pad '{}' references an unresolved asset.", pad.name))?;
        native_pads.push(NativeSamplePad {
            id: pad.id.clone(),
            name: pad.name.clone(),
            asset_path: content_location,
            start_ms: pad.start_ms,
            end_ms: pad.end_ms,
            midi_key: pad.midi_key,
            gain_db: pad.gain_db,
            loop_enabled: pad.loop_enabled,
        });
    }
    Ok(native_pads)
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

    session.updated_at_ms = now_ms();
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

    crate::queue_session_index(context.data_root, &session);
    let committed = session.clone();
    *context.session.lock().map_err(lock_error)? = session;

    // In Safe Mode the runtime stayed isolated; report the current status so
    // React reflects the real (muted/offline) engine.
    let status = match runtime_status {
        Some(status) => status,
        None => context.audio.refresh_status()?,
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

    session.updated_at_ms = now_ms();
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

    crate::queue_session_index(context.data_root, &session);
    let committed = session.clone();
    *context.session.lock().map_err(lock_error)? = session;
    let status = match runtime_status {
        Some(status) => status,
        None => context.audio.refresh_status()?,
    };
    Ok(SessionAudioPair {
        session: committed,
        audio: status,
    })
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

/// Commits a mutated session through the canonical pipeline: validate +
/// normalize, persist to the SessionStore, refresh the Library index, and swap
/// the in-memory session. This is the "save" boundary for Session Application
/// Operations that do not also change the Audio Runtime.
pub fn commit_session(
    context: &SessionContext<'_>,
    mut session: CreativeSession,
) -> Result<CreativeSession, String> {
    session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(context.data_root)
        .save(&session)
        .map_err(|error| format!("Session could not be saved: {error}"))?;
    crate::queue_session_index(context.data_root, &session);
    let committed = session.clone();
    *context.session.lock().map_err(lock_error)? = session;
    Ok(committed)
}

/// Saves a caller-supplied session (the canonical save intent). The session is
/// validated and normalized before persistence.
pub fn save_session(
    context: &SessionContext<'_>,
    session: CreativeSession,
) -> Result<CreativeSession, String> {
    commit_session(context, session)
}

/// Imports a project manifest and commits the resulting session.
pub fn import_session(
    context: &SessionContext<'_>,
    path: &Path,
) -> Result<CreativeSession, String> {
    let session = crate::projects::import(context.data_root, path)?;
    commit_session(context, session)
}

/// Restores a saved recovery generation as the active session. The generation
/// file is already canonical, so it is swapped into memory without re-saving.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<CreativeSession, String> {
    let session = SessionStore::new(context.data_root)
        .restore_generation(file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    crate::queue_session_index(context.data_root, &session);
    let restored = session.clone();
    *context.session.lock().map_err(lock_error)? = session;
    Ok(restored)
}

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
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
}

fn runtime_timeline_snapshot(
    context: &SessionContext<'_>,
    session: &CreativeSession,
) -> serde_json::Value {
    let arrangement = &session.arrangement;
    let has_solo = arrangement.tracks.iter().any(|track| track.solo);
    let mut clips = Vec::with_capacity(arrangement.audio_clips.len());
    let mut unavailable_clip_ids = Vec::new();
    for clip in &arrangement.audio_clips {
        let Some(path) = asset::resolve_content_location(context.data_root, &clip.asset_id) else {
            unavailable_clip_ids.push(clip.id.clone());
            continue;
        };
        let Some(track) = arrangement
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id)
        else {
            unavailable_clip_ids.push(clip.id.clone());
            continue;
        };
        clips.push(serde_json::json!({
                "clipId": clip.id,
                "trackId": clip.track_id,
                "path": path,
                "sourceSampleRate": clip.source_sample_rate,
                "sourceStartFrame": clip.source_range.start,
                "sourceEndFrame": clip.source_range.end,
                "durationFrames": clip.timeline_duration.frames,
                "durationSampleRate": clip.timeline_duration.sample_rate,
                "startTick": clip.start_tick.0,
                "fadeInFrames": clip.fade_in.frames,
                "fadeOutFrames": clip.fade_out.frames,
                "gainDb": clip.gain_db + track.gain_db,
                "pan": (clip.pan + track.pan).clamp(-1.0, 1.0),
                "loopEnabled": clip.loop_enabled,
                "muted": clip.muted || track.muted || (has_solo && !track.solo),
        }));
    }
    serde_json::json!({
        "revision": arrangement.revision,
        "timebase": arrangement.timebase,
        "loopRange": arrangement.loop_range,
        "tracks": arrangement.tracks,
        "audioClips": clips,
        "unavailableClipIds": unavailable_clip_ids,
        "midiClips": [],
        "automation": [],
    })
}

fn sync_arrangement_best_effort(context: &SessionContext<'_>, session: &CreativeSession) {
    if let Err(error) = context
        .audio
        .load_timeline_snapshot(runtime_timeline_snapshot(context, session))
    {
        tracing::warn!(revision = session.arrangement.revision, %error, "timeline runtime sync failed");
    }
}

pub fn sync_arrangement_runtime(context: &SessionContext<'_>) -> Result<(), String> {
    let session = context.session.lock().map_err(lock_error)?.clone();
    context
        .audio
        .load_timeline_snapshot(runtime_timeline_snapshot(context, &session))
}

pub fn play_timeline(context: &SessionContext<'_>) -> Result<(), String> {
    sync_arrangement_runtime(context)?;
    context.audio.play_timeline()
}

pub fn stop_timeline(context: &SessionContext<'_>) -> Result<(), String> {
    context.audio.stop_timeline()
}

pub fn seek_timeline(context: &SessionContext<'_>, tick: TimelineTick) -> Result<(), String> {
    context.audio.seek_timeline(tick.0)
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
    sync_arrangement_best_effort(context, &committed);
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
    sync_arrangement_best_effort(context, &committed);
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

/// Switches the active workspace. This is a pure workspace change (no design
/// tool or target), persisted through the canonical commit.
pub fn switch_workspace(
    context: &SessionContext<'_>,
    workspace: Workspace,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session.workspace = workspace;
    commit_session(context, session)
}

/// Bounded patch for the session metadata and settings that are edited by the
/// current UI. Structural production state (rack, tracks, clips and pads) has
/// dedicated operations and cannot be smuggled through this patch.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettingsPatch {
    pub project_name: Option<Option<String>>,
    pub loop_enabled: Option<bool>,
    pub count_in_beats: Option<u8>,
    pub note: Option<String>,
    pub ai_permission: Option<String>,
    pub ai_context: Option<Vec<String>>,
}

pub fn update_session_settings(
    context: &SessionContext<'_>,
    patch: SessionSettingsPatch,
) -> Result<CreativeSession, String> {
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
    if let Some(note) = patch.note {
        session.settings.note = note.chars().take(16_384).collect();
    }
    if let Some(permission) = patch.ai_permission {
        if !matches!(permission.as_str(), "Explain" | "Suggest" | "Apply") {
            return Err(format!("Unsupported AI permission: {permission}"));
        }
        session.settings.ai_permission = permission;
    }
    if let Some(context_items) = patch.ai_context {
        session.settings.ai_context = context_items;
    }
    commit_session(context, session)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
}

pub fn add_track(context: &SessionContext<'_>, name: String) -> Result<CreativeSession, String> {
    let name = name.trim().chars().take(80).collect::<String>();
    if name.is_empty() {
        return Err("Track name must not be empty.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session.arrangement.tracks.push(Track {
        id: format!("track:{}", now_ms()),
        name,
        kind: TrackKind::Audio,
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
    });
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
}

pub fn update_track(
    context: &SessionContext<'_>,
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
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
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
    let committed = commit_session(context, session)?;
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
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
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = commit_session(context, session)?;
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
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
    sync_arrangement_best_effort(context, &committed);
    Ok(committed)
}

pub fn apply_ai_suggestion(
    context: &SessionContext<'_>,
    clip_id: &str,
    proposed_gain_db: f64,
) -> Result<CreativeSession, String> {
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    if session.settings.ai_permission != "Apply" {
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
        permission: session.settings.ai_permission.clone(),
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
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    session = crate::missing::mark_disabled_placeholder(&session, device_id);
    commit_session(context, session)
}
