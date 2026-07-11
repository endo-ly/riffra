mod analysis;
mod model;
mod native_audio;
mod plugins;
mod recordings;
mod separation;
mod storage;

use model::{AudioStatus, BootstrapState, MidiProbe, ScratchSession};
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
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    Ok(BootstrapState {
        session: state.session.lock().map_err(lock_error)?.clone(),
        recovered_from_generation: state.recovered_from_generation,
        data_root: state.data_root.to_string_lossy().into_owned(),
        vst3_root: DEFAULT_VST3_ROOT.into(),
    })
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
    *state.session.lock().map_err(lock_error)? = session;
    Ok(())
}

#[tauri::command]
async fn scan_vst3_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<plugins::ScanReport, String> {
    let data_root = state.data_root.clone();
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
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
        report
    }).await.map_err(|error| format!("Plugin catalog task failed: {error}"))?)
}

#[tauri::command]
fn list_recordings(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<recordings::RecordingAsset>, String> {
    recordings::list(&state.data_root, query.as_deref())
}

#[tauri::command]
async fn analyze_audio(path: String) -> Result<analysis::AudioAnalysis, String> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || analysis::analyze(&path))
        .await
        .map_err(|error| format!("Audio analysis task failed: {error}"))?
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
fn start_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
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
fn recover_audio_device(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.recover_audio_device()
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
}

#[tauri::command]
async fn probe_midi_devices(app: tauri::AppHandle) -> Result<MidiProbe, String> {
    let command = app
        .shell()
        .sidecar("riffra-audio")
        .map_err(|error| format!("MIDI probe sidecar could not be prepared: {error}"))?
        .args(["--probe"]);
    let output = command.output().await.map_err(|error| {
        format!("MIDI probe could not start; no device state was changed: {error}")
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!(
                "MIDI probe exited with code {:?}; no device state was changed.",
                output.status.code()
            )
        } else {
            format!("MIDI probe failed: {detail}")
        });
    }
    let probe = parse_midi_probe(&output.stdout)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_root = app.path().app_data_dir().map_err(|error| {
                format!("Windows application data folder is unavailable: {error}")
            })?;
            std::fs::create_dir_all(&data_root)?;
            let (mut session, recovered_from_generation) =
                SessionStore::new(&data_root).load_or_create()?;
            session.emergency_muted = true;
            app.manage(AppState {
                data_root,
                session: Mutex::new(session),
                audio: AudioSupervisor::start(app.handle()),
                recovered_from_generation,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            save_scratch_session,
            scan_vst3_folder,
            list_recordings,
            analyze_audio,
            separate_channels,
            list_separations,
            load_plugin,
            clear_plugin,
            get_audio_status,
            set_emergency_mute,
            start_recording,
            stop_recording,
            set_plugin_bypassed,
            recover_audio_device,
            probe_midi_devices
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}
mod plugin_catalog;
mod plugin_validation;

#[cfg(test)]
mod tests {
    use super::parse_midi_probe;

    #[test]
    fn parses_midi_probe_with_unicode_device_names() {
        let probe = parse_midi_probe(
            br#"{"type":"audioDeviceProbe","midiInputs":["Keyboard"],"midiOutputs":["Microsoft GS Wavetable Synth"]}"#,
        )
        .unwrap();
        assert_eq!(probe.midi_inputs, ["Keyboard"]);
        assert_eq!(probe.midi_outputs, ["Microsoft GS Wavetable Synth"]);
    }

    #[test]
    fn rejects_non_probe_messages() {
        let error = parse_midi_probe(br#"{"type":"audioStatus"}"#).unwrap_err();
        assert!(error.contains("unexpected"));
    }
}
