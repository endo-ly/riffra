//! Tauri command boundary for the shared Recording Application Operations.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::library;
use crate::model::{AudioStatus, RecordingStopResult};
use crate::recording::RecordingAsset;
use riffra_runtime::recording::{self, RecordingContext, RecordingService};

async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _recording_operation = state.host.lock_recording_gate()?;
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Recording blocking operation failed: {error}"))?
}

fn app_context(state: &AppState) -> RecordingContext<'_> {
    RecordingContext {
        core: state.host.core(),
        audio: state.host.core().audio(),
        runtime: state.host.runtime(),
        data_root: state.host.data_root(),
        safe_mode: state.host.core().safe_mode(),
    }
}

#[tauri::command]
pub async fn list_recordings(
    query: Option<String>,
    app: AppHandle,
) -> Result<Vec<RecordingAsset>, String> {
    run_blocking(app, move |state| {
        recording::list_recordings(&app_context(state), query.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn rename_recording(
    id: String,
    new_name: String,
    app: AppHandle,
) -> Result<String, String> {
    run_blocking(app, move |state| {
        recording::rename_recording(&app_context(state), &id, &new_name)
    })
    .await
}

#[tauri::command]
pub async fn delete_recording(id: String, app: AppHandle) -> Result<(), String> {
    run_blocking(app, move |state| {
        recording::delete_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn archive_recording(id: String, app: AppHandle) -> Result<String, String> {
    run_blocking(app, move |state| {
        recording::archive_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn promote_recording(id: String, app: AppHandle) -> Result<String, String> {
    run_blocking(app, move |state| {
        recording::promote_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn detect_duplicate_recordings(app: AppHandle) -> Result<Vec<Vec<String>>, String> {
    run_blocking(app, |state| {
        recording::detect_duplicate_recordings(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn tag_recording(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    app: AppHandle,
) -> Result<library::LibraryAsset, String> {
    run_blocking(app, move |state| {
        recording::tag_recording(&app_context(state), &id, tag, note)
    })
    .await
}

#[tauri::command]
pub async fn start_arrange_recording(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        RecordingService {
            core: state.host.core(),
            audio: state.host.core().audio(),
            runtime: state.host.runtime(),
            data_root: state.host.data_root(),
            safe_mode: state.host.core().safe_mode(),
        }
        .start(None)
    })
    .await
}

#[tauri::command]
pub async fn record_another_take(
    recording_session_id: String,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        RecordingService {
            core: state.host.core(),
            audio: state.host.core().audio(),
            runtime: state.host.runtime(),
            data_root: state.host.data_root(),
            safe_mode: state.host.core().safe_mode(),
        }
        .start(Some(&recording_session_id))
    })
    .await
}

#[tauri::command]
pub async fn stop_arrange_recording(app: AppHandle) -> Result<RecordingStopResult, String> {
    run_blocking(app, |state| {
        RecordingService {
            core: state.host.core(),
            audio: state.host.core().audio(),
            runtime: state.host.runtime(),
            data_root: state.host.data_root(),
            safe_mode: state.host.core().safe_mode(),
        }
        .stop()
    })
    .await
}
