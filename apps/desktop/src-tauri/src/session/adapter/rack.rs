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
    candidate: CreativeSession,
    base_sequence: u64,
    operation: impl FnOnce() -> Result<CreativeSession, String>,
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
    let committed = match operation() {
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
    commit_plugin_arrangement(context, candidate, previous.sequence, || {
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
    commit_plugin_arrangement(context, candidate, previous.sequence, || {
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
    commit_plugin_arrangement(context, candidate, previous.sequence, || {
        commit_core_application(context, |core, store| {
            core.application(store).replace_track_plugin_at_sequence(
                device_id,
                replacement,
                previous.sequence,
            )
        })
    })
}
