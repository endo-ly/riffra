//! Tauri Application Composition Root.
//!
//! `lib.rs` deliberately hosts only:
//!
//! - `mod` declarations,
//! - the `AppState` struct and the `pub(crate)` queue helper used by feature
//!   modules to refresh the Library index after a canonical-state change,
//! - the Tauri `setup` hook (load session, start audio supervisor, register
//!   managed state),
//! - the `invoke_handler` registration that wires Tauri commands to their
//!   feature-level implementations,
//! - a small number of app-level aggregations that do not belong to a single
//!   feature (`get_bootstrap_state`, the device probe sidecar, generic
//!   background-job lifecycle, low-level Audio Runtime passthroughs that no
//!   Production Workflow in React depends on).
//!
//! All Production Workflow lives in the feature modules: Recording lifecycle
//! and Inbox management in `recording`, background-job orchestration in
//! `analysis` / `separation` / `render` / `plugins`, rack + RackDefinition
//! operations in `rack`, session + arrangement + design + missing-dep recovery
//! in `session`, library read-model queries in `library`, and asset preview in
//! `asset`.

mod analysis;
mod asset;
mod audio_preferences;
mod diagnostics;
mod errors;
mod jobs;
mod library;
mod missing;
mod model;
mod native_audio;
mod plugin_catalog;
mod plugin_validation;
mod plugins;
mod projects;
mod rack;
mod recording;
mod render;
mod runtime_reconciler;
mod separation;
mod session;
mod storage;
#[cfg(test)]
mod types;

use model::{
    AudioDeviceProbe, AudioDriverInfo, AudioStatus, BootstrapState, MidiProbe, RecoveryCandidate,
    RuntimeProjectionStatus,
};
use native_audio::AudioSupervisor;
use riffra_core::AppCore;
use riffra_render_worker::RenderWorker;
use serde::Deserialize;
use session::CreativeSession;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use storage::SessionStore;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::ShellExt;

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";

struct AppState {
    core: AppCore<AudioSupervisor>,
    session_actor: session::actor::SessionActor,
    rack_operation_gate: Mutex<()>,
    recording_operation_gate: Mutex<()>,
    /// Keeps a workspace's stop intent and processing-mode update together so
    /// concurrent navigation commands cannot interleave the two writes.
    workspace_runtime_gate: Mutex<()>,
    runtime: runtime_reconciler::RuntimeReconciler<AudioSupervisor>,
    render_worker: RenderWorker,
    audio_preferences: Mutex<audio_preferences::AudioPreferences>,
    jobs: jobs::JobRegistry,
}

#[derive(Default)]
struct PendingSessionIndex {
    latest: Option<CreativeSession>,
    running: bool,
}

// Session saves are intentionally durable per user intent, but the Library
// read-model refresh is derived data. A rapid sequence of parameter/arrangement
// edits must not create one blocking database worker per click; only the latest
// session for a data root is useful once an earlier refresh is already queued.
static SESSION_INDEX_QUEUE: OnceLock<Mutex<HashMap<PathBuf, PendingSessionIndex>>> =
    OnceLock::new();

/// Runs a synchronous native/persistence operation on Tokio's blocking pool.
/// An `async fn` alone only changes where the future is scheduled; it does not
/// make Condvar waits, VST IPC, fsync, or database work non-blocking. Keeping
/// those waits off the async worker pool is necessary for window commands and
/// status delivery to remain serviceable while a plugin is busy.
async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Native blocking operation failed: {error}"))?
}

/// Refreshes the Library Read Model after a Production Operation has changed
/// the canonical CreativeSession. Feature modules call this instead of
/// re-implementing the spawn_blocking + sync_session fan-out.
pub(crate) fn queue_session_index(data_root: &std::path::Path, session: &CreativeSession) {
    let data_root = data_root.to_path_buf();
    let should_start = {
        let queues = SESSION_INDEX_QUEUE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut queues = match queues.lock() {
            Ok(queues) => queues,
            Err(error) => abort_on_poison(error),
        };
        let pending = queues.entry(data_root.clone()).or_default();
        pending.latest = Some(session.clone());
        if pending.running {
            false
        } else {
            pending.running = true;
            true
        }
    };
    if !should_start {
        return;
    }

    tauri::async_runtime::spawn_blocking(move || {
        loop {
            let session = {
                let queues = SESSION_INDEX_QUEUE.get_or_init(|| Mutex::new(HashMap::new()));
                let mut queues = match queues.lock() {
                    Ok(queues) => queues,
                    Err(error) => abort_on_poison(error),
                };
                let Some(pending) = queues.get_mut(&data_root) else {
                    return;
                };
                let Some(session) = pending.latest.take() else {
                    queues.remove(&data_root);
                    return;
                };
                session
            };
            let _ = library::sync_session(&data_root, &session);
        }
    });
}

