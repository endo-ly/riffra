//! Tauri boundary for the shared Host audio-preference workflow.

use crate::{AppState, model::AudioStatus};
use tauri::{AppHandle, Manager};

pub(crate) use riffra_runtime::AudioDriverConfig;

#[tauri::command]
pub async fn set_audio_driver(
    config: AudioDriverConfig,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().host_connection.dispatch(
            "audio.driver.set",
            serde_json::to_value(config).map_err(|e| e.to_string())?,
        )
    })
    .await
    .map_err(|error| format!("Audio driver operation failed: {error}"))?
}
