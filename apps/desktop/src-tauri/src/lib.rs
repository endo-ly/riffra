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
//! - startup state construction and the invoke registration table.
//!
//! All Production Workflow lives in the feature modules: Recording lifecycle
//! and Inbox management in `recording`, background-job orchestration in
//! `analysis` / `separation` / `render` / `plugins`, session + arrangement +
//! design + missing-dep recovery in `session`, library read-model queries in
//! `library`, and asset preview in `asset`. App-level commands and native
//! probes live in `host_commands`.

mod analysis;
mod asset;
mod audio_preferences;
mod diagnostics;
mod host_commands;
mod jobs;
mod library;
mod missing;
mod model;
mod native_audio;
mod plugin_catalog;
mod plugin_validation;
mod plugins;
mod presentation;
mod projects;
mod recording;
mod render;
mod runtime;
mod separation;
mod session;
mod startup;
mod storage;
#[cfg(test)]
mod types;

use host_commands::*;
use model::{
    AudioDeviceProbe, AudioDriverInfo, AudioStatus, BootstrapState, MidiProbe, RecoveryCandidate,
    RuntimeProjectionStatus, RuntimeStartupFinishedEvent,
};
use native_audio::{AudioDeviceReopenOutcome, AudioSupervisor};
use riffra_core::{AppCore, CreativeSession};
use riffra_render_worker::RenderWorker;
use serde::Deserialize;
use session::adapter as session_adapter;
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use storage::SessionStore;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";
const NATIVE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

static NATIVE_PROBE_GATE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

struct AppState {
    core: AppCore<AudioSupervisor>,
    view_state: Mutex<presentation::DesktopViewState>,
    command_gate: Mutex<()>,
    recording_operation_gate: Mutex<()>,
    runtime: Arc<runtime::RuntimeReconciler<AudioSupervisor>>,
    render_worker: RenderWorker,
    audio_preferences: Mutex<audio_preferences::AudioPreferences>,
    jobs: jobs::JobRegistry,
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
        let (status, runtime_started) = if state.core.safe_mode() {
            (None, state.core.audio().startup_completed())
        } else {
            match startup::initialize_audio_runtime(&state, || {
                library::index::queue(&data_root, &session);
            }) {
                Ok(initialization) => {
                    let runtime_started = initialization.runtime_error.is_none();
                    if let Some(error) = initialization.runtime_error.as_deref() {
                        let _ = diagnostics::record(&data_root, "startup-runtime", error);
                    }
                    (Some(initialization.status), runtime_started)
                }
                Err(error) => {
                    let _ = diagnostics::record(&data_root, "startup-audio", &error);
                    (None, false)
                }
            }
        };
        let _ = app_handle.emit(
            "runtime-startup-finished",
            RuntimeStartupFinishedEvent {
                succeeded: runtime_started,
            },
        );
        state.core.audio().emit_status(&app_handle);

        if state.core.safe_mode() {
            library::index::queue(&data_root, &session);
            return;
        }

        if let Some(status) = status {
            match audio_preferences::AudioPreferences::from_effective_status(&status) {
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
                            let message = error.to_string();
                            let _ = diagnostics::record(&data_root, "startup-audio", &message);
                        }
                        if let Ok(mut current) = state.audio_preferences.lock() {
                            *current = effective;
                        }
                    }
                }
                Err(error) => {
                    let _ = diagnostics::record(&data_root, "startup-audio", &error);
                }
            }
        }
    });
}

