//! Tauri boundary for offline timeline rendering.

use tauri::State;

use crate::AppState;
use crate::render::{self, RenderOptions, RenderResult};
use crate::storage::now_ms;

#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<RenderResult, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    let data_root = state.data_root.clone();
    let created_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        render::render_timeline_with_options(
            &data_root,
            &session,
            created_at_ms,
            options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|error| format!("Timeline render task failed: {error}"))?
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    tracing::error!(%message, "aborting to prevent corrupted state propagation");
    std::process::abort();
}
