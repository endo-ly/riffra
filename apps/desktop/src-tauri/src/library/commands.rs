//! Thin Tauri command boundary for Library Read Model queries and updates.
//!
//! Each command is a single delegation to [`super::search`] /
//! [`super::update_metadata`] / [`super::related`] over the Library Read Model.
//! They do not span Domain / Persistence / Runtime, so they live here as thin
//! wrappers rather than in an `application.rs`.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::library::{self, LibraryAsset};

#[tauri::command]
pub async fn search_library(query: String, app: AppHandle) -> Result<Vec<LibraryAsset>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| library::search(state.host.data_root(), &query))
    })
    .await
    .map_err(|error| format!("Library search task failed: {error}"))?
}

#[tauri::command]
pub async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    app: AppHandle,
) -> Result<LibraryAsset, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            library::update_metadata(state.host.data_root(), &id, tag, note)
        })
    })
    .await
    .map_err(|error| format!("Library metadata task failed: {error}"))?
}

#[tauri::command]
pub async fn related_library_assets(
    id: String,
    app: AppHandle,
) -> Result<Vec<LibraryAsset>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| library::related(state.host.data_root(), &id))
    })
    .await
    .map_err(|error| format!("Related asset task failed: {error}"))?
}
