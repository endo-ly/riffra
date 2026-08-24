//! Thin Tauri command boundary for the on-demand audio analyzer.
//!
//! [`analyze_asset`] delegates to [`super::analyze`] after resolving the
//! AssetId to a validated audio path.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::analysis::{self, AudioAnalysis};
use crate::asset;

#[tauri::command]
pub async fn analyze_asset(asset_id: String, app: AppHandle) -> Result<AudioAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            let path = asset::resolve_audio_path(state.host.data_root(), &asset_id)?;
            analysis::analyze(&path)
        })
    })
    .await
    .map_err(|error| format!("Audio analysis task failed: {error}"))?
}
