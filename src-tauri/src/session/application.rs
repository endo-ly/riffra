//! Session Application Operations: production workflows that change the
//! canonical [`CreativeSession`] and keep it consistent with the Audio Runtime
//! and the Asset registry.
//!
//! Two families live here:
//!
//! - Sample-pad operations ([`create_sample_pad`], [`update_sample_pad`],
//!   [`remove_sample_pad`]) touch play state, design context, the Asset
//!   registry (existence check), and the Audio Runtime (pad configuration).
//!   Because the runtime and the persisted session must agree, each operation
//!   applies the new pad set to the runtime, persists the session, and restores
//!   the previous pad set when persistence fails.
//!
//! - Pure-session operations ([`commit_session`], [`apply_arrangement_edit`],
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

use std::path::Path;
use std::sync::Mutex;

use crate::asset::{self, AssetId};
use crate::errors::DomainError;
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::{AudioSupervisor, NativeSamplePad};
use crate::session::{Arrangement, CreativeSession, DesignTool, SamplePad, Workspace};
use crate::storage::{SessionStore, now_ms};

/// Concrete dependencies a Session Application Operation needs.
pub struct SessionContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("An internal state lock was poisoned: {error}")
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
    duration_ms: u64,
) -> Result<(CreativeSession, AudioStatus), String> {
    if name.trim().is_empty() {
        return Err("Sample pad name must not be empty.".into());
    }
    if duration_ms == 0 {
        return Err("Sample pad duration must be greater than zero.".into());
    }
    if asset::load(context.data_root, &asset_id).is_none() {
        return Err(format!(
            "Sample pad references an unregistered asset: {asset_id}"
        ));
    }

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
            return Ok((previous_session, status));
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
    Ok((committed, status))
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
) -> Result<(CreativeSession, AudioStatus), String> {
    let runtime_status = if context.safe_mode {
        None
    } else {
        let native_pads = resolve_native_pads(
            context.data_root,
            &session.play_state.sample_instrument.pads,
        )?;
        let status = context.audio.configure_sample_pads(&native_pads)?;
        if !audio_command_succeeded(&status) {
            return Ok((previous_session, status));
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
    Ok((committed, status))
}

/// Updates one SamplePad's slice range, gain, or loop flag through the canonical
/// clamp rules, then synchronizes the runtime and persists.
pub fn update_sample_pad(
    context: &SessionContext<'_>,
    pad_id: &str,
    patch: &SamplePadPatch,
) -> Result<(CreativeSession, AudioStatus), String> {
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
) -> Result<(CreativeSession, AudioStatus), String> {
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
    commit_session(context, session)
}

/// Adds an audio clip referencing a canonical Asset to the arrangement, then
/// commits the session and switches to the Arrange workspace.
pub fn add_audio_clip(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
    duration_ms: u64,
    track_id: Option<String>,
) -> Result<CreativeSession, String> {
    if name.trim().is_empty() {
        return Err("Audio clip name must not be empty.".into());
    }
    if duration_ms == 0 {
        return Err("Audio clip duration must be greater than zero.".into());
    }
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let track_id = track_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            session
                .arrangement
                .tracks
                .first()
                .map(|track| track.id.clone())
        })
        .ok_or_else(|| "Arrangement has no track for the new audio clip.".to_string())?;
    let position_ms = session
        .arrangement
        .audio_clips
        .iter()
        .map(|clip| clip.position_ms.saturating_add(clip.duration_ms))
        .max()
        .unwrap_or(0);
    let clip = crate::session::AudioClip {
        id: format!("clip:{}:{}", asset_id.as_str(), now_ms()),
        name,
        track_id,
        asset_id,
        position_ms,
        duration_ms,
        source_start_ms: 0,
        source_end_ms: 0,
        gain_db: 0.0,
        fade_in_ms: 0,
        fade_out_ms: 0,
        pan: 0.0,
        loop_enabled: false,
        muted: false,
    };
    session
        .arrangement
        .add_audio_clip(clip, |id| asset::load(context.data_root, id).is_some())
        .map_err(|error| error.to_string())?;
    session.workspace = Workspace::Arrange;
    commit_session(context, session)
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
