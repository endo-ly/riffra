//! Tauri adapters for Host-owned Asset operations.

use riffra_control::new_instance_id;
use riffra_core::AssetId;
use serde_json::json;
use std::io::Write;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::asset::application::AssetPreviewOptions;
use crate::model::AudioStatus;

#[tauri::command]
pub async fn preview_asset(
    asset_id: String,
    options: AssetPreviewOptions,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().host_connection.dispatch(
            "asset.preview",
            json!({
                "assetId": asset_id,
                "startMs": options.start_ms,
                "endMs": options.end_ms,
                "looped": options.looped,
                "gain": options.gain,
            }),
        )
    })
    .await
    .map_err(|error| format!("Asset operation failed: {error}"))?
}

#[tauri::command]
pub async fn import_midi_file(
    path: String,
    name: Option<String>,
    app: AppHandle,
) -> Result<AssetId, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch("asset.import-midi", json!({ "path": path, "name": name }))
    })
    .await
    .map_err(|error| format!("MIDI import task failed: {error}"))?
}

#[tauri::command]
pub async fn import_midi_bytes(
    name: String,
    bytes: Vec<u8>,
    app: AppHandle,
) -> Result<AssetId, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let staging = std::env::temp_dir().join(format!("riffra-midi-{}.mid", new_instance_id()));
        let write_result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .and_then(|mut file| file.write_all(&bytes));
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&staging);
            return Err(format!("MIDI staging file could not be written: {error}"));
        }
        let result = app.state::<AppState>().host_connection.dispatch(
            "asset.import-midi",
            json!({ "path": staging, "name": name }),
        );
        let _ = std::fs::remove_file(&staging);
        result
    })
    .await
    .map_err(|error| format!("MIDI import task failed: {error}"))?
}
