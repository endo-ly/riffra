mod model;
mod native_audio;
mod plugins;
mod storage;

use model::{AudioStatus, BootstrapState, ScratchSession};
use native_audio::AudioSupervisor;
use std::{path::PathBuf, sync::Mutex};
use storage::{SessionStore, now_ms};
use tauri::{Manager, State};

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
fn start_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    let inbox = state.data_root.join("recordings").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| {
        format!("Recording Inbox could not be created; no audio was started: {error}")
    })?;
    let directory = inbox.join(format!("take-{}", now_ms()));
    state.audio.start_recording(&directory)
}

#[tauri::command]
fn stop_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_recording()
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
            load_plugin,
            get_audio_status,
            set_emergency_mute,
            start_recording,
            stop_recording
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}
mod plugin_catalog;
mod plugin_validation;
