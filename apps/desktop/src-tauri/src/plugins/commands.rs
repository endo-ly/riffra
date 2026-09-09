//! Tauri adapters for Host-owned VST3 catalog and scan operations.

use serde_json::json;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::plugins::ScanReport;
use crate::{AppState, NativeCommandError};
use riffra_runtime::jobs::BackgroundJobStatus;

#[tauri::command]
pub async fn scan_vst3_folder(
    path: Option<String>,
    app: AppHandle,
) -> Result<ScanReport, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch("plugin.scan", json!({ "path": path.map(PathBuf::from) }))
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("Plugin scan operation failed: {error}"))
    })?
}

#[tauri::command]
pub fn start_scan_job(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<BackgroundJobStatus, NativeCommandError> {
    state.host_connection.dispatch(
        "plugin.scan.start",
        json!({ "path": path.map(PathBuf::from) }),
    )
}
