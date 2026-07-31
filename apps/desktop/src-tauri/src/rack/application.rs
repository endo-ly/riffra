//! Rack Application Operations.
//!
//! These functions own the production workflow that keeps the Audio Runtime and
//! the persisted [`CreativeSession`] rack in lock-step. A single user intent
//! (load a plugin, clear it, toggle bypass, change a parameter, restore the
//! rack at startup) is applied to the runtime first, then committed to the
//! session and persisted. When persistence fails after a successful runtime
//! apply, the runtime is rolled back to its previous plugin so the two never
//! diverge; a failed rollback is surfaced as a distinct, more severe error and
//! never reported as success.
//!
//! This layer deliberately knows nothing about `tauri::State`, Tauri `invoke`,
//! or React DTO shapes. It receives the concrete dependencies it needs
//! (`AudioSupervisor`, the data root, and the in-memory session lock) so the
//! same orchestration is exercised directly from tests. There is no DI
//! framework and no generic transaction/rollback abstraction: the compensation
//! is local to the single plugin slot the runtime actually supports.

use std::path::Path;
use std::sync::Mutex;

use crate::model::{AudioState, AudioStatus, SessionAudioPair};
use crate::native_audio::AudioSupervisor;
use crate::plugin_catalog;
use crate::rack::{DeviceKind, RackDevice};
use crate::session::actor::SessionActor;
use crate::session::{CreativeSession, SessionSnapshot};
use crate::storage::now_ms;

/// Concrete dependencies a Rack Application Operation needs. Bundling them keeps
/// the operation signatures small without pulling in `tauri::State`.
pub struct RackContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub session_actor: &'a SessionActor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}

/// Mirrors the React `audioCommandSucceeded` safety gate: a faulted or offline
/// engine must not be treated as a successful apply.
fn audio_command_succeeded(status: &AudioStatus) -> bool {
    status.state != AudioState::Faulted && status.state != AudioState::Offline
}

/// The single active (non-placeholder) plugin device in a rack, if any.
fn active_plugin_device(session: &CreativeSession) -> Option<RackDevice> {
    session
        .rack
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Plugin && !device.disabled_placeholder)
        .cloned()
}

fn runtime_plugin_matches(device: &RackDevice, status: &AudioStatus) -> bool {
    let Some(path) = device.path.as_deref() else {
        return false;
    };
    let Some(runtime_path) = status
        .plugin
        .as_ref()
        .and_then(|plugin| plugin.path.as_deref())
    else {
        return false;
    };
    status.plugin.as_ref().is_some_and(|plugin| plugin.loaded)
        && if cfg!(windows) {
            runtime_path.eq_ignore_ascii_case(path)
        } else {
            runtime_path == path
        }
}

/// Replaces any existing plugin device with a freshly described one, leaving
/// every non-plugin device untouched. This is the canonical rack projection the
/// React `rackWithPluginLoaded` helper used to perform.
fn rack_with_plugin_device(session: &mut CreativeSession, device: RackDevice) {
    session
        .rack
        .devices
        .retain(|d| d.kind != DeviceKind::Plugin);
    session.rack.devices.push(device);
}

fn merge_rack_session(
    current: &CreativeSession,
    _base: &CreativeSession,
    candidate: CreativeSession,
) -> CreativeSession {
    let mut merged = current.clone();
    merged.rack = candidate.rack;
    merged
}

fn commit_session_only(
    context: &RackContext<'_>,
    base: CreativeSession,
    session: CreativeSession,
) -> Result<SessionAudioPair, String> {
    let committed = crate::session::application::commit_merged_session(
        context.session_actor,
        context.data_root,
        context.session,
        &base,
        session,
        merge_rack_session,
    )?;
    Ok(SessionAudioPair {
        session: committed.session,
        audio: context.audio.refresh_status()?,
    })
}