/// Completes startup-only persistence and native reconciliation after Tauri
/// has registered managed state. Audio-device initialization can involve a
/// driver handshake, so it must not hold up the first WebView paint.
fn queue_startup_maintenance(
    app_handle: AppHandle,
    data_root: std::path::PathBuf,
    session: CreativeSession,
    requested_preferences: audio_preferences::AudioPreferences,
) {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        if let Err(error) = library::sync_session(&data_root, &session) {
            let _ = diagnostics::record(&data_root, "startup-library", &error);
        }
        if state.core.safe_mode() {
            return;
        }

        match state.core.audio().refresh_status() {
            Ok(status) => match audio_preferences::AudioPreferences::from_effective_status(&status)
            {
                Ok(effective) => {
                    let preferences_unchanged = state
                        .audio_preferences
                        .lock()
                        .map(|current| *current == requested_preferences)
                        .unwrap_or(false);
                    if preferences_unchanged {
                        if let Err(error) =
                            audio_preferences::AudioPreferencesStore::new(&data_root)
                                .save(&effective)
                        {
                            let _ = diagnostics::record(&data_root, "startup-audio", &error);
                        }
                        if let Err(error) = state
                            .core
                            .audio()
                            .set_restart_preferences(effective.clone())
                        {
                            let _ = diagnostics::record(&data_root, "startup-audio", &error);
                        }
                        if let Ok(mut current) = state.audio_preferences.lock() {
                            *current = effective;
                        }
                    }
                }
                Err(error) => {
                    let _ = diagnostics::record(&data_root, "startup-audio", &error);
                }
            },
            Err(error) => {
                let _ = diagnostics::record(&data_root, "startup-audio", &error);
            }
        }

        let projection = match state.session_actor.capture_projection(state.core.session()) {
            Ok(projection) => projection,
            Err(error) => {
                let _ = diagnostics::record(&data_root, "startup-runtime", &error);
                return;
            }
        };
        let current_session = projection.session;
        let workspace = current_session.workspace;
        let mode = match workspace {
            session::Workspace::Play => "play",
            session::Workspace::Arrange => "arrange",
            session::Workspace::Home | session::Workspace::Design => "passive",
        };
        if let Err(error) = state.core.audio().set_processing_mode(mode) {
            let _ = diagnostics::record(&data_root, "startup-audio", &error);
        }
        if workspace == session::Workspace::Arrange {
            let snapshot =
                session::application::runtime_snapshot_for_recording(&data_root, &current_session);
            let _ = state.runtime.submit(
                snapshot,
                runtime_reconciler::ProjectionKey {
                    sequence: projection.sequence,
                    session_revision: current_session.arrangement.revision,
                },
            );
        }
    });
}

#[tauri::command]
async fn get_bootstrap_state(app: AppHandle) -> Result<BootstrapState, String> {
    run_blocking(app, |state| {
        Ok(BootstrapState {
            session: state.core.session().lock().map_err(lock_error)?.clone(),
            recovered_from_generation: state.core.recovered_from_generation(),
            safe_mode: state.core.safe_mode(),
            native_available: true,
            recovery_candidates: SessionStore::new(state.core.data_root())
                .recovery_candidates()
                .map_err(|error| format!("Recovery candidates could not be read: {error}"))?
                .into_iter()
                .map(|candidate| RecoveryCandidate {
                    file_name: candidate.file_name,
                    updated_at_ms: candidate.updated_at_ms,
                    session_id: candidate.session_id,
                    project_name: candidate.project_name,
                    note: candidate.note,
                })
                .collect(),
            data_root: state.core.data_root().to_string_lossy().into_owned(),
            vst3_root: DEFAULT_VST3_ROOT.into(),
        })
    })
    .await
}

