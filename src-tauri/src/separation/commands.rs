//! Thin Tauri command boundary for Separation jobs and listing results.
//!
//! The separation job delegates to [`super::separate_asset_with_cancel`] after
//! validating the source Asset registration. [`super::list`] is a pure query
//! over previously produced separation outputs.

use tauri::State;

use crate::AppState;
use crate::asset::{self, AssetId};
use crate::jobs::{self, BackgroundJobStatus, JobKind};
use crate::separation::{self, SeparationResult};
use crate::storage::now_ms;

#[tauri::command]
pub fn start_separation_job(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<BackgroundJobStatus, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    asset::load(&state.data_root, &asset_id)
        .ok_or_else(|| format!("Source asset is not registered: {asset_id}"))?;
    let (id, status) = state.jobs.start(JobKind::Separation);
    let registry = state.jobs.clone();
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        registry.set_running(&id, "Separating stereo channels in the background.");
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return;
        };
        let result = separation::separate_asset_with_cancel(
            &data_root,
            &asset_id,
            now_ms(),
            Some(cancelled.as_ref()),
        );
        match result {
            Ok(result) => match jobs::serialize_result(&result) {
                Ok(value) => registry.complete(&id, value, "Separation completed."),
                Err(message) => jobs::fail(&registry, &data_root, &id, message),
            },
            Err(message) => jobs::fail(&registry, &data_root, &id, message),
        }
    });
    jobs::to_background_status(status)
}

#[tauri::command]
pub fn list_separations(state: State<'_, AppState>) -> Result<Vec<SeparationResult>, String> {
    separation::list(&state.data_root)
}
