//! Thin Tauri command boundary for Asset Application Operations.

use tauri::State;

use crate::AppState;
use crate::asset::AssetId;
use crate::asset::application::{self, AssetPreviewContext, AssetPreviewOptions};
use crate::model::AudioStatus;

fn context<'a>(state: &'a State<'_, AppState>) -> AssetPreviewContext<'a> {
    AssetPreviewContext {
        audio: state.core.audio(),
        data_root: state.core.data_root(),
        safe_mode: state.core.safe_mode(),
    }
}

#[tauri::command]
pub fn preview_asset(
    asset_id: String,
    options: AssetPreviewOptions,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::preview_asset(&context(&state), asset_id, options)
}
