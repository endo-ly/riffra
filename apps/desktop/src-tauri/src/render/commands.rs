//! Tauri boundary for offline timeline rendering.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::render::{RenderOptions, RenderResult};

#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    app: AppHandle,
) -> Result<RenderResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            state
                .host
                .render_timeline(options.unwrap_or_default())
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(|error| format!("Render operation failed: {error}"))?
}