#[tauri::command]
async fn export_scratch_session(app: AppHandle) -> Result<projects::ProjectExport, String> {
    run_blocking(app, |state| {
        let session = state.core.session().lock().map_err(lock_error)?.clone();
        projects::export(state.core.data_root(), &session, storage::now_ms())
    })
    .await
}

#[tauri::command]
fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .jobs
        .status(&id)
        .map(jobs::to_background_status)
        .transpose()
}

#[tauri::command]
fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .jobs
        .cancel(&id)
        .map(jobs::to_background_status)
        .transpose()
}

// App-level audio device discovery. The native sidecar owns the actual probe;
// Rust parses its stdout into the same DTOs the React layer consumes. These
// commands are pure device queries — they touch neither the canonical session
// nor the Asset registry.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMidiProbe {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default)]
    midi_inputs: Vec<model::MidiDeviceInfo>,
    #[serde(default)]
    midi_outputs: Vec<model::MidiDeviceInfo>,
    #[serde(default)]
    drivers: Vec<NativeAudioDriver>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeAudioDriver {
    name: String,
    #[serde(default)]
    access_mode: model::AudioAccessMode,
    device_pairing: model::AudioDevicePairing,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
}

#[tauri::command]
async fn probe_midi_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<MidiProbe, String> {
    if state.core.safe_mode() {
        return Ok(MidiProbe {
            inputs: Vec::new(),
            outputs: Vec::new(),
            refreshed_at_ms: storage::now_ms(),
            message: "Safe Mode skipped MIDI discovery.".into(),
        });
    }
    let probe = run_native_probe(app).await?;
    let empty = probe.midi_inputs.is_empty() && probe.midi_outputs.is_empty();
    Ok(MidiProbe {
        inputs: probe.midi_inputs,
        outputs: probe.midi_outputs,
        refreshed_at_ms: storage::now_ms(),
        message: if empty {
            "No MIDI devices are currently visible to Windows.".into()
        } else {
            "MIDI device list refreshed.".into()
        },
    })
}

#[tauri::command]
async fn probe_audio_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AudioDeviceProbe, String> {
    if state.core.safe_mode() {
        return Ok(AudioDeviceProbe {
            drivers: Vec::new(),
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            refreshed_at_ms: storage::now_ms(),
            message: "Safe Mode skipped audio and MIDI device discovery.".into(),
        });
    }
    let probe = run_native_probe(app).await?;
    Ok(AudioDeviceProbe {
        drivers: probe
            .drivers
            .into_iter()
            .map(|driver| AudioDriverInfo {
                name: driver.name,
                access_mode: driver.access_mode,
                device_pairing: driver.device_pairing,
                inputs: driver.inputs,
                outputs: driver.outputs,
            })
            .collect(),
        midi_inputs: probe.midi_inputs,
        midi_outputs: probe.midi_outputs,
        refreshed_at_ms: storage::now_ms(),
        message: "Audio and MIDI device list refreshed.".into(),
    })
}

async fn run_native_probe(app: tauri::AppHandle) -> Result<NativeMidiProbe, String> {
    let command = app
        .shell()
        .sidecar("riffra-audio")
        .map_err(|error| format!("Device probe sidecar could not be prepared: {error}"))?
        .args(["--probe"]);
    let output = command.output().await.map_err(|error| {
        format!("Device probe could not start; no device state was changed: {error}")
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!(
                "Device probe exited with code {:?}; no device state was changed.",
                output.status.code()
            )
        } else {
            format!("Device probe failed: {detail}")
        });
    }
    parse_midi_probe(&output.stdout)
}

fn parse_midi_probe(stdout: &[u8]) -> Result<NativeMidiProbe, String> {
    let probe = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_slice::<NativeMidiProbe>(line).ok())
        .ok_or_else(|| "MIDI probe returned no readable device list.".to_string())?;
    if probe.message_type != "audioDeviceProbe" {
        return Err("MIDI probe returned an unexpected response.".into());
    }
    Ok(probe)
}

