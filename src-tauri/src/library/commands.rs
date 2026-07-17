//! Thin Tauri command boundary for Library Read Model queries and updates.
//!
//! Each command is a single delegation to [`super::search`] /
//! [`super::update_metadata`] / [`super::related`] over the Library Read Model.
//! They do not span Domain / Persistence / Runtime, so they live here as thin
//! wrappers rather than in an `application.rs`.

use tauri::State;

use crate::AppState;
use crate::library::{self, LibraryAsset};

#[tauri::command]
pub async fn search_library(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::search(&data_root, &query))
        .await
        .map_err(|error| format!("Library search task failed: {error}"))?
}

#[tauri::command]
pub async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<LibraryAsset, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        library::update_metadata(&data_root, &id, tag, note)
    })
    .await
    .map_err(|error| format!("Library metadata task failed: {error}"))?
}

#[tauri::command]
pub async fn related_library_assets(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::related(&data_root, &id))
        .await
        .map_err(|error| format!("Related asset task failed: {error}"))?
}
