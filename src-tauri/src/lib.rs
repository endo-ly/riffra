mod analysis;
mod library;
mod midi;
mod model;
mod native_audio;
mod plugins;
mod projects;
mod recordings;
mod render;
mod separation;
mod storage;

use model::{
    AudioDeviceProbe, AudioDriverInfo, AudioStatus, BootstrapState, MidiProbe, SamplePad,
    ScratchSession,
};
use native_audio::AudioSupervisor;
use serde::Deserialize;
use std::{path::PathBuf, sync::Mutex};
use storage::{SessionStore, now_ms};
use tauri::{Manager, State};
use tauri_plugin_shell::ShellExt;

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";

struct AppState {
    data_root: PathBuf,
    session: Mutex<ScratchSession>,
    audio: AudioSupervisor,
    recovered_from_generation: bool,
    safe_mode: bool,
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    Ok(BootstrapState {
        session: state.session.lock().map_err(lock_error)?.clone(),
        recovered_from_generation: state.recovered_from_generation,
        safe_mode: state.safe_mode,
        recovery_candidates: SessionStore::new(&state.data_root)
            .recovery_candidates()
            .map_err(|error| format!("Recovery candidates could not be read: {error}"))?,
        data_root: state.data_root.to_string_lossy().into_owned(),
        vst3_root: DEFAULT_VST3_ROOT.into(),
    })
}

fn queue_session_index(data_root: &std::path::Path, session: &ScratchSession) {
    let data_root = data_root.to_path_buf();
    let session = session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = library::sync_session(&data_root, &session);
    });
}

#[tauri::command]
fn save_scratch_session(session: ScratchSession, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!(
                "Scratch Session could not be saved; the in-memory session is unchanged: {error}"
            )
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(())
}

#[tauri::command]
fn restore_recovery_generation(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<ScratchSession, String> {
    let session = SessionStore::new(&state.data_root)
        .restore_generation(&file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
fn export_scratch_session(state: State<'_, AppState>) -> Result<projects::ProjectExport, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    projects::export(&state.data_root, &session, now_ms())
}

#[tauri::command]
fn import_scratch_session(
    path: String,
    state: State<'_, AppState>,
) -> Result<ScratchSession, String> {
    let session = projects::import(&PathBuf::from(path))?;
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Imported project could not be persisted: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
async fn scan_vst3_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<plugins::ScanReport, String> {
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
    if state.safe_mode {
        let now = now_ms();
        return Ok(plugins::ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: now,
            finished_at_ms: now,
            plugins: Vec::new(),
            issues: vec![plugins::ScanIssue {
                path: root.to_string_lossy().into_owned(),
                message: "Safe Mode skipped VST3 discovery; saved project data remains available."
                    .into(),
            }],
        });
    }
    let data_root = state.data_root.clone();
    let report = tauri::async_runtime::spawn_blocking(move || plugins::discover(&root))
        .await
        .map_err(|error| {
            format!("VST3 discovery task failed; no session data was changed: {error}")
        })?;
    let mut report = plugin_validation::validate_report(app, report).await;
    report.finished_at_ms = now_ms();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = plugin_catalog::save(&data_root, &report) {
            report.issues.push(plugins::ScanIssue {
                path: data_root.to_string_lossy().into_owned(),
                message: format!("Plugin catalog could not be saved: {error}. Scan results remain usable for this session."),
            });
        }
        if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
            report.issues.push(plugins::ScanIssue {
                path: data_root.to_string_lossy().into_owned(),
                message: format!("Library index could not be updated: {error}. Scan results remain usable for this session."),
            });
        }
        report
    }).await.map_err(|error| format!("Plugin catalog task failed: {error}"))?)
}

#[tauri::command]
fn list_recordings(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<recordings::RecordingAsset>, String> {
    let assets = recordings::list(&state.data_root, query.as_deref())?;
    let _ = library::sync_recordings(&state.data_root, &assets);
    Ok(assets)
}

#[tauri::command]
async fn search_library(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<library::LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::search(&data_root, &query))
        .await
        .map_err(|error| format!("Library search task failed: {error}"))?
}

#[tauri::command]
async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<library::LibraryAsset, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        library::update_metadata(&data_root, &id, tag, note)
    })
    .await
    .map_err(|error| format!("Library metadata task failed: {error}"))?
}

#[tauri::command]
async fn related_library_assets(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<library::LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::related(&data_root, &id))
        .await
        .map_err(|error| format!("Related asset task failed: {error}"))?
}

#[tauri::command]
async fn analyze_audio(path: String) -> Result<analysis::AudioAnalysis, String> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || analysis::analyze(&path))
        .await
        .map_err(|error| format!("Audio analysis task failed: {error}"))?
}

