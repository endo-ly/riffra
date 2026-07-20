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
//! in `session`, library read-model queries in `library`, MIDI export in
//! `midi`, asset preview in `asset`.

mod analysis;
mod asset;
mod audio_preferences;
mod diagnostics;
mod errors;
mod jobs;
mod library;
mod midi;
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
mod separation;
mod session;
mod storage;

use model::{
    AudioDeviceProbe, AudioDriverInfo, AudioStatus, BootstrapState, MidiProbe, RecoveryCandidate,
};
use native_audio::AudioSupervisor;
use serde::Deserialize;
use session::CreativeSession;
use std::{path::PathBuf, sync::Mutex};
use storage::SessionStore;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::ShellExt;

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";

struct AppState {
    data_root: PathBuf,
    session: Mutex<CreativeSession>,
    audio: AudioSupervisor,
    audio_preferences: Mutex<audio_preferences::AudioPreferences>,
    recovered_from_generation: bool,
    safe_mode: bool,
    jobs: jobs::JobRegistry,
}

/// Refreshes the Library Read Model after a Production Operation has changed
/// the canonical CreativeSession. Feature modules call this instead of
/// re-implementing the spawn_blocking + sync_session fan-out.
pub(crate) fn queue_session_index(data_root: &std::path::Path, session: &CreativeSession) {
    let data_root = data_root.to_path_buf();
    let session = session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = library::sync_session(&data_root, &session);
    });
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    Ok(BootstrapState {
        session: state.session.lock().map_err(lock_error)?.clone(),
        recovered_from_generation: state.recovered_from_generation,
        safe_mode: state.safe_mode,
        native_available: true,
        recovery_candidates: SessionStore::new(&state.data_root)
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
        data_root: state.data_root.to_string_lossy().into_owned(),
        vst3_root: DEFAULT_VST3_ROOT.into(),
    })
}

#[tauri::command]
fn export_scratch_session(state: State<'_, AppState>) -> Result<projects::ProjectExport, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    projects::export(&state.data_root, &session, storage::now_ms())
}

#[tauri::command]
fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::JobStatus>, String> {
    Ok(state.jobs.status(&id))
}

#[tauri::command]
fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::JobStatus>, String> {
    Ok(state.jobs.cancel(&id))
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
    midi_inputs: Vec<String>,
    #[serde(default)]
    midi_outputs: Vec<String>,
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
    if state.safe_mode {
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
    if state.safe_mode {
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
async fn get_audio_status(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.refresh_meters()
}

#[tauri::command]
fn preview_master_gain_db(gain_db: f64, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    state.audio.set_master_gain_db(gain_db)
}

#[tauri::command]
fn set_emergency_mute(muted: bool, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.set_emergency_mute(muted)
}

#[tauri::command]
fn recover_audio_device(app: AppHandle, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
    }
    state.audio.recover_audio_device(&app)
}

#[tauri::command]
fn enable_midi_listening(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks MIDI input; offline MIDI and audio export remain available.".into(),
        );
    }
    state.audio.enable_midi_listening()
}

#[tauri::command]
fn disable_midi_listening(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.disable_midi_listening()
}

#[tauri::command]
fn send_midi_to_plugin(bytes: Vec<u8>, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks outgoing MIDI; offline MIDI and audio export remain available."
                .into(),
        );
    }
    state.audio.send_midi(&bytes)
}

#[tauri::command]
fn stop_preview(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_preview()
}

#[tauri::command]
fn stop_preview_for_key(voice_key: i32, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_preview_for_key(voice_key)
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("An internal state lock was poisoned: {error}")
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
            let effective_preferences = if safe_mode {
                preferences
            } else {
                match audio.refresh_status() {
                    Ok(status) => {
                        audio_preferences::AudioPreferences::from_effective_status(&status)?
                    }
                    Err(_) => preferences,
                }
            };
            audio_preferences::AudioPreferencesStore::new(&data_root)
                .save(&effective_preferences)?;
            audio.set_restart_preferences(effective_preferences.clone())?;
            SessionStore::new(&data_root).save(&session)?;
            let _ = library::sync_session(&data_root, &session);
            app.manage(AppState {
                data_root,
                session: Mutex::new(session),
                audio,
                audio_preferences: Mutex::new(effective_preferences),
                recovered_from_generation,
                safe_mode,
                jobs: jobs::JobRegistry::default(),
            });
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
            session::commands::update_audio_clip,
            session::commands::move_audio_clip_to_track,
            session::commands::set_audio_clip_muted,
            session::commands::set_audio_clip_loop,
            session::commands::duplicate_audio_clip,
            session::commands::split_audio_clip,
            session::commands::remove_audio_clip,
            session::commands::open_asset_in_design,
            session::commands::switch_workspace,
            session::commands::update_session_settings,
            session::commands::add_track,
            session::commands::update_track,
            session::commands::import_midi_clip,
            session::commands::update_midi_note,
            session::commands::remove_midi_note,
            session::commands::remove_midi_clip,
            session::commands::apply_ai_suggestion,
            session::commands::set_master_gain_db,
            audio_preferences::set_audio_driver,
            session::commands::relink_missing_dependency,
            session::commands::disable_missing_plugin,
            session::commands::get_missing_dependencies,
            // Asset Application Operations.
            asset::commands::preview_asset,
            // Recording Application Operations.
            recording::commands::list_recordings,
            recording::commands::rename_recording,
            recording::commands::delete_recording,
            recording::commands::archive_recording,
            recording::commands::promote_recording,
            recording::commands::detect_duplicate_recordings,
            recording::commands::tag_recording,
            recording::commands::start_recording,
            recording::commands::stop_recording,
            // Library Read Model queries / updates.
            library::commands::search_library,
            library::commands::update_library_asset,
            library::commands::related_library_assets,
            // MIDI export.
            midi::commands::export_midi,
            // Background-job orchestration per feature.
            analysis::commands::start_analysis_job,
            analysis::commands::analyze_asset,
            separation::commands::start_separation_job,
            separation::commands::list_separations,
            render::commands::start_render_job,
            render::commands::start_render_stems_job,
            render::commands::render_timeline,
            render::commands::render_timeline_stems,
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
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","accessMode":"driverManaged","devicePairing":"sameDevice","inputs":["Focusrite"],"outputs":["Focusrite"]}],"midiInputs":["Keyboard"],"midiOutputs":["Microsoft GS Wavetable Synth"]}"#,
        )
        .unwrap();
        assert_eq!(probe.drivers[0].name, "ASIO");
        assert_eq!(probe.midi_inputs, ["Keyboard"]);
        assert_eq!(probe.midi_outputs, ["Microsoft GS Wavetable Synth"]);
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
