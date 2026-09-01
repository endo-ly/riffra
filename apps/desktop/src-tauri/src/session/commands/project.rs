use super::*;

use crate::model::ProjectState;

#[tauri::command]
pub async fn undo_session(app: AppHandle) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "undo", json!({})).await
}

#[tauri::command]
pub async fn redo_session(app: AppHandle) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "redo", json!({})).await
}

#[tauri::command]
pub async fn get_history_state(app: AppHandle) -> Result<HistoryState, String> {
    dispatch(app, "history.get", json!({})).await
}

#[tauri::command]
pub async fn restore_recovery_generation(
    file_name: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "project.restore-generation",
        json!({ "fileName": file_name }),
    )
    .await
}

#[tauri::command]
pub async fn import_project(path: String, app: AppHandle) -> Result<ProjectState, String> {
    dispatch(app, "project.import", json!({ "path": path })).await
}

#[tauri::command]
pub async fn list_projects(app: AppHandle) -> Result<ProjectState, String> {
    dispatch(app, "project.list", json!({})).await
}

#[tauri::command]
pub async fn create_project(name: Option<String>, app: AppHandle) -> Result<ProjectState, String> {
    dispatch(app, "project.create", json!({ "name": name })).await
}

#[tauri::command]
pub async fn open_project(project_id: String, app: AppHandle) -> Result<ProjectState, String> {
    dispatch(app, "project.open", json!({ "projectId": project_id })).await
}

#[tauri::command]
pub async fn rename_project(name: String, app: AppHandle) -> Result<ProjectState, String> {
    dispatch(app, "project.rename", json!({ "name": name })).await
}

#[tauri::command]
pub async fn update_session_settings(
    patch: SessionSettingsPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "session.settings.update", patch).await
}

#[tauri::command]
pub async fn set_master_gain_db(gain_db: f64, app: AppHandle) -> Result<SessionAudioPair, String> {
    dispatch(app, "audio.master-gain.set", json!({ "gainDb": gain_db })).await
}

#[tauri::command]
pub async fn relink_missing_dependency(
    asset_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    dispatch(
        app,
        "missing.relink",
        json!({ "assetId": asset_id, "newPath": new_path }),
    )
    .await
}

#[tauri::command]
pub async fn disable_missing_plugin(
    device_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "missing.disable-plugin",
        json!({ "deviceId": device_id }),
    )
    .await
}

#[tauri::command]
pub async fn replace_missing_track_plugin(
    device_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "missing.replace-plugin",
        json!({ "deviceId": device_id, "newPath": new_path }),
    )
    .await
}

#[tauri::command]
pub async fn get_missing_dependencies(app: AppHandle) -> Result<Vec<MissingDependency>, String> {
    dispatch(app, "missing.list", json!({})).await
}
