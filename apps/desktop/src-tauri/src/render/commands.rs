//! Tauri boundary for offline timeline rendering.

use tauri::State;

use crate::AppState;
use crate::render::{RenderOptions, RenderResult};

#[tauri::command]
pub fn render_timeline(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<RenderResult, String> {
    state
        .host
        .render_timeline(options.unwrap_or_default())
        .map_err(|error| error.to_string())
}
