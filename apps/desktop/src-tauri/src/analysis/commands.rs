//! Tauri adapter for the Host-owned audio analyzer.

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::analysis::AudioAnalysis;

#[tauri::command]
pub async fn analyze_asset(asset_id: String, app: AppHandle) -> Result<AudioAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch("analysis.start", json!({ "assetId": asset_id }))
    })
    .await
    .map_err(|error| format!("Audio analysis task failed: {error}"))?
}
