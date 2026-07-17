//! Thin Tauri command boundary for Analysis jobs and the on-demand analyzer.
//!
//! Both commands delegate to [`super::analyze_with_cancel`] / [`super::analyze`]
//! after resolving the AssetId to a validated audio path. Shared job lifecycle
//! helpers live in [`crate::jobs`].

use tauri::State;

use crate::AppState;
use crate::analysis::{self, AudioAnalysis};
use crate::asset;
use crate::jobs::{self, JobStatus};

#[tauri::command]
pub fn start_analysis_job(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<JobStatus, String> {
    let path = asset::resolve_audio_path(&state.data_root, &asset_id)?;
    let (id, status) = state.jobs.start("analysis");
    let registry = state.jobs.clone();
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        registry.set_running(&id, "Analyzing audio in the background.");
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return;
        };
        let result = analysis::analyze_with_cancel(&path, Some(cancelled.as_ref()));
        match result {
            Ok(result) => match jobs::serialize_result(&result) {
                Ok(value) => registry.complete(&id, value, "Audio analysis completed."),
                Err(message) => jobs::fail(&registry, &data_root, &id, message),
            },
            Err(message) => jobs::fail(&registry, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
pub async fn analyze_asset(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<AudioAnalysis, String> {
    let path = asset::resolve_audio_path(&state.data_root, &asset_id)?;
    tauri::async_runtime::spawn_blocking(move || analysis::analyze(&path))
        .await
        .map_err(|error| format!("Audio analysis task failed: {error}"))?
}