#[tauri::command]
fn read_midi_events(path: String) -> Result<Vec<recordings::MidiEvent>, String> {
    recordings::read_midi_events(&PathBuf::from(path))
}

#[tauri::command]
async fn separate_channels(
    path: String,
    state: State<'_, AppState>,
) -> Result<separation::SeparationResult, String> {
    let source = PathBuf::from(path);
    let output_root = state.data_root.join("separations");
    let created_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        separation::separate_channels(&source, &output_root, created_at_ms)
    })
    .await
    .map_err(|error| format!("Separation task failed: {error}"))?
}

#[tauri::command]
fn list_separations(
    state: State<'_, AppState>,
) -> Result<Vec<separation::SeparationResult>, String> {
    separation::list(&state.data_root)
}

#[tauri::command]
async fn render_timeline(
    options: Option<render::RenderOptions>,
    state: State<'_, AppState>,
) -> Result<render::RenderResult, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    let data_root = state.data_root.clone();
    let created_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        render::render_timeline_with_options(
            &data_root,
            &session,
            created_at_ms,
            options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|error| format!("Timeline render task failed: {error}"))?
}

#[tauri::command]
fn export_midi(state: State<'_, AppState>) -> Result<midi::MidiExportResult, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    midi::export(&state.data_root, &session, now_ms())
}

#[tauri::command]
fn get_audio_status(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.status()
}

#[tauri::command]
fn set_emergency_mute(muted: bool, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    let audio = state.audio.set_emergency_mute(muted)?;
    if let Ok(mut session) = state.session.lock() {
        session.emergency_muted = muted;
    }
    Ok(audio)
}

#[tauri::command]
fn load_plugin(path: String, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to reconnect external plugins.".into());
    }
    let path = PathBuf::from(path);
    if !path.exists() || path.extension().and_then(|value| value.to_str()) != Some("vst3") {
        return Err("Only an existing .vst3 bundle can be loaded.".into());
    }
    state.audio.load_plugin(&path)
}

#[tauri::command]
fn clear_plugin(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.clear_plugin()
}

#[tauri::command]
fn preview_sample(
    path: String,
    start_ms: u64,
    end_ms: Option<u64>,
    looped: Option<bool>,
    gain: Option<f32>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks live sample preview; offline analysis and export remain available."
                .into(),
        );
    }
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("Sample preview source does not exist.".into());
    }
    state.audio.preview_sample(
        &path,
        start_ms,
        end_ms,
        looped.unwrap_or(false),
        gain.unwrap_or(1.0),
    )
}

#[tauri::command]
fn stop_preview(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_preview()
}

#[tauri::command]
fn start_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks new hardware recordings; existing Inbox assets remain available for export.".into());
    }
    let inbox = state.data_root.join("recordings").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| {
        format!("Recording Inbox could not be created; no audio was started: {error}")
    })?;
    let directory = inbox.join(format!("take-{}", now_ms()));
    let mut status = state.audio.start_recording(&directory)?;
    let provenance = state
        .session
        .lock()
        .ok()
        .map(|session| recordings::RecordingProvenance {
            recorded_at_ms: now_ms(),
            session_id: session.session_id.clone(),
            workspace: format!("{:?}", session.workspace).to_lowercase(),
            master_db: session.master_db,
            rack: session.rack.clone(),
            source: "raw DI + processed safety path".into(),
        });
    if let Some(provenance) = provenance {
        if let Err(error) = recordings::save_provenance(&directory, &provenance) {
            status.message = format!(
                "Recording started, but provenance could not be saved: {error}. Audio files remain active."
            );
        }
    } else {
        status.message =
            "Recording started, but session provenance was unavailable; audio files remain active."
                .into();
    }
    Ok(status)
}

#[tauri::command]
fn stop_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_recording()
}

#[tauri::command]
fn set_plugin_bypassed(bypassed: bool, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.set_plugin_bypassed(bypassed)
}

#[tauri::command]
fn set_plugin_parameter(
    index: u32,
    value: f32,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks external VST3 parameter changes.".into());
    }
    state.audio.set_plugin_parameter(index, value)
}

#[tauri::command]
fn set_master_gain_db(gain_db: f64, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    let audio = state.audio.set_master_gain_db(gain_db)?;
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    session.master_db = gain_db.clamp(-90.0, 0.0);
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!("Master gain changed, but the session could not be saved: {error}")
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(audio)
}

#[tauri::command]
fn recover_audio_device(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
    }
    state.audio.recover_audio_device()
}

