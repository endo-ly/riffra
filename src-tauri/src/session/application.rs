//! Session Application Operations that coordinate more than one subsystem.
//!
//! The SamplePad workflow is the Block 1 member of this layer: creating a pad
//! touches the canonical [`CreativeSession`] (play state + design context), the
//! Asset registry (existence check), and the Audio Runtime (pad configuration).
//! Because success requires the runtime and the persisted session to agree, the
//! operation applies the new pad set to the runtime, persists the session, and
//! restores the previous pad set if persistence fails.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the compensation is a single re-application of the previous pad
//! set, matching the runtime's "reconfigure the whole pad set" capability.

use std::path::Path;
use std::sync::Mutex;

use crate::asset::{self, AssetId};
use crate::model::{AudioState, AudioStatus};
use crate::native_audio::{AudioSupervisor, NativeSamplePad};
use crate::session::{CreativeSession, DesignTool, SamplePad, Workspace};
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
/// Runtime configuration happens inside the operation so a normal pad change no
/// longer depends on a follow-up React `useEffect`. If persistence fails after
/// the runtime accepted the new pad set, the previous pad set is re-applied.
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
