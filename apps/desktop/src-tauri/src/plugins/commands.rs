//! Tauri boundaries for the Host-owned VST3 catalog and scan jobs.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::jobs::BackgroundJobStatus;
use crate::plugins::ScanReport;

#[tauri::command]
pub async fn scan_vst3_folder(path: Option<String>, app: AppHandle) -> Result<ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            state
                .host
                .scan_plugins(path.map(PathBuf::from))
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(|error| format!("Plugin scan operation failed: {error}"))?
}

#[tauri::command]
pub fn start_scan_job(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<BackgroundJobStatus, String> {
    state.with_host_lifecycle(|state| {
        state
            .host
            .start_plugin_scan(path.map(PathBuf::from))
            .map_err(|error| error.to_string())
    })
}
