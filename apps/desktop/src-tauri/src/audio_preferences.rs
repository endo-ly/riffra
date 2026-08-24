//! Tauri boundary for the shared Host audio-preference workflow.

use crate::{AppState, model::AudioStatus};
use tauri::State;

pub(crate) use riffra_runtime::AudioDriverConfig;

#[tauri::command]
pub fn set_audio_driver(
    config: AudioDriverConfig,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    state
        .host
        .set_audio_driver(config)
        .map_err(|error| error.to_string())
}
