//! Tauri adapter that stages WebView-supplied MIDI bytes for Host import.

use riffra_control::new_instance_id;
use riffra_runtime::api::params::AssetImportParams;
use riffra_runtime::api::{CanonicalCommand, ControlOutput};
use std::io::Write;
use tauri::{AppHandle, Manager};

use crate::{AppState, NativeCommandError};

/// Writes MIDI bytes to a private staging file, imports it as an Asset, and
/// removes the staging file.
#[tauri::command]
pub async fn import_midi_bytes(
    name: String,
    bytes: Vec<u8>,
    app: AppHandle,
) -> Result<riffra_core::AssetId, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let staging = std::env::temp_dir().join(format!("riffra-midi-{}.mid", new_instance_id()));
        let write_result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .and_then(|mut file| file.write_all(&bytes));
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&staging);
            return Err(NativeCommandError::command_failed(format!(
                "MIDI staging file could not be written: {error}"
            )));
        }
        let result = app.state::<AppState>().host_connection.dispatch(
            CanonicalCommand::AssetImportMidi(AssetImportParams {
                path: staging.clone(),
                name: Some(name),
            })
            .into(),
        );
        let _ = std::fs::remove_file(&staging);
        match result? {
            ControlOutput::AssetId(asset_id) => Ok(asset_id),
            _ => Err(NativeCommandError::command_failed(
                "Host returned a non-Asset result for asset.import-midi",
            )),
        }
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("MIDI import task failed: {error}"))
    })?
}
