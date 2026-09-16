//! Tauri adapters for Host-owned Library read model operations.

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::library::LibraryAsset;
use crate::library::{InstrumentCollection, InstrumentLibraryItem};
use crate::{AppState, NativeCommandError};

async fn dispatch<T: serde::de::DeserializeOwned + Send + 'static>(
    app: AppHandle,
    command: &'static str,
    params: serde_json::Value,
) -> Result<T, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .host_connection
            .dispatch(command, params)
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("Library operation failed: {error}"))
    })?
}

#[tauri::command]
pub async fn search_library(
    query: String,
    app: AppHandle,
) -> Result<Vec<LibraryAsset>, NativeCommandError> {
    dispatch(app, "library.search", json!({ "query": query })).await
}

#[tauri::command]
pub async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    app: AppHandle,
) -> Result<LibraryAsset, NativeCommandError> {
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
) -> Result<Vec<LibraryAsset>, NativeCommandError> {
    dispatch(app, "library.related", json!({ "id": id })).await
}

#[tauri::command]
pub async fn list_instruments(
    app: AppHandle,
) -> Result<Vec<InstrumentLibraryItem>, NativeCommandError> {
    dispatch(app, "library.instrument.list", json!({})).await
}

#[tauri::command]
pub async fn set_instrument_favorite(
    instrument_id: String,
    favorite: bool,
    app: AppHandle,
) -> Result<InstrumentLibraryItem, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.favorite.set",
        json!({ "instrumentId": instrument_id, "favorite": favorite }),
    )
    .await
}

#[tauri::command]
pub async fn set_instrument_category_override(
    instrument_id: String,
    category: Option<String>,
    app: AppHandle,
) -> Result<InstrumentLibraryItem, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.category.set",
        json!({ "instrumentId": instrument_id, "category": category }),
    )
    .await
}

#[tauri::command]
pub async fn set_instrument_user_tags(
    instrument_id: String,
    tags: Vec<String>,
    app: AppHandle,
) -> Result<InstrumentLibraryItem, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.tags.set",
        json!({ "instrumentId": instrument_id, "tags": tags }),
    )
    .await
}

#[tauri::command]
pub async fn list_instrument_collections(
    app: AppHandle,
) -> Result<Vec<InstrumentCollection>, NativeCommandError> {
    dispatch(app, "library.instrument.collection.list", json!({})).await
}

#[tauri::command]
pub async fn create_instrument_collection(
    name: String,
    app: AppHandle,
) -> Result<InstrumentCollection, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.collection.create",
        json!({ "name": name }),
    )
    .await
}

#[tauri::command]
pub async fn rename_instrument_collection(
    id: i64,
    name: String,
    app: AppHandle,
) -> Result<InstrumentCollection, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.collection.rename",
        json!({ "id": id, "name": name }),
    )
    .await
}

#[tauri::command]
pub async fn delete_instrument_collection(
    id: i64,
    app: AppHandle,
) -> Result<(), NativeCommandError> {
    dispatch::<serde_json::Value>(
        app,
        "library.instrument.collection.delete",
        json!({ "id": id }),
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn set_instrument_collection_membership(
    collection_id: i64,
    instrument_id: String,
    included: bool,
    app: AppHandle,
) -> Result<InstrumentLibraryItem, NativeCommandError> {
    dispatch(
        app,
        "library.instrument.collection.membership.set",
        json!({
            "collectionId": collection_id,
            "instrumentId": instrument_id,
            "included": included,
        }),
    )
    .await
}
