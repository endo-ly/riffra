use super::*;

#[tauri::command]
pub async fn undo_session(app: AppHandle) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, |state| adapter::undo(&app_context(state))).await
}

#[tauri::command]
pub async fn redo_session(app: AppHandle) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, |state| adapter::redo(&app_context(state))).await
}

#[tauri::command]
pub async fn get_history_state(app: AppHandle) -> Result<HistoryState, String> {
    run_blocking_without_command_gate(app, |state| {
        let store = SessionStore::new(state.core.data_root());
        state
            .core
            .application(&store)
            .history_state()
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn restore_recovery_generation(
    file_name: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::restore_generation(&app_context(state), &file_name)
    })
    .await
}

#[tauri::command]
pub async fn import_scratch_session(
    path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        let path = std::path::PathBuf::from(path);
        adapter::import_session(&app_context(state), &path)
    })
    .await
}

#[tauri::command]
pub async fn update_session_settings(
    patch: SessionSettingsPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_session_settings(&app_context(state), patch)
    })
    .await
}

#[tauri::command]
pub async fn set_master_gain_db(gain_db: f64, app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        adapter::set_master_gain_db(&app_context(state), gain_db)
    })
    .await
}

#[tauri::command]
pub async fn relink_missing_dependency(
    asset_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        adapter::relink_missing_dependency(&app_context(state), asset_id, &new_path)
    })
    .await
}

#[tauri::command]
pub async fn disable_missing_plugin(
    device_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::disable_missing_plugin(&app_context(state), &device_id)
    })
    .await
}

#[tauri::command]
pub async fn replace_missing_track_plugin(
    device_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking_without_command_gate(app, move |state| {
        adapter::replace_missing_track_plugin(&app_context(state), &device_id, &new_path)
    })
    .await
}

#[tauri::command]
pub async fn get_missing_dependencies(app: AppHandle) -> Result<Vec<MissingDependency>, String> {
    run_blocking(app, |state| {
        let session = state
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .session;
        Ok(crate::missing::collect_missing(
            state.core.data_root(),
            &session,
        ))
    })
    .await
}