/// Pushes a single plugin device's full state into the runtime, assuming any
/// previous plugin has just been cleared. Returns the final status on success.
fn apply_plugin_to_runtime(
    audio: &AudioSupervisor,
    path: &str,
    parameter_values: &[f32],
    bypassed: bool,
    state_data: Option<&str>,
) -> Result<AudioStatus, String> {
    let plugin_path = Path::new(path);
    if !plugin_path.exists()
        || plugin_path.extension().and_then(|value| value.to_str()) != Some("vst3")
    {
        return Err(format!(
            "Rack references a missing or invalid plugin bundle: {path}"
        ));
    }
    let mut status = audio.load_plugin(plugin_path, parameter_values, bypassed, state_data)?;
    if !audio_command_succeeded(&status) {
        return Ok(status);
    }
    // Fresh loads need a parameter snapshot so the canonical rack can retain
    // the plugin's defaults. Restores with saved parameters/state already have
    // their authoritative values and must not request the 2,000+ element
    // parameter list merely to replay it again.
    if parameter_values.is_empty() && !state_data.is_some_and(|value| !value.is_empty()) {
        status = audio.plugin_parameter_status()?;
    }
    Ok(status)
}

/// Restores the runtime to a previously captured plugin device, or clears it
/// when there was none. Used as local compensation when persistence fails after
/// a successful runtime apply. A failed step is surfaced so a broken rollback is
/// never silently swallowed.
fn restore_previous_plugin(
    audio: &AudioSupervisor,
    previous: Option<&RackDevice>,
) -> Result<(), String> {
    let cleared = audio.clear_plugin()?;
    if !audio_command_succeeded(&cleared) {
        return Err("Runtime could not clear the plugin during rollback.".into());
    }
    let Some(device) = previous else {
        return Ok(());
    };
    let Some(path) = device.path.as_deref() else {
        return Ok(());
    };
    let status = apply_plugin_to_runtime(
        audio,
        path,
        &device.parameter_values,
        device.bypassed,
        device.state_data.as_deref(),
    )?;
    if !audio_command_succeeded(&status) {
        return Err(format!(
            "Runtime could not reload the previous plugin '{}' during rollback.",
            device.name
        ));
    }
    Ok(())
}

/// Commits a runtime change into the session and persists it, rolling the
/// runtime back to `previous_plugin` if persistence fails.
fn commit_with_rollback(
    context: &RackContext<'_>,
    base: CreativeSession,
    session: CreativeSession,
    previous_plugin: Option<RackDevice>,
    runtime_status: AudioStatus,
) -> Result<SessionAudioPair, String> {
    match crate::session::application::commit_merged_session(
        context.session_actor,
        context.data_root,
        context.session,
        &base,
        session,
        merge_rack_session,
    ) {
        Ok(committed) => Ok(SessionAudioPair {
            session: committed.session,
            audio: runtime_status,
        }),
        Err(error) => match restore_previous_plugin(context.audio, previous_plugin.as_ref()) {
            Ok(()) => Err(format!(
                "The rack change was applied to the runtime but the session could not be \
                     saved; the previous rack was restored. Persistence error: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "The rack change was applied to the runtime but the session could not be \
                     saved, and runtime rollback also failed ({rollback_error}). The runtime \
                     may be out of sync with the persisted session. Persistence error: {error}"
            )),
        },
    }
}

