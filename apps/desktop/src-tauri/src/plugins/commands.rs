//! Tauri boundaries for the Host-owned VST3 catalog and scan jobs.

use std::path::PathBuf;

use tauri::State;

use crate::AppState;
use crate::jobs::BackgroundJobStatus;
use crate::plugins::ScanReport;

#[tauri::command]
pub fn scan_vst3_folder(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<ScanReport, String> {
    state
        .host
        .scan_plugins(path.map(PathBuf::from))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_scan_job(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<BackgroundJobStatus, String> {
    state
        .host
        .start_plugin_scan(path.map(PathBuf::from))
        .map_err(|error| error.to_string())
}
