//! Tauri adapters for Host-owned VST3 catalog and scan operations.

use serde_json::json;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::jobs::BackgroundJobStatus;
use crate::plugins::ScanReport;

#[tauri::command]
pub async fn scan_vst3_folder(path: Option<String>, app: AppHandle) -> Result<ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch("plugin.scan", json!({ "path": path.map(PathBuf::from) }))
    })
    .await
    .map_err(|error| format!("Plugin scan operation failed: {error}"))?
}

#[tauri::command]
pub fn start_scan_job(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<BackgroundJobStatus, String> {
    state.host_connection.dispatch(
        "plugin.scan.start",
        json!({ "path": path.map(PathBuf::from) }),
    )
}
