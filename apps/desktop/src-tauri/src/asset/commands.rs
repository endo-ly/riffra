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

/// Imports an external Standard MIDI File as a canonical MIDI Asset. Runs on a
/// blocking task because canonical registration touches SQLite and the
/// filesystem. Returns the freshly minted AssetId; the session is not mutated.
#[tauri::command]
pub async fn import_midi_file(
    path: String,
    name: Option<String>,
    state: State<'_, AppState>,
) -> Result<AssetId, String> {
    let data_root = state.core.data_root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        application::import_midi_asset(&data_root, &path, name.as_deref())
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
    state: State<'_, AppState>,
) -> Result<AssetId, String> {
    let data_root = state.core.data_root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        application::import_midi_bytes(&data_root, &name, &bytes)
    })
    .await
    .map_err(|error| format!("MIDI import task failed: {error}"))?
}