/// Loads a plugin into the rack: applies it to the runtime, projects it into the
/// session rack, and persists. Replaces any existing plugin device.
pub fn load_plugin_into_rack(
    context: &RackContext<'_>,
    path: &str,
    parameter_values: &[f32],
    bypassed: bool,
    state_data: Option<&str>,
) -> Result<SessionAudioPair, String> {
    if context.safe_mode {
        return Err("Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to reconnect external plugins.".into());
    }
    let (name, validated_path) =
        plugin_catalog::validated_plugin(context.data_root, Path::new(path))?;
    let validated_path = validated_path.to_string_lossy().into_owned();
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin = active_plugin_device(&previous_session);

    let status = apply_plugin_to_runtime(
        context.audio,
        &validated_path,
        parameter_values,
        bypassed,
        state_data,
    )?;
    if !audio_command_succeeded(&status) {
        // The runtime rejected the new plugin. Leave the session untouched and
        // report the faulted status so React does not project a phantom device.
        return Ok(SessionAudioPair {
            session: previous_session,
            audio: status,
        });
    }

    let mut session = previous_session.clone();
    let device_params = status
        .plugin
        .as_ref()
        .and_then(|plugin| {
            // A configured restore intentionally returns the lightweight
            // runtime status without asking the sidecar to serialize every
            // parameter again. Keep the caller's canonical values in that
            // case; a fresh load below still supplies the discovered defaults.
            (!plugin.parameters.is_empty()).then(|| {
                plugin
                    .parameters
                    .iter()
                    .map(|p| p.value)
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_else(|| parameter_values.to_vec());
    rack_with_plugin_device(
        &mut session,
        RackDevice {
            id: format!("plugin:{validated_path}"),
            name,
            kind: DeviceKind::Plugin,
            path: Some(validated_path),
            bypassed,
            gain_db: 0.0,
            parameter_values: device_params,
            state_data: status
                .plugin
                .as_ref()
                .and_then(|plugin| plugin.state_data.clone())
                .or_else(|| state_data.map(|value| value.to_owned())),
            disabled_placeholder: false,
        },
    );
    commit_with_rollback(context, previous_session, session, previous_plugin, status)
}

/// Clears the plugin from the rack: clears the runtime, removes the plugin
/// device from the session, and persists.
pub fn clear_plugin_from_rack(context: &RackContext<'_>) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin = active_plugin_device(&previous_session);
    let status = context.audio.clear_plugin()?;
    if !audio_command_succeeded(&status) {
        return Ok(SessionAudioPair {
            session: previous_session,
            audio: status,
        });
    }
    let mut session = previous_session.clone();
    session
        .rack
        .devices
        .retain(|d| d.kind != DeviceKind::Plugin);
    commit_with_rollback(context, previous_session, session, previous_plugin, status)
}

/// Opens the native editor belonging to the active runtime plugin instance.
pub fn open_plugin_editor(context: &RackContext<'_>) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err("Safe Mode blocks external VST3 editors.".into());
    }
    context.audio.open_plugin_editor()
}

/// Sets the bypass flag on the rack plugin device, applying it to the runtime
/// first and persisting the result.
pub fn set_rack_plugin_bypassed(
    context: &RackContext<'_>,
    bypassed: bool,
) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin = active_plugin_device(&previous_session);
    let status = context.audio.set_plugin_bypassed(bypassed)?;
    if !audio_command_succeeded(&status) {
        return Ok(SessionAudioPair {
            session: previous_session,
            audio: status,
        });
    }
    let mut session = previous_session.clone();
    for device in session.rack.devices.iter_mut() {
        if device.kind == DeviceKind::Plugin {
            device.bypassed = bypassed;
        }
    }
    commit_with_rollback(context, previous_session, session, previous_plugin, status)
}

/// Sets a single plugin parameter, applying it to the runtime first and
/// persisting the current parameter values afterward.
pub fn set_rack_plugin_parameter(
    context: &RackContext<'_>,
    index: u32,
    value: f32,
) -> Result<SessionAudioPair, String> {
    if context.safe_mode {
        return Err("Safe Mode blocks external VST3 parameter changes.".into());
    }
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin = active_plugin_device(&previous_session);
    let status = context.audio.set_plugin_parameter(index, value)?;
    if !audio_command_succeeded(&status) {
        return Ok(SessionAudioPair {
            session: previous_session,
            audio: status,
        });
    }
    let status = context.audio.plugin_parameter_status()?;
    let mut session = previous_session.clone();
    let captured = status.plugin.as_ref().map(|plugin| {
        plugin
            .parameters
            .iter()
            .map(|p| p.value)
            .collect::<Vec<_>>()
    });
    for device in session.rack.devices.iter_mut() {
        if device.kind == DeviceKind::Plugin {
            if let Some(values) = captured.clone() {
                device.parameter_values = values;
            }
            device.state_data = status
                .plugin
                .as_ref()
                .and_then(|plugin| plugin.state_data.clone())
                .or_else(|| device.state_data.clone());
        }
    }
    commit_with_rollback(context, previous_session, session, previous_plugin, status)
}

/// Synchronizes the current session rack into the Audio Runtime at startup or
/// when Play becomes active.
///
/// The session is already the canonical state, so a normal restore does not
/// rewrite it; only the runtime is brought into agreement and the resulting
/// [`AudioStatus`] is returned. In Safe Mode the runtime stays isolated and the
/// current status is returned unchanged.
pub fn restore_current_rack(context: &RackContext<'_>) -> Result<AudioStatus, String> {
    let session = context.session.lock().map_err(lock_error)?.clone();
    if context.safe_mode {
        return context.audio.refresh_status();
    }
    let current = context.audio.refresh_status()?;
    if !audio_command_succeeded(&current) {
        return Err(format!(
            "Runtime rejected rack status refresh: {}",
            current.message
        ));
    }
    let Some(device) = active_plugin_device(&session) else {
        // There is no persisted Play plugin. Only tear down a stale runtime
        // plugin; an already empty rack needs no sidecar mutation.
        if current.plugin.as_ref().is_none_or(|plugin| !plugin.loaded) {
            return Ok(current);
        }
        let cleared = context.audio.clear_plugin()?;
        if !audio_command_succeeded(&cleared) {
            return Err(format!(
                "Runtime rejected stale rack plugin clear: {}",
                cleared.message
            ));
        }
        return Ok(cleared);
    };
    let Some(path) = device.path.as_deref() else {
        return Err("The saved rack plugin has no runtime path.".into());
    };
    let plugin_path = Path::new(path);
    if !plugin_path.exists()
        || plugin_path.extension().and_then(|value| value.to_str()) != Some("vst3")
    {
        // The persisted plugin is unavailable. The runtime is left unchanged
        // and the session is not rewritten; missing-dependency handling owns
        // the session-level relink/disable decision separately. If another
        // plugin is currently live, it remains available because replacement
        // is intentionally attempted only after this validation succeeds.
        return Err(format!("The saved rack plugin is unavailable: {path}"));
    }

    if runtime_plugin_matches(&device, &current) {
        // Workspace navigation is a runtime reconciliation, not a request to
        // recreate a third-party plugin. Reusing the live instance preserves
        // editor/plugin state and, critically, avoids paying VST construction
        // time on every Play -> Home -> Play transition.
        if current
            .plugin
            .as_ref()
            .is_some_and(|plugin| plugin.bypassed != device.bypassed)
        {
            return context.audio.set_plugin_bypassed(device.bypassed);
        }
        return Ok(current);
    }

    // `loadPlugin` creates and prepares the candidate before swapping it into
    // the rack. Do not clear first: that extra command creates an avoidable
    // silent gap and throws away the currently usable plugin if construction
    // of the replacement fails.
    apply_plugin_to_runtime(
        context.audio,
        path,
        &device.parameter_values,
        device.bypassed,
        device.state_data.as_deref(),
    )
}

/// Changes one rack macro value. If the macro is mapped, the Runtime parameter
/// and persisted RackInstance are committed as one operation with local
/// rollback; an unmapped macro is a session-only rack edit.
pub fn set_rack_macro_value(
    context: &RackContext<'_>,
    macro_id: &str,
    value: f32,
) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin = active_plugin_device(&previous_session);
    let value = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let parameter_index = previous_session
        .rack
        .macros
        .iter()
        .find(|item| item.id == macro_id)
        .ok_or_else(|| format!("Rack macro is not registered: {macro_id}"))?
        .parameter_index;
    let mut session = previous_session.clone();
    if let Some(item) = session
        .rack
        .macros
        .iter_mut()
        .find(|item| item.id == macro_id)
    {
        item.value = value;
    }
    let Some(index) = parameter_index else {
        return commit_session_only(context, previous_session, session);
    };
    let status = context.audio.set_plugin_parameter(index, value)?;
    if !audio_command_succeeded(&status) {
        return Err(format!(
            "Runtime rejected rack macro change: {}",
            status.message
        ));
    }
    let status = context.audio.plugin_parameter_status()?;
    if let Some(values) = status.plugin.as_ref().map(|plugin| {
        plugin
            .parameters
            .iter()
            .map(|parameter| parameter.value)
            .collect::<Vec<_>>()
    }) {
        for device in &mut session.rack.devices {
            if device.kind == DeviceKind::Plugin {
                device.parameter_values = values.clone();
                device.state_data = status
                    .plugin
                    .as_ref()
                    .and_then(|plugin| plugin.state_data.clone())
                    .or_else(|| device.state_data.clone());
            }
        }
    }
    commit_with_rollback(context, previous_session, session, previous_plugin, status)
}

pub fn map_rack_macro(
    context: &RackContext<'_>,
    macro_id: &str,
    parameter_index: Option<u32>,
) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let mut session = context.session.lock().map_err(lock_error)?.clone();
    let item = session
        .rack
        .macros
        .iter_mut()
        .find(|item| item.id == macro_id)
        .ok_or_else(|| format!("Rack macro is not registered: {macro_id}"))?;
    item.parameter_index = parameter_index;
    commit_session_only(context, previous_session, session)
}