fn bootstrap_recovery_candidates(
    data_root: &std::path::Path,
    recovered_from_generation: bool,
) -> Result<Vec<RecoveryCandidate>, String> {
    if !recovered_from_generation {
        return Ok(Vec::new());
    }
    SessionStore::new(data_root)
        .recovery_candidates()
        .map_err(|error| format!("Recovery candidates could not be read: {error}"))
        .map(|candidates| {
            candidates
                .into_iter()
                .map(|candidate| RecoveryCandidate {
                    file_name: candidate.file_name,
                    updated_at_ms: candidate.updated_at_ms,
                    session_id: candidate.session_id,
                    project_name: candidate.project_name,
                    note: candidate.note,
                })
                .collect()
        })
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
            let preferences = audio_preferences::load_or_default(&data_root)?;
            let audio = if safe_mode {
                AudioSupervisor::offline(
                    "Safe Mode is active; native audio, MIDI, and external plugins remain isolated.",
                )
            } else {
                AudioSupervisor::start(app.handle(), preferences.clone())
            };
            let loaded = match SessionStore::new(&data_root).load_or_create() {
                Ok(loaded) => loaded,
                Err(error) => {
                    audio.force_shutdown();
                    return Err(error.into());
                }
            };
            let session = loaded.session;
            let recovered_from_generation = loaded.recovered_from_generation;
            let core = AppCore::new(
                data_root.clone(),
                session.clone(),
                audio.clone(),
                recovered_from_generation,
                safe_mode,
            );
            let runtime_recovery:
                Option<crate::runtime::projection_coordinator::RuntimeRecovery> = if safe_mode {
                None
            } else {
                let runtime_audio = audio.clone();
                let runtime_app = app.handle().clone();
                Some(Arc::new(move |expected_generation, timeout| {
                    runtime_audio.restart_sidecar_for_runtime(
                        &runtime_app,
                        expected_generation,
                        timeout,
                    )
                    .map_err(crate::runtime::error::RuntimeError::from)
                }) as crate::runtime::projection_coordinator::RuntimeRecovery)
            };
            let runtime = Arc::new(runtime::RuntimeReconciler::new(
                Arc::new(audio.clone()),
                runtime_recovery,
            )?);
            let runtime_for_restart = Arc::downgrade(&runtime);
            let recovery_session = core.shared_session();
            let recovery_data_root = data_root.clone();
            audio.set_runtime_restart_handler(Arc::new(move |runtime_audio, generation| {
                let pads = recovery_session
                    .snapshot()
                    .map_err(|error| {
                        format!("Canonical Session could not be read during Runtime recovery: {error}")
                    })
                    .and_then(|session| {
                        session_adapter::resolve_native_pads(
                            &recovery_data_root,
                            &session.play_state.sample_instrument.pads,
                        )
                    });
                match pads {
                    Ok(pads) => match runtime_audio.configure_sample_pads(&pads) {
                        Ok(status) if session_adapter::audio_command_succeeded(&status) => {}
                        Ok(status) => tracing::warn!(
                            generation,
                            message = %status.message,
                            "Sample Pad restoration after Runtime restart was rejected"
                        ),
                        Err(error) => tracing::warn!(
                            generation,
                            error = %error,
                            "Sample Pad restoration after Runtime restart failed"
                        ),
                    },
                    Err(error) => tracing::warn!(
                        generation,
                        error = %error,
                        "Sample Pad state could not be prepared after Runtime restart"
                    ),
                }
                if let Some(runtime) = runtime_for_restart.upgrade()
                    && !runtime.requeue_after_runtime_restart(generation)
                    && let Err(error) = runtime_audio.release_runtime_mute_if_allowed()
                {
                    tracing::warn!(
                        generation,
                        error = %error,
                        "Runtime restart had no graph to restore and mute release failed"
                    );
                }
            }))?;
            let effective_preferences = preferences.clone();
            audio.set_restart_preferences(effective_preferences.clone())?;
            let startup_data_root = data_root.clone();
            let startup_session = session.clone();
            app.manage(AppState {
                core,
                view_state: Mutex::new(presentation::DesktopViewState::default()),
                command_gate: Mutex::new(()),
                recording_operation_gate: Mutex::new(()),
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
            probe_device_channels,
            get_audio_status,
            get_runtime_projection_status,
            preview_master_gain_db,
            set_emergency_mute,
            recover_audio_device,
            retry_startup_runtime,
            enable_midi_listening,
            disable_midi_listening,
            session::commands::send_midi_to_track,
            session::commands::panic_midi_track,
            stop_preview,
            stop_preview_for_key,
            // Session Application Operations.
            session::commands::undo_session,
            session::commands::redo_session,
            session::commands::get_history_state,
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
            session::commands::retry_runtime_projection,
            session::commands::restore_sample_pads,
            session::commands::play_timeline,
            session::commands::stop_timeline,
            session::commands::go_to_start_timeline,
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
            recording::commands::start_arrange_recording,
            recording::commands::record_another_take,
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
    use super::{bootstrap_recovery_candidates, safe_mode_from_args};
    use crate::host_commands::{NativeAudioProbe, parse_stdout};
    use crate::model::DeviceChannels;
    use crate::storage::SessionStore;
    use riffra_core::CreativeSession;

    #[test]
    fn parses_audio_probe_with_unicode_device_names() {
        let probe = parse_stdout::<NativeAudioProbe>(
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","accessMode":"driverManaged","devicePairing":"sameDevice","inputs":[{"name":"Focusrite","channels":[{"index":0,"name":"Input 1"}]}],"outputs":[{"name":"Focusrite","channels":[{"index":0,"name":"Output 1"}]}]},{"name":"WASAPI","accessMode":"shared","devicePairing":"independent","inputs":[],"outputs":[]}]}"#,
            "audioDeviceProbe",
        )
        .unwrap();
        assert_eq!(probe.drivers[0].name, "ASIO");
        assert_eq!(probe.drivers[0].inputs[0].name, "Focusrite");
        assert_eq!(probe.drivers[0].inputs[0].channels[0].name, "Input 1");
        assert_eq!(probe.drivers[0].outputs[0].channels[0].index, 0);
        assert_eq!(probe.drivers[0].outputs[0].channels[0].name, "Output 1");
        assert_eq!(
            probe.drivers[1].device_pairing,
            crate::model::AudioDevicePairing::Independent
        );
        assert!(probe.drivers[1].inputs.is_empty());
        assert!(probe.drivers[1].outputs.is_empty());
    }

    #[test]
    fn parses_device_channels_detail() {
        let detail = parse_stdout::<DeviceChannels>(
            br#"{"type":"deviceChannels","driver":"ASIO","inputDevice":"Focusrite","inputChannels":[{"index":0,"name":"Analogue 1"}],"outputDevice":"Focusrite","outputChannels":[{"index":0,"name":"Output 1"}]}"#,
            "deviceChannels",
        )
        .unwrap();
        assert_eq!(detail.driver, "ASIO");
        assert_eq!(detail.input_channels[0].name, "Analogue 1");
        assert_eq!(detail.output_channels[0].name, "Output 1");
    }

    #[test]
    fn rejects_non_probe_messages() {
        let error = parse_stdout::<DeviceChannels>(br#"{"type":"audioStatus"}"#, "deviceChannels")
            .unwrap_err();
        assert!(error.contains("readable"));
    }

    #[test]
    fn recognizes_safe_mode_only_from_explicit_flag() {
        assert!(safe_mode_from_args(["riffra.exe", "--safe-mode"]));
        assert!(safe_mode_from_args(["--SAFE-MODE"]));
        assert!(!safe_mode_from_args(["riffra.exe", "--serve"]));
    }

    #[test]
    fn bootstrap_lists_recovery_candidates_only_after_recovery() {
        // Arrange
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("riffra-bootstrap-recovery-{nonce}"));
        let store = SessionStore::new(&root);
        store.ensure_layout().unwrap();
        let payload = serde_json::to_vec(&CreativeSession::new(1_000)).unwrap();
        std::fs::write(root.join("scratch/generations/1-1.json"), payload).unwrap();

        // Act
        let normal = bootstrap_recovery_candidates(&root, false).unwrap();
        let recovered = bootstrap_recovery_candidates(&root, true).unwrap();

        // Assert
        assert!(normal.is_empty());
        assert_eq!(recovered.len(), 1);
        let _ = std::fs::remove_dir_all(root);
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
