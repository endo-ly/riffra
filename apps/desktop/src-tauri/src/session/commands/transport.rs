use super::*;

#[tauri::command]
pub async fn get_runtime_projection_status(
    app: AppHandle,
) -> Result<RuntimeProjectionStatus, NativeCommandError> {
    dispatch(app, "runtime.projection.get", json!({})).await
}

#[tauri::command]
pub async fn retry_runtime_projection(
    app: AppHandle,
) -> Result<RuntimeProjectionStatus, NativeCommandError> {
    dispatch(app, "runtime.projection.retry", json!({})).await
}

#[tauri::command]
pub async fn play_timeline(app: AppHandle) -> Result<(), NativeCommandError> {
    dispatch(app, "transport.play", json!({})).await
}

#[tauri::command]
pub async fn stop_timeline(app: AppHandle) -> Result<(), NativeCommandError> {
    dispatch(app, "transport.stop", json!({})).await
}

#[tauri::command]
pub async fn go_to_start_timeline(app: AppHandle) -> Result<(), NativeCommandError> {
    dispatch(app, "transport.go-to-start", json!({})).await
}

#[tauri::command]
pub async fn seek_timeline(tick: TimelineTick, app: AppHandle) -> Result<(), NativeCommandError> {
    dispatch(app, "transport.seek", json!({ "tick": tick.0 })).await
}