pub fn capture_snapshot(context: &RackContext<'_>, slot: &str) -> Result<SessionAudioPair, String> {
    if !matches!(slot, "A" | "B") {
        return Err("Snapshot slot must be A or B.".into());
    }
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let mut session = previous_session.clone();
    let id = format!("snapshot:{slot}");
    let snapshot = SessionSnapshot {
        id: id.clone(),
        name: slot.to_owned(),
        created_at_ms: now_ms(),
        description: String::new(),
        tag: None,
        parent_id: None,
        master_db: session.settings.master_db,
        rack: session.rack.devices.clone(),
        macros: session.rack.macros.clone(),
    };
    session.snapshots.retain(|item| item.id != id);
    session.snapshots.push(snapshot);
    commit_session_only(context, previous_session, session)
}

/// Recalls an A/B session snapshot: clears the current runtime plugin, applies
/// the snapshot's plugin (if any) to the runtime, then commits the snapshot's
/// rack devices, macros, and master gain to the session. The snapshot's plugin
/// is restored as a unit (state blob supersedes individual parameters) so
/// React never re-derives the rack or sequences the low-level runtime calls
/// itself. Persistence failure rolls the runtime back to the previous plugin.
pub fn recall_snapshot(context: &RackContext<'_>, slot: &str) -> Result<SessionAudioPair, String> {
    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let snapshot_id = format!("snapshot:{slot}");
    let snapshot = previous_session
        .snapshots
        .iter()
        .find(|candidate| candidate.id == snapshot_id)
        .ok_or_else(|| format!("Snapshot slot {slot} is not registered."))?
        .clone();
    let previous_plugin = active_plugin_device(&previous_session);

    let cleared = context.audio.clear_plugin()?;
    if !audio_command_succeeded(&cleared) {
        // Runtime could not clear; surface the status and do not touch session.
        return Ok(SessionAudioPair {
            session: previous_session,
            audio: cleared,
        });
    }

    let new_plugin = snapshot
        .rack
        .iter()
        .find(|device| device.kind == DeviceKind::Plugin && !device.disabled_placeholder)
        .cloned();
    let final_status = match new_plugin.as_ref() {
        Some(device) => {
            let Some(path) = device.path.as_deref() else {
                return Ok(SessionAudioPair {
                    session: previous_session,
                    audio: cleared,
                });
            };
            let status = apply_plugin_to_runtime(
                context.audio,
                path,
                &device.parameter_values,
                device.bypassed,
                device.state_data.as_deref(),
            )?;
            if !audio_command_succeeded(&status) {
                return Ok(SessionAudioPair {
                    session: previous_session,
                    audio: status,
                });
            }
            status
        }
        None => cleared,
    };

    let mut session = previous_session.clone();
    session.settings.master_db = snapshot.master_db;
    session.rack.devices = snapshot.rack.clone();
    session.rack.macros = snapshot.macros.clone();
    commit_with_rollback(
        context,
        previous_session,
        session,
        previous_plugin,
        final_status,
    )
}