#[tauri::command]
fn set_audio_driver(
    driver: String,
    sample_rate: Option<u32>,
    buffer_size: Option<u32>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks audio-driver changes; restart Riffra without --safe-mode first."
                .into(),
        );
    }
    if driver.trim().is_empty() {
        return Err("Audio driver name must not be empty.".into());
    }
    let driver = driver.trim().to_owned();
    if let Some(rate) = sample_rate {
        if !(8_000..=192_000).contains(&rate) {
            return Err("Audio sample rate preference is outside 8-192 kHz.".into());
        }
    }
    if let Some(buffer) = buffer_size {
        if !(16..=8192).contains(&buffer) {
            return Err("Audio buffer preference is outside 16-8192 samples.".into());
        }
    }
    let audio = state
        .audio
        .set_audio_driver(&driver, sample_rate, buffer_size)?;
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    session.audio_driver = Some(driver);
    session.audio_sample_rate = sample_rate;
    session.audio_buffer_size = buffer_size;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!("Audio driver changed, but the session preference could not be saved: {error}")
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(audio)
}

#[tauri::command]
fn open_midi_input(name: String, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks MIDI input; offline MIDI and audio export remain available.".into(),
        );
    }
    if name.trim().is_empty() {
        return Err("MIDI input name must not be empty.".into());
    }
    state.audio.open_midi_input(name.trim())
}

#[tauri::command]
fn close_midi_input(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.close_midi_input()
}

#[tauri::command]
fn configure_sample_pads(
    pads: Vec<SamplePad>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode keeps MIDI-triggered pad playback isolated.".into());
    }
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    for pad in &pads {
        if pad.asset_path.trim().is_empty() || pad.end_ms <= pad.start_ms {
            return Err(format!(
                "Sample pad '{}' has an invalid source or slice.",
                pad.name
            ));
        }
    }
    state.audio.configure_sample_pads(&pads)
}

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
            refreshed_at_ms: now_ms(),
            message: "Safe Mode skipped MIDI discovery.".into(),
        });
    }
    let probe = run_native_probe(app).await?;
    let empty = probe.midi_inputs.is_empty() && probe.midi_outputs.is_empty();
    Ok(MidiProbe {
        inputs: probe.midi_inputs,
        outputs: probe.midi_outputs,
        refreshed_at_ms: now_ms(),
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
            refreshed_at_ms: now_ms(),
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
                inputs: driver.inputs,
                outputs: driver.outputs,
            })
            .collect(),
        midi_inputs: probe.midi_inputs,
        midi_outputs: probe.midi_outputs,
        refreshed_at_ms: now_ms(),
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
            let (mut session, recovered_from_generation) =
                SessionStore::new(&data_root).load_or_create()?;
            let _ = library::sync_session(&data_root, &session);
            session.emergency_muted = true;
            let audio = if safe_mode {
                AudioSupervisor::offline(
                    "Safe Mode is active; native audio, MIDI, and external plugins remain isolated.",
                )
            } else {
                AudioSupervisor::start(app.handle())
            };
            if !safe_mode {
                if let Some(driver) = session.audio_driver.as_deref() {
                    let _ = audio.set_audio_driver(
                        driver,
                        session.audio_sample_rate,
                        session.audio_buffer_size,
                    );
                }
            }
            app.manage(AppState {
                data_root,
                session: Mutex::new(session),
                audio,
                recovered_from_generation,
                safe_mode,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            save_scratch_session,
            restore_recovery_generation,
            export_scratch_session,
            import_scratch_session,
            scan_vst3_folder,
            list_recordings,
            search_library,
            update_library_asset,
            related_library_assets,
            analyze_audio,
            read_midi_events,
            separate_channels,
            list_separations,
            render_timeline,
            export_midi,
            load_plugin,
            clear_plugin,
            preview_sample,
            stop_preview,
            probe_audio_devices,
            get_audio_status,
            set_emergency_mute,
            start_recording,
            stop_recording,
            set_plugin_bypassed,
            set_plugin_parameter,
            set_master_gain_db,
            recover_audio_device,
            set_audio_driver,
            open_midi_input,
            close_midi_input,
            configure_sample_pads,
            probe_midi_devices
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}
mod plugin_catalog;
mod plugin_validation;

#[cfg(test)]
mod tests {
    use super::{parse_midi_probe, safe_mode_from_args};

    #[test]
    fn parses_midi_probe_with_unicode_device_names() {
        let probe = parse_midi_probe(
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","inputs":["Focusrite"],"outputs":["Focusrite"]}],"midiInputs":["Keyboard"],"midiOutputs":["Microsoft GS Wavetable Synth"]}"#,
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
}
