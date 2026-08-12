//! Tauri boundary for offline timeline rendering.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::render::{self, RenderOptions, RenderResult};
use crate::storage::now_ms;

#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    app: AppHandle,
) -> Result<RenderResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let session = state
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .session;
        render::render_timeline_with_options(
            &state.render_worker,
            state.core.data_root(),
            &session,
            now_ms(),
            options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|error| format!("Timeline render task failed: {error}"))?
}