// Canonical RackDefinition Asset operations.
//
// These bridge the canonical Asset store (where RackDefinitions live as
// JSON-rendered Assets) and the Audio Runtime + CreativeSession rack. They are
// Production Workflow because a single user intent loads a definition, applies
// it to the runtime, updates the persisted session, and rolls the runtime back
// if persistence fails — exactly the same shape as the in-session rack
// operations above.

/// Saves the current session rack as a canonical `RackDefinition` Asset. The
/// definition is written to the user-supplied path, then registered as an
/// Asset so it is searchable from the Library without duplicating its metadata
/// into the Library Read Model.
pub fn save_rack_definition(
    context: &RackContext<'_>,
    name: &str,
    path: &str,
) -> Result<crate::asset::AssetId, String> {
    let definition = {
        let session = context.session.lock().map_err(lock_error)?;
        crate::rack::RackDefinition::from_instance(&session.rack)
    };
    let path_buf = std::path::PathBuf::from(path);
    if path_buf.as_os_str().is_empty() {
        return Err("Rack definition path must not be empty.".into());
    }
    if let Some(parent) = path_buf.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Rack definition folder could not be created: {error}"))?;
    }
    let payload = serde_json::to_vec_pretty(&definition)
        .map_err(|error| format!("Rack definition could not be encoded: {error}"))?;
    std::fs::write(&path_buf, payload)
        .map_err(|error| format!("Rack definition could not be saved: {error}"))?;
    let asset_id = crate::asset::register_rack_definition(
        context.data_root,
        &definition,
        name,
        &path_buf.to_string_lossy(),
    )?;
    // The canonical RackDefinition Asset is searchable directly from the
    // `assets` table via `list_rack_definitions` / `search`; do not mirror its
    // metadata into the Library Read Model.
    Ok(asset_id)
}

