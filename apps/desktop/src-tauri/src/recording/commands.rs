//! Thin Tauri command boundary for Recording Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`RecordingContext`](super::application::RecordingContext) of concrete
//! dependencies, delegates to the matching Application Operation, and returns
//! the resulting DTO. The production workflow (audio capture lifecycle,
//! Filesystem + Asset + Library relocation) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::library;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::recording::RecordingAsset;
use crate::recording::application::{self, RecordingContext};

async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _recording_operation = state
            .recording_operation_gate
            .lock()
            .map_err(|error| format!("Recording operation lock was poisoned: {error}"))?;
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Recording blocking operation failed: {error}"))?
}

fn app_context(state: &AppState) -> RecordingContext<'_> {
    RecordingContext {
        core: &state.core,
        audio: state.core.audio(),
        runtime: &state.runtime,
        data_root: state.core.data_root(),
        safe_mode: state.core.safe_mode(),
    }
}

#[tauri::command]
pub async fn list_recordings(
    query: Option<String>,
    app: AppHandle,
) -> Result<Vec<RecordingAsset>, String> {
    run_blocking(app, move |state| {
        application::list_recordings(&app_context(state), query.as_deref())
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
        application::rename_recording(&app_context(state), &id, &new_name)
    })
    .await
}

#[tauri::command]
pub async fn delete_recording(id: String, app: AppHandle) -> Result<(), String> {
    run_blocking(app, move |state| {
        application::delete_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn archive_recording(id: String, app: AppHandle) -> Result<String, String> {
    run_blocking(app, move |state| {
        application::archive_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn promote_recording(id: String, app: AppHandle) -> Result<String, String> {
    run_blocking(app, move |state| {
        application::promote_recording(&app_context(state), &id)
    })
    .await
}

#[tauri::command]
pub async fn detect_duplicate_recordings(app: AppHandle) -> Result<Vec<Vec<String>>, String> {
    run_blocking(app, |state| {
        application::detect_duplicate_recordings(&app_context(state))
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
        application::tag_recording(&app_context(state), &id, tag, note)
    })
    .await
}

#[tauri::command]
pub async fn start_arrange_recording(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        application::start_recording(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn record_another_take(
    recording_session_id: String,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        application::record_another_take(&app_context(state), &recording_session_id)
    })
    .await
}

#[tauri::command]
pub async fn stop_arrange_recording(app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, |state| {
        application::stop_recording(&app_context(state))
    })
    .await
}
