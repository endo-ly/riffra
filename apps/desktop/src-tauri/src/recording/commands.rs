//! Tauri adapters for Host-owned Recording operations.

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::library::LibraryAsset;
use crate::model::{AudioStatus, RecordingStopResult};
use crate::recording::RecordingAsset;
use crate::{AppState, NativeCommandError};

async fn dispatch<T: serde::de::DeserializeOwned + Send + 'static>(
    app: AppHandle,
    command: &'static str,
    params: serde_json::Value,
) -> Result<T, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch(command, params)
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("Recording operation failed: {error}"))
    })?
}

#[tauri::command]
pub async fn list_recordings(
    query: Option<String>,
    app: AppHandle,
) -> Result<Vec<RecordingAsset>, NativeCommandError> {
    dispatch(app, "record.list", json!({ "query": query })).await
}

#[tauri::command]
pub async fn rename_recording(
    id: String,
    new_name: String,
    app: AppHandle,
) -> Result<String, NativeCommandError> {
    dispatch(
        app,
        "record.rename",
        json!({ "id": id, "newName": new_name }),
    )
    .await
}

#[tauri::command]
pub async fn delete_recording(id: String, app: AppHandle) -> Result<(), NativeCommandError> {
    dispatch(app, "record.delete", json!({ "id": id })).await
}

#[tauri::command]
pub async fn archive_recording(id: String, app: AppHandle) -> Result<String, NativeCommandError> {
    dispatch(app, "record.archive", json!({ "id": id })).await
}

#[tauri::command]
pub async fn promote_recording(id: String, app: AppHandle) -> Result<String, NativeCommandError> {
    dispatch(app, "record.promote", json!({ "id": id })).await
}

#[tauri::command]
pub async fn detect_duplicate_recordings(
    app: AppHandle,
) -> Result<Vec<Vec<String>>, NativeCommandError> {
    dispatch(app, "record.duplicates", json!({})).await
}

#[tauri::command]
pub async fn tag_recording(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    app: AppHandle,
) -> Result<LibraryAsset, NativeCommandError> {
    dispatch(
        app,
        "record.tag",
        json!({ "id": id, "tag": tag, "note": note }),
    )
    .await
}

#[tauri::command]
pub async fn start_arrange_recording(app: AppHandle) -> Result<AudioStatus, NativeCommandError> {
    dispatch(app, "record.start", json!({ "recordingSessionId": null })).await
}

#[tauri::command]
pub async fn record_another_take(
    recording_session_id: String,
    app: AppHandle,
) -> Result<AudioStatus, NativeCommandError> {
    dispatch(
        app,
        "record.start",
        json!({ "recordingSessionId": recording_session_id }),
    )
    .await
}

#[tauri::command]
pub async fn stop_arrange_recording(
    app: AppHandle,
) -> Result<RecordingStopResult, NativeCommandError> {
    dispatch(app, "record.stop", json!({})).await
}
