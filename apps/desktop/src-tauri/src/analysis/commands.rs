//! Thin Tauri command boundary for the on-demand audio analyzer.
//!
//! [`analyze_asset`] delegates to [`super::analyze`] after resolving the
//! AssetId to a validated audio path.

use tauri::State;

use crate::AppState;
use crate::analysis::{self, AudioAnalysis};
use crate::asset;

#[tauri::command]
pub async fn analyze_asset(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<AudioAnalysis, String> {
    let path = asset::resolve_audio_path(state.core.data_root(), &asset_id)?;
    tauri::async_runtime::spawn_blocking(move || analysis::analyze(&path))
        .await
        .map_err(|error| format!("Audio analysis task failed: {error}"))?
}