// Low-level Audio Runtime passthroughs.
//
// These commands are single delegations to the Audio Runtime with no
// canonical-state side effects. They stay in `lib.rs` because they are not
// Production Workflow: the rack + session coordination that used to call them
// from React has moved to Rust Application Operations (rack::application,
// session::application), and the remaining React UI only uses them for
// transport-style runtime control (preview voices, MIDI open/close, device
// recovery). Their session-persisting counterparts (master gain, driver
// selection, emergency mute) live in `session::commands` so the session stays
// in lock-step with the runtime.

#[tauri::command]
async fn get_audio_status(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| state.core.audio().refresh_meters()).await
}

#[tauri::command]
async fn get_runtime_projection_status(app: AppHandle) -> Result<RuntimeProjectionStatus, String> {
    run_blocking(app, |state| Ok(state.runtime.status())).await
}

#[tauri::command]
async fn preview_master_gain_db(gain_db: f64, app: AppHandle) -> Result<(), String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    run_blocking(app, move |state| {
        state.core.audio().preview_master_gain_db(gain_db)
    })
    .await
}

#[tauri::command]
async fn set_emergency_mute(muted: bool, app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        state.core.audio().set_emergency_mute(muted)
    })
    .await
}

#[tauri::command]
async fn recover_audio_device(app: AppHandle) -> Result<AudioStatus, String> {
    let operation_app = app.clone();
    run_blocking(app, move |state| {
        if state.core.safe_mode() {
            return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
        }
        state.core.audio().recover_audio_device(&operation_app)
    })
    .await
}

#[tauri::command]
async fn enable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        if state.core.safe_mode() {
            return Err(
                "Safe Mode blocks MIDI input; offline MIDI and audio export remain available."
                    .into(),
            );
        }
        state.core.audio().enable_midi_listening()
    })
    .await
}

#[tauri::command]
async fn disable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| state.core.audio().disable_midi_listening()).await
}

#[tauri::command]
async fn send_midi_to_plugin(bytes: Vec<u8>, app: AppHandle) -> Result<(), String> {
    run_blocking(app, move |state| {
        if state.core.safe_mode() {
            return Err(
                "Safe Mode blocks outgoing MIDI; offline MIDI and audio export remain available."
                    .into(),
            );
        }
        state.core.audio().send_midi(&bytes)
    })
    .await
}

#[tauri::command]
async fn stop_preview(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| state.core.audio().stop_preview()).await
}

#[tauri::command]
async fn stop_preview_for_key(voice_key: i32, app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        state.core.audio().stop_preview_for_key(voice_key)
    })
    .await
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}

fn abort_on_poison<T>(error: std::sync::PoisonError<T>) -> ! {
    eprintln!("[riffra] Internal state lock was poisoned: {error}. Aborting.");
    std::process::abort();
}

fn safe_mode_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref().eq_ignore_ascii_case("--safe-mode"))
}

