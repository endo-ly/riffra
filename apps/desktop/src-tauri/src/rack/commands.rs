//! Thin Tauri command boundary for Rack Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`RackContext`](super::application::RackContext) of concrete dependencies,
//! delegates to the matching Application Operation, and returns the resulting
//! DTO. The production workflow (runtime apply, session commit, rollback,
//! RackDefinition Asset round-trip) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::asset;
use crate::asset::AssetId;
use crate::library::LibraryAsset;
use crate::model::{AudioStatus, SessionAudioPair};
use crate::rack::application::{self, RackContext};

async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Rack blocking operation failed: {error}"))?
}

fn app_context(state: &AppState) -> RackContext<'_> {
    RackContext {
        audio: state.core.audio(),
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: state.core.safe_mode(),
    }
}

#[tauri::command]
pub async fn load_plugin_into_rack(
    path: String,
    parameter_values: Vec<f32>,
    bypassed: bool,
    state_data: Option<String>,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::load_plugin_into_rack(
            &app_context(state),
            &path,
            &parameter_values,
            bypassed,
            state_data.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn clear_plugin_from_rack(app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, |state| {
        application::clear_plugin_from_rack(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn open_plugin_editor(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        application::open_plugin_editor(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn set_rack_plugin_bypassed(
    bypassed: bool,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::set_rack_plugin_bypassed(&app_context(state), bypassed)
    })
    .await
}

#[tauri::command]
pub async fn set_rack_plugin_parameter(
    index: u32,
    value: f32,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::set_rack_plugin_parameter(&app_context(state), index, value)
    })
    .await
}

#[tauri::command]
pub async fn set_rack_macro_value(
    macro_id: String,
    value: f32,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::set_rack_macro_value(&app_context(state), &macro_id, value)
    })
    .await
}

#[tauri::command]
pub async fn map_rack_macro(
    macro_id: String,
    parameter_index: Option<u32>,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::map_rack_macro(&app_context(state), &macro_id, parameter_index)
    })
    .await
}

#[tauri::command]
pub async fn restore_current_rack(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        application::restore_current_rack(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn recall_snapshot(slot: String, app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::recall_snapshot(&app_context(state), &slot)
    })
    .await
}

#[tauri::command]
pub async fn capture_snapshot(slot: String, app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::capture_snapshot(&app_context(state), &slot)
    })
    .await
}

#[tauri::command]
pub async fn save_rack_definition(
    name: String,
    path: String,
    app: AppHandle,
) -> Result<AssetId, String> {
    run_blocking(app, move |state| {
        application::save_rack_definition(&app_context(state), &name, &path)
    })
    .await
}

#[tauri::command]
pub async fn list_rack_definitions(app: AppHandle) -> Result<Vec<LibraryAsset>, String> {
    run_blocking(app, |state| {
        let assets = asset::list_by_kind(
            state.core.data_root(),
            crate::asset::AssetKind::RackDefinition,
        )?;
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
    })
    .await
}

#[tauri::command]
pub async fn load_rack_definition_asset(
    asset_id: String,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::load_rack_definition_asset(&app_context(state), asset_id)
    })
    .await
}
