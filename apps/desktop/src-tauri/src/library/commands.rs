//! Tauri adapters for Host-owned Library read model operations.

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::library::LibraryAsset;

async fn dispatch<T: serde::de::DeserializeOwned + Send + 'static>(
    app: AppHandle,
    command: &'static str,
    params: serde_json::Value,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch(command, params)
    })
    .await
    .map_err(|error| format!("Library operation failed: {error}"))?
}

#[tauri::command]
pub async fn search_library(query: String, app: AppHandle) -> Result<Vec<LibraryAsset>, String> {
    dispatch(app, "library.search", json!({ "query": query })).await
}

#[tauri::command]
pub async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    app: AppHandle,
) -> Result<LibraryAsset, String> {
    dispatch(
        app,
        "library.asset.update",
        json!({ "id": id, "tag": tag, "note": note }),
    )
    .await
}

#[tauri::command]
pub async fn related_library_assets(
    id: String,
    app: AppHandle,
) -> Result<Vec<LibraryAsset>, String> {
    dispatch(app, "library.related", json!({ "id": id })).await
}
