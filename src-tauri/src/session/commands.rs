//! Thin Tauri command boundary for Session Application Operations.

use tauri::State;

use crate::AppState;
use crate::asset::AssetId;
use crate::model::AudioStatus;
use crate::session::CreativeSession;
use crate::session::application::{self, SessionContext};

fn context<'a>(state: &'a State<'_, AppState>) -> SessionContext<'a> {
    SessionContext {
        audio: &state.audio,
        data_root: &state.data_root,
        session: &state.session,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn create_sample_pad(
    asset_id: String,
    name: String,
    duration_ms: u64,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::create_sample_pad(&context(&state), asset_id, name, duration_ms)
}

#[tauri::command]
pub fn update_sample_pad(
    pad_id: String,
    patch: application::SamplePadPatch,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    application::update_sample_pad(&context(&state), &pad_id, &patch)
}

#[tauri::command]
pub fn remove_sample_pad(
    pad_id: String,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    application::remove_sample_pad(&context(&state), &pad_id)
}
