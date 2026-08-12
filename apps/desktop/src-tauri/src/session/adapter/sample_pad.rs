//! Sample Pad runtime adapter.

use super::*;

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
