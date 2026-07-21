//! Thin Tauri command boundary for Rack Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`RackContext`](super::application::RackContext) of concrete dependencies,
//! delegates to the matching Application Operation, and returns the resulting
//! DTO. The production workflow (runtime apply, session commit, rollback,
//! RackDefinition Asset round-trip) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::State;

use crate::AppState;
use crate::asset;
use crate::asset::AssetId;
use crate::library::LibraryAsset;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::rack::application::{self, RackContext};

fn context<'a>(state: &'a State<'_, AppState>) -> RackContext<'a> {
    RackContext {
        audio: &state.audio,
        data_root: &state.data_root,
        session: &state.session,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn load_plugin_into_rack(
    path: String,
    parameter_values: Vec<f32>,
    bypassed: bool,
    state_data: Option<String>,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::load_plugin_into_rack(
        &context(&state),
        &path,
        &parameter_values,
        bypassed,
        state_data.as_deref(),
    )
}

#[tauri::command]
pub fn clear_plugin_from_rack(state: State<'_, AppState>) -> Result<SessionAudioPair, String> {
    application::clear_plugin_from_rack(&context(&state))
}

#[tauri::command]
pub fn open_plugin_editor(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    application::open_plugin_editor(&context(&state))
}

#[tauri::command]
pub fn set_rack_plugin_bypassed(
    bypassed: bool,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::set_rack_plugin_bypassed(&context(&state), bypassed)
}

#[tauri::command]
pub fn set_rack_plugin_parameter(
    index: u32,
    value: f32,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::set_rack_plugin_parameter(&context(&state), index, value)
}

#[tauri::command]
pub fn set_rack_macro_value(
    macro_id: String,
    value: f32,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::set_rack_macro_value(&context(&state), &macro_id, value)
}

#[tauri::command]
pub fn map_rack_macro(
    macro_id: String,
    parameter_index: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::map_rack_macro(&context(&state), &macro_id, parameter_index)
}

#[tauri::command]
pub fn restore_current_rack(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    application::restore_current_rack(&context(&state))
}

#[tauri::command]
pub fn recall_snapshot(
    slot: String,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::recall_snapshot(&context(&state), &slot)
}

#[tauri::command]
pub fn capture_snapshot(
    slot: String,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::capture_snapshot(&context(&state), &slot)
}

#[tauri::command]
pub fn save_rack_definition(
    name: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<AssetId, String> {
    application::save_rack_definition(&context(&state), &name, &path)
}

#[tauri::command]
pub fn list_rack_definitions(state: State<'_, AppState>) -> Result<Vec<LibraryAsset>, String> {
    let assets = asset::list_by_kind(&state.data_root, crate::asset::AssetKind::RackDefinition)?;
    Ok(assets
        .into_iter()
        .map(|asset| LibraryAsset {
            id: asset.id.as_str().to_owned(),
            name: asset.name,
            kind: "rackDefinition".into(),
            path: Some(asset.content_location),
            tag: asset.tag,
            note: asset.note,
            created_at_ms: Some(asset.created_at_ms),
            updated_at_ms: Some(asset.updated_at_ms),
            stability: "saved".into(),
        })
        .collect())
}

#[tauri::command]
pub fn load_rack_definition_asset(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::load_rack_definition_asset(&context(&state), asset_id)
}
