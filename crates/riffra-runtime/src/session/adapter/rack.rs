//! Rack and plugin runtime adapters.

use super::*;

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
pub(super) fn commit_plugin_arrangement<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    prepared: riffra_core::PreparedSession,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if let Err(error) =
        prepare_arrangement_candidate(context, prepared.session(), prepared.sequence())
    {
        return Err(match error {
            AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            } => AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            },
            AdapterError::RuntimeUnavailable(message) => {
                AdapterError::runtime(repair_previous_arrangement(context, message))
            }
            AdapterError::CommandFailed(message) => {
                AdapterError::command(repair_previous_arrangement(context, message))
            }
        });
    }
    if let Err(error) = commit_core_application(context, |core, store| {
        core.application(store).commit_prepared(prepared)
    }) {
        return Err(match error {
            AdapterError::Conflict {
                expected_sequence,
                current_sequence,
            } => {
                let _ = repair_previous_arrangement(context, error.to_string());
                AdapterError::Conflict {
                    expected_sequence,
                    current_sequence,
                }
            }
            AdapterError::RuntimeUnavailable(message) => {
                AdapterError::runtime(repair_previous_arrangement(context, message))
            }
            AdapterError::CommandFailed(message) => {
                AdapterError::command(repair_previous_arrangement(context, message))
            }
        });
    }
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_audio_input(
    context: &SessionContext<'_>,
    track_id: &str,
    channel_index: Option<u32>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_audio_input(track_id, channel_index)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_midi_input(
    context: &SessionContext<'_>,
    track_id: &str,
    route: MidiInputRoute,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_midi_input(track_id, route)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    set_track_instrument_with_expected_sequence(context, track_id, path, None)
}

pub(crate) fn set_track_instrument_with_expected_sequence(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect instruments.",
        ));
    }
    let (name, validated_path) =
        plugin_catalog::validated_plugin(context.data_root, Path::new(path))?;
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_instrument(
            track_id,
            name,
            validated_path.to_string_lossy().into_owned(),
        )
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_plugin_arrangement(context, prepared)
}

pub fn clear_track_instrument(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store).set_track_instrument(track_id, None)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn add_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    add_track_effect_with_expected_sequence(context, track_id, path, None)
}

pub(crate) fn add_track_effect_with_expected_sequence(
    context: &SessionContext<'_>,
    track_id: &str,
    path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if context.safe_mode {
        return Err(AdapterError::runtime(
            "Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to connect effects.",
        ));
    }
    let (name, validated_path) =
        plugin_catalog::validated_plugin(context.data_root, Path::new(path))?;
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_effect(
            track_id,
            name,
            validated_path.to_string_lossy().into_owned(),
        )
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_plugin_arrangement(context, prepared)
}

pub fn remove_track_effect(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .remove_track_effect(track_id, device_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn reorder_track_effects(
    context: &SessionContext<'_>,
    track_id: &str,
    ordered_device_ids: &[String],
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .reorder_track_effects(track_id, ordered_device_ids.to_owned())
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn set_track_device_bypassed(
    context: &SessionContext<'_>,
    track_id: &str,
    device_id: &str,
    bypassed: bool,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
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
        Ok(_) => crate::session::adapter::arrangement_mutation_without_projection(context),
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
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
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
        Ok(_) => crate::session::adapter::arrangement_mutation_without_projection(context),
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
) -> Result<(), AdapterError> {
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
        return Err(format!("Track Device is not registered: {device_id}").into());
    }
    drop(session);
    let project_id = context
        .storage
        .project_id()
        .map_err(|error| AdapterError::runtime(error.to_string()))?;
    context
        .audio
        .open_track_plugin_editor(&project_id, track_id, device_id)
        .map_err(|error| AdapterError::runtime(error.to_string()))
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
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
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
    })?;
    crate::session::adapter::arrangement_mutation_without_projection(context)
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
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    if parameter_index < 0 || !value.is_finite() {
        return Err("Track Plugin Editor returned an invalid parameter change.".into());
    }
    let index = usize::try_from(parameter_index)
        .map_err(|_| "Track Plugin Editor returned an invalid parameter index.".to_string())?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .persist_track_plugin_parameter(track_id, device_id, index, value)
    })?;
    crate::session::adapter::arrangement_mutation_without_projection(context)
}

/// Rewrites every canonical Asset reference pointed to by `asset_id` to the
/// user's new file and persists the updated session. The Asset's
/// `content_location` is also updated so future operations resolve to the new
/// path.
pub fn relink_missing_dependency(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    new_path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let session = current_session(context)?;
    if !session
        .arrangement
        .audio_clips
        .iter()
        .any(|clip| clip.asset_id == asset_id)
    {
        return Err(format!("Asset is not referenced by the project: {asset_id}").into());
    }
    let new_path = Path::new(new_path);
    if !new_path.is_file() {
        return Err(format!("Replacement asset does not exist: {}", new_path.display()).into());
    }
    let name = new_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio");
    let new_asset_id = asset::register(
        context.data_root,
        AssetKind::Audio,
        name,
        &new_path.to_string_lossy(),
        Some(riffra_core::Provenance::imported()),
    )?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .replace_asset_references(&asset_id, new_asset_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

/// Marks a missing plugin device as a disabled placeholder so it no longer
/// surfaces as a missing dependency. The session is persisted through the
/// canonical commit.
pub fn disable_missing_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store).disable_missing_plugin(device_id)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

/// Replaces an unresolved Track Device in place so its chain position and id
/// remain stable while the plugin binary and plugin state are refreshed.
pub fn replace_missing_track_plugin(
    context: &SessionContext<'_>,
    device_id: &str,
    new_path: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    replace_missing_track_plugin_with_expected_sequence(context, device_id, new_path, None)
}

pub(crate) fn replace_missing_track_plugin_with_expected_sequence(
    context: &SessionContext<'_>,
    device_id: &str,
    new_path: &str,
    expected_sequence: Option<u64>,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    let path = Path::new(new_path.trim());
    if !path.exists() {
        return Err("Replacement VST3 path does not exist.".into());
    }
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned();
    let prepared = context
        .core
        .application(&context.storage)
        .prepare_track_plugin_replacement(device_id, name, path.to_string_lossy().into_owned())
        .map_err(AdapterError::from)?;
    let prepared = match expected_sequence {
        Some(sequence) => prepared.with_expected_sequence(sequence),
        None => prepared,
    };
    commit_plugin_arrangement(context, prepared)
}