/// Loads a canonical `RackDefinition` Asset into the current rack. The runtime
/// is cleared first; if the definition has an active plugin, it is loaded with
/// its saved state/parameters/bypass. The session rack is then updated and
/// persisted; a persistence failure rolls the runtime back to the previous
/// plugin so the two never diverge.
pub fn load_rack_definition_asset(
    context: &RackContext<'_>,
    asset_id: crate::asset::AssetId,
) -> Result<SessionAudioPair, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks rack application; restart normally to apply a saved rack.".into(),
        );
    }
    let asset = crate::asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("RackDefinition asset is not registered: {asset_id}"))?;
    if asset.kind != crate::asset::AssetKind::RackDefinition {
        return Err(format!("Asset {asset_id} is not a RackDefinition."));
    }
    let payload = std::fs::read(&asset.content_location)
        .map_err(|error| format!("RackDefinition payload could not be read: {error}"))?;
    let definition: crate::rack::RackDefinition = serde_json::from_slice(&payload)
        .map_err(|error| format!("RackDefinition payload is invalid: {error}"))?;
    definition.runtime_supported().map_err(|error| {
        // Surface unsupported definitions as an explicit failure rather than a
        // partial apply, per the canonical-load contract.
        format!("unsupported rack definition: {error}")
    })?;

    let previous_session = context.session.lock().map_err(lock_error)?.clone();
    let previous_plugin_device = previous_session
        .rack
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Plugin && !device.disabled_placeholder)
        .cloned();

    // Apply the new definition to the runtime first. If this fails, the
    // session is not touched.
    let cleared = context.audio.clear_plugin()?;
    if !audio_command_succeeded(&cleared) {
        return Err(format!(
            "Runtime could not clear the existing plugin before applying the rack: {}",
            cleared.message
        ));
    }
    let final_status = if let Some(device) = definition.active_plugin_device() {
        let Some(path) = device.path.as_deref() else {
            return Err("Rack definition references a plugin device without a path.".to_string());
        };
        let status = apply_plugin_to_runtime(
            context.audio,
            path,
            &device.parameter_values,
            device.bypassed,
            device.state_data.as_deref(),
        )?;
        if !audio_command_succeeded(&status) {
            return Err(format!(
                "Runtime rejected plugin '{}'; the rack was not applied.",
                device.name
            ));
        }
        status
    } else {
        cleared
    };

    let mut session = previous_session.clone();
    session.rack = crate::rack::RackInstance::from_definition(&definition);
    commit_with_rollback(
        context,
        previous_session,
        session,
        previous_plugin_device,
        final_status,
    )
}
