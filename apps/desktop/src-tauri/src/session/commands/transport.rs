use super::*;

#[tauri::command]
pub async fn get_runtime_projection_status(
    app: AppHandle,
) -> Result<RuntimeProjectionStatus, String> {
    dispatch(app, "runtime.projection.get", json!({})).await
}

#[tauri::command]
pub async fn retry_runtime_projection(app: AppHandle) -> Result<RuntimeProjectionStatus, String> {
    dispatch(app, "runtime.projection.retry", json!({})).await
}

#[tauri::command]
pub async fn play_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    dispatch(
        app,
        "transport.play",
        json!({ "transportSequence": transport_sequence }),
    )
    .await
}

#[tauri::command]
pub async fn stop_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    dispatch(
        app,
        "transport.stop",
        json!({ "transportSequence": transport_sequence }),
    )
    .await
}

#[tauri::command]
pub async fn go_to_start_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    dispatch(
        app,
        "transport.go-to-start",
        json!({ "transportSequence": transport_sequence }),
    )
    .await
}

#[tauri::command]
pub async fn seek_timeline(tick: TimelineTick, app: AppHandle) -> Result<(), String> {
    dispatch(app, "transport.seek", json!({ "tick": tick.0 })).await
}
