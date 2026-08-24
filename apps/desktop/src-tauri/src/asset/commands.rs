//! Thin Tauri command boundary for Asset Application Operations.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::asset::application::{self, AssetPreviewContext, AssetPreviewOptions};
use crate::model::AudioStatus;
use riffra_core::AssetId;

async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(operation)
    })
    .await
    .map_err(|error| format!("Asset blocking operation failed: {error}"))?
}

fn app_context(state: &AppState) -> AssetPreviewContext<'_> {
    AssetPreviewContext {
        audio: state.host.core().audio(),
        data_root: state.host.data_root(),
        safe_mode: state.host.core().safe_mode(),
    }
}

#[tauri::command]
pub async fn preview_asset(
    asset_id: String,
    options: AssetPreviewOptions,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::preview_asset(&app_context(state), asset_id, options)
    })
    .await
}

/// Imports an external Standard MIDI File as a canonical MIDI Asset. Runs on a
/// blocking task because canonical registration touches SQLite and the
/// filesystem. Returns the freshly minted AssetId; the session is not mutated.
#[tauri::command]
pub async fn import_midi_file(
    path: String,
    name: Option<String>,
    app: AppHandle,
) -> Result<AssetId, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            application::import_midi_asset(state.host.data_root(), &path, name.as_deref())
        })
    })
    .await
    .map_err(|error| format!("MIDI import task failed: {error}"))?
}

/// Imports a Standard MIDI File delivered as an in-memory byte payload, used by
/// HTML5 drag-and-drop where the OS file path is not exposed to the webview.
/// Runs on a blocking task because canonical registration touches SQLite and
/// the filesystem. Returns the freshly minted AssetId; the session is not
/// mutated.
#[tauri::command]
pub async fn import_midi_bytes(
    name: String,
    bytes: Vec<u8>,
    app: AppHandle,
) -> Result<AssetId, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            application::import_midi_bytes(state.host.data_root(), &name, &bytes)
        })
    })
    .await
    .map_err(|error| format!("MIDI import task failed: {error}"))?
}