fn safe_mode_requested() -> bool {
    safe_mode_from_args(std::env::args())
        || std::env::var("RIFFRA_SAFE_MODE")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed)
                && let Some(state) = window.try_state::<AppState>()
            {
                // The window is actually gone; the frontend has already had
                // its chance to flush plugin state before calling destroy().
                // Shutting the audio sidecar down only now keeps the runtime
                // alive while a close request is still cancellable.
                state.core.audio().force_shutdown();
            }
        })
        .setup(|app| {
            let safe_mode = safe_mode_requested();
            let data_root = app.path().app_data_dir().map_err(|error| {
                format!("Windows application data folder is unavailable: {error}")
            })?;
            std::fs::create_dir_all(&data_root)?;
            let loaded = SessionStore::new(&data_root).load_or_create()?;
            let session = loaded.session;
            let recovered_from_generation = loaded.recovered_from_generation;
            let preferences = audio_preferences::load_or_default(&data_root)?;
            let audio = if safe_mode {
                AudioSupervisor::offline(
                    "Safe Mode is active; native audio, MIDI, and external plugins remain isolated.",
                )
            } else {
                AudioSupervisor::start(app.handle(), preferences.clone())
            };
            let runtime_audio = audio.clone();
            let runtime_app = app.handle().clone();
            let runtime_recovery: runtime_reconciler::RuntimeRecovery =
                std::sync::Arc::new(move |expected_generation, timeout| {
                    runtime_audio.restart_sidecar_for_runtime(
                        &runtime_app,
                        expected_generation,
                        timeout,
                    )
                });
            let runtime = runtime_reconciler::RuntimeReconciler::new(
                std::sync::Arc::new(audio.clone()),
                Some(runtime_recovery),
            )?;
            let effective_preferences = preferences.clone();
            audio.set_restart_preferences(effective_preferences.clone())?;
            let startup_data_root = data_root.clone();
            let startup_session = session.clone();
            app.manage(AppState {
                core: AppCore::new(
                    data_root,
                    session,
                    audio,
                    recovered_from_generation,
                    safe_mode,
                ),
                session_actor: session::actor::SessionActor::default(),
                rack_operation_gate: Mutex::new(()),
                recording_operation_gate: Mutex::new(()),
                workspace_runtime_gate: Mutex::new(()),
                runtime,
                render_worker: RenderWorker::bundled()?,
                audio_preferences: Mutex::new(effective_preferences),
                jobs: jobs::JobRegistry::default(),
            });
            queue_startup_maintenance(
                app.handle().clone(),
                startup_data_root,
                startup_session,
                preferences,
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            export_scratch_session,
            get_background_job,
            cancel_background_job,
            probe_audio_devices,
            probe_midi_devices,
            get_audio_status,
            get_runtime_projection_status,
            preview_master_gain_db,
            set_emergency_mute,
            recover_audio_device,
            enable_midi_listening,
            disable_midi_listening,
            send_midi_to_plugin,
            stop_preview,
            stop_preview_for_key,
            // Rack Application Operations.
            rack::commands::load_plugin_into_rack,
            rack::commands::clear_plugin_from_rack,
            rack::commands::open_plugin_editor,
            rack::commands::set_rack_plugin_bypassed,
            rack::commands::set_rack_plugin_parameter,
            rack::commands::set_rack_macro_value,
            rack::commands::map_rack_macro,
            rack::commands::restore_current_rack,
            rack::commands::capture_snapshot,
            rack::commands::recall_snapshot,
            rack::commands::save_rack_definition,
            rack::commands::list_rack_definitions,
            rack::commands::load_rack_definition_asset,
            // Session Application Operations.
            session::commands::save_scratch_session,
            session::commands::restore_recovery_generation,
            session::commands::import_scratch_session,
            session::commands::create_sample_pad,
            session::commands::update_sample_pad,
            session::commands::remove_sample_pad,
            session::commands::add_audio_clip_to_arrangement,
            session::commands::add_midi_clip_to_arrangement,
            session::commands::update_audio_clip,
            session::commands::remove_timeline_clips,
            session::commands::trim_audio_clip,
            session::commands::split_audio_clip,
            session::commands::duplicate_audio_clip,
            session::commands::move_audio_clips,
            session::commands::update_midi_clip,
            session::commands::move_midi_clips,
            session::commands::trim_midi_clip,
            session::commands::split_midi_clip,
            session::commands::duplicate_midi_clip,
            session::commands::paste_timeline_clips,
            session::commands::crossfade_audio_clips,
            session::commands::sync_arrangement_runtime,
            session::commands::restore_sample_pads,
            session::commands::play_timeline,
            session::commands::stop_timeline,
            session::commands::seek_timeline,
            session::commands::update_arrangement_timebase,
            session::commands::update_timeline_loop_range,
            session::commands::update_timeline_punch_range,
            session::commands::open_asset_in_design,
            session::commands::switch_workspace,
            session::commands::update_session_settings,
            session::commands::add_track,
            session::commands::update_track,
            session::commands::set_track_automation,
            session::commands::set_track_audio_input,
            session::commands::set_track_midi_input,
            session::commands::set_track_instrument,
            session::commands::clear_track_instrument,
            session::commands::add_track_effect,
            session::commands::remove_track_effect,
            session::commands::reorder_track_effects,
            session::commands::set_track_device_bypassed,
            session::commands::set_track_device_parameter,
            session::commands::open_track_plugin_editor,
            session::commands::persist_track_plugin_state,
            session::commands::persist_track_plugin_parameter,
            session::commands::remove_track,
            session::commands::duplicate_track,
            session::commands::reorder_track,
            session::commands::add_marker,
            session::commands::update_marker,
            session::commands::remove_marker,
            session::commands::add_midi_note,
            session::commands::update_midi_note,
            session::commands::update_midi_notes,
            session::commands::remove_midi_note,
            session::commands::quantize_midi_notes,
            session::commands::duplicate_midi_notes,
            session::commands::set_audio_clip_take_variant,
            session::commands::start_take_comparison,
            session::commands::switch_take_comparison_variant,
            session::commands::stop_take_comparison,
            session::commands::activate_take,
            session::commands::place_take_as_separate_clip,
            session::commands::apply_ai_suggestion,
            session::commands::set_master_gain_db,
            audio_preferences::set_audio_driver,
            session::commands::relink_missing_dependency,
            session::commands::disable_missing_plugin,
            session::commands::replace_missing_track_plugin,
            session::commands::get_missing_dependencies,
            // Asset Application Operations.
            asset::commands::preview_asset,
            asset::commands::import_midi_file,
            asset::commands::import_midi_bytes,
            // Recording Application Operations.
            recording::commands::list_recordings,
            recording::commands::rename_recording,
            recording::commands::delete_recording,
            recording::commands::archive_recording,
            recording::commands::promote_recording,
            recording::commands::detect_duplicate_recordings,
            recording::commands::tag_recording,
            recording::commands::start_recording,
            recording::commands::start_arrange_recording,
            recording::commands::record_another_take,
            recording::commands::stop_recording,
            recording::commands::stop_arrange_recording,
            // Library Read Model queries / updates.
            library::commands::search_library,
            library::commands::update_library_asset,
            library::commands::related_library_assets,
            // MIDI export.
            // Background-job orchestration per feature.
            analysis::commands::start_analysis_job,
            analysis::commands::analyze_asset,
            separation::commands::start_separation_job,
            separation::commands::list_separations,
            render::commands::render_timeline,
            plugins::commands::scan_vst3_folder,
            plugins::commands::start_scan_job
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}

#[cfg(test)]
mod tests {
    use super::{parse_midi_probe, safe_mode_from_args};

    #[test]
    fn parses_midi_probe_with_unicode_device_names() {
        let probe = parse_midi_probe(
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","accessMode":"driverManaged","devicePairing":"sameDevice","inputs":["Focusrite"],"outputs":["Focusrite"]}],"midiInputs":[{"id":"midi-in-1","name":"Keyboard"}],"midiOutputs":[{"id":"midi-out-1","name":"Microsoft GS Wavetable Synth"}]}"#,
        )
        .unwrap();
        assert_eq!(probe.drivers[0].name, "ASIO");
        assert_eq!(probe.midi_inputs[0].id, "midi-in-1");
        assert_eq!(probe.midi_inputs[0].name, "Keyboard");
        assert_eq!(probe.midi_outputs[0].id, "midi-out-1");
    }

    #[test]
    fn rejects_non_probe_messages() {
        let error = parse_midi_probe(br#"{"type":"audioStatus"}"#).unwrap_err();
        assert!(error.contains("unexpected"));
    }

    #[test]
    fn recognizes_safe_mode_only_from_explicit_flag() {
        assert!(safe_mode_from_args(["riffra.exe", "--safe-mode"]));
        assert!(safe_mode_from_args(["--SAFE-MODE"]));
        assert!(!safe_mode_from_args(["riffra.exe", "--serve"]));
    }

    #[test]
    fn denies_network_client_dependencies() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml"))
            .expect("Cargo.toml must be readable for the SEC-001 guard");

        let dependencies = manifest
            .split("\n[dependencies]")
            .nth(1)
            .and_then(|section| section.split("\n[").next())
            .unwrap_or("");

        let forbidden = [
            "reqwest",
            "ureq",
            "hyper",
            "isahc",
            "attohttpc",
            "surf",
            "minreq",
            "curl",
            "tauri-plugin-http",
        ];
        for crate_name in forbidden {
            let prefix = format!("{crate_name} =");
            let offender = dependencies
                .lines()
                .find(|line| line.trim_start().starts_with(&prefix));
            assert!(
                offender.is_none(),
                "SEC-001 violation: network client crate '{crate_name}' is listed in [dependencies]. \
                 Local First requires no implicit network transport; audio, project, and AI context \
                 must not leave the machine without explicit user action. \
                 Offending line: {}",
                offender.unwrap_or("?")
            );
        }
    }
}
