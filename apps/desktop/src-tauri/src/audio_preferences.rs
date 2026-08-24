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
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            state
                .host
                .set_audio_driver(config)
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(|error| format!("Audio driver operation failed: {error}"))?
}
