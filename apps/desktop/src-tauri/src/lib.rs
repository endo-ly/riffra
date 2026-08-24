//! Tauri Application Composition Root.
//!
//! `lib.rs` deliberately hosts only:
//!
//! - `mod` declarations,
//! - the `AppState` struct containing the shared Host,
//! - the Tauri `setup` hook that creates and registers the shared Host,
//! - the `invoke_handler` registration that wires Tauri commands to their
//!   feature-level implementations,
//! - startup state construction and the invoke registration table.
//!
//! The Runtime crate owns the live DAW services. This crate contains only
//! Tauri command adapters, Desktop bootstrap DTOs, resource-path resolution,
//! and the event sink that forwards Host events to the WebView.

mod analysis;
mod asset;
mod audio_preferences;
mod diagnostics;
mod host_commands;
mod jobs;
mod library;
mod missing;
mod model;
mod plugin_catalog;
mod plugins;
mod projects;
mod recording;
mod render;
mod session;
#[cfg(test)]
mod types;

use host_commands::*;
use model::{AudioDeviceProbe, AudioStatus, BootstrapState, RecoveryCandidate};
use riffra_runtime::{DawHost, HostConfig, HostEvent, HostEventSink, RuntimeBinaries};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    host: DawHost,
}

impl AppState {
    pub(crate) fn with_host_lifecycle<T, F>(&self, operation: F) -> Result<T, String>
    where
        F: FnOnce(&Self) -> Result<T, String>,
    {
        self.host.with_lifecycle(|| operation(self))
    }
}

fn default_vst3_root() -> String {
    #[cfg(windows)]
    {
        r"C:\Program Files\Common Files\VST3".into()
    }
    #[cfg(not(windows))]
    {
        "/usr/lib/vst3".into()
    }
}

struct TauriHostEventSink {
    app: AppHandle,
}

impl HostEventSink for TauriHostEventSink {
    fn emit(&self, event: HostEvent) {
        let result = match event {
            HostEvent::CanonicalStateChanged(value) => {
                self.app.emit("canonical-state-changed", value)
            }
            HostEvent::RuntimeStartupFinished { succeeded } => self.app.emit(
                "runtime-startup-finished",
                serde_json::json!({"succeeded": succeeded}),
            ),
            HostEvent::RuntimeProjectionStatus(value) => {
                self.app.emit("runtime-projection-status", value)
            }
            HostEvent::AudioStatus(value) => self.app.emit("audio-status", value),
            HostEvent::AudioMeters(value) => self.app.emit("audio-meters", value),
            HostEvent::TransportStatus(value) => self.app.emit("transport-status", value),
            HostEvent::RuntimeRestarted { generation } => self.app.emit(
                "runtime-restarted",
                serde_json::json!({"generation": generation}),
            ),
            HostEvent::TrackPluginStateChanged(value) => {
                self.app.emit("track-plugin-state-changed", value)
            }
            HostEvent::TrackPluginParameterChanged(value) => {
                self.app.emit("track-plugin-parameter-changed", value)
            }
        };
        if let Err(error) = result {
            tracing::debug!(error = %error, "Tauri Host event could not be delivered");
        }
    }
}

fn map_recovery_candidates(
    candidates: Vec<riffra_host::RecoveryCandidate>,
) -> Vec<RecoveryCandidate> {
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
}

fn bootstrap_recovery_candidates(
    host: &DawHost,
    recovered_from_generation: bool,
) -> Result<Vec<RecoveryCandidate>, String> {
    if !recovered_from_generation {
        return Ok(Vec::new());
    }
    host.recovery_candidates()
        .map(map_recovery_candidates)
        .map_err(|error| format!("Recovery candidates could not be read: {error}"))
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

fn monitor_shutdown_request(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("riffra-desktop-shutdown".into())
        .spawn(move || {
            loop {
                let requested = app
                    .try_state::<AppState>()
                    .is_some_and(|state| state.host.shutdown_requested());
                if requested {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.host.shutdown();
                    }
                    app.exit(0);
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed)
                && let Some(state) = window.try_state::<AppState>()
            {
                // The window is actually gone; the frontend has already had
                // its chance to flush plugin state before calling destroy().
                // Shutting the shared Host down only now keeps the runtime
                // alive while a close request is still cancellable.
                state.host.shutdown();
            }
        })
        .setup(|app| {
            let safe_mode = safe_mode_requested();
            let data_root = app.path().app_data_dir().map_err(|error| {
                format!("Windows application data folder is unavailable: {error}")
            })?;
            let binaries = RuntimeBinaries::beside_current_executable()?;
            let host = DawHost::open(
                HostConfig {
                    data_root: data_root.clone(),
                    safe_mode,
                    binaries: binaries.clone(),
                },
                Arc::new(TauriHostEventSink {
                    app: app.handle().clone(),
                }),
            )
            .map_err(|error| error.to_string())?;
            app.manage(AppState { host });
            monitor_shutdown_request(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            export_scratch_session,
            get_background_job,
            cancel_background_job,
            probe_audio_devices,
            probe_device_channels,
            get_audio_status,
            preview_master_gain_db,
            set_emergency_mute,
            recover_audio_device,
            retry_startup_runtime,
            enable_midi_listening,
            disable_midi_listening,
            session::commands::send_midi_to_track,
            session::commands::panic_midi_track,
            stop_preview,
            // Session Application Operations.
            session::commands::undo_session,
            session::commands::redo_session,
            session::commands::get_history_state,
            session::commands::restore_recovery_generation,
            session::commands::import_scratch_session,
            session::commands::add_audio_clip_to_arrangement,
            session::commands::create_midi_clip,
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
            session::commands::get_runtime_projection_status,
            session::commands::retry_runtime_projection,
            session::commands::play_timeline,
            session::commands::stop_timeline,
            session::commands::go_to_start_timeline,
            session::commands::seek_timeline,
            session::commands::update_arrangement_timebase,
            session::commands::update_timeline_loop_range,
            session::commands::update_timeline_punch_range,
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
            session::commands::insert_midi_notes,
            session::commands::update_midi_note,
            session::commands::update_midi_notes,
            session::commands::remove_midi_note,
            session::commands::remove_midi_notes,
            session::commands::quantize_midi_notes,
            session::commands::transform_midi_notes,
            session::commands::duplicate_midi_notes,
            session::commands::set_audio_clip_take_variant,
            session::commands::start_take_comparison,
            session::commands::switch_take_comparison_variant,
            session::commands::stop_take_comparison,
            session::commands::activate_take,
            session::commands::place_take_as_separate_clip,
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
            analysis::commands::analyze_asset,
            render::commands::render_timeline,
            plugins::commands::scan_vst3_folder,
            plugins::commands::start_scan_job
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}

#[cfg(test)]
mod tests {
    use super::{map_recovery_candidates, safe_mode_from_args};
    use riffra_core::CreativeSession;
    use riffra_host::SessionStore;

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
        let normal = map_recovery_candidates(Vec::new());
        let recovered = map_recovery_candidates(store.recovery_candidates().unwrap());

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
