use super::*;

#[tauri::command]
pub async fn set_track_instrument(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "instrument.set",
        json!({ "trackId": track_id, "pluginPath": plugin_path }),
    )
    .await
}

#[tauri::command]
pub async fn clear_track_instrument(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "instrument.clear", json!({ "trackId": track_id })).await
}

#[tauri::command]
pub async fn add_track_effect(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "effect.add",
        json!({ "trackId": track_id, "pluginPath": plugin_path }),
    )
    .await
}

#[tauri::command]
pub async fn remove_track_effect(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "effect.remove",
        json!({ "trackId": track_id, "deviceId": device_id }),
    )
    .await
}

#[tauri::command]
pub async fn reorder_track_effects(
    track_id: String,
    ordered_device_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "effect.reorder",
        json!({ "trackId": track_id, "deviceIds": ordered_device_ids }),
    )
    .await
}

#[tauri::command]
pub async fn set_track_device_bypassed(
    track_id: String,
    device_id: String,
    bypassed: bool,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "device.bypass",
        json!({ "trackId": track_id, "deviceId": device_id, "bypassed": bypassed }),
    )
    .await
}

#[tauri::command]
pub async fn set_track_device_parameter(
    track_id: String,
    device_id: String,
    parameter_index: u32,
    value: f32,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "device.parameter.set",
        json!({
            "trackId": track_id,
            "deviceId": device_id,
            "parameterIndex": parameter_index,
            "value": value,
        }),
    )
    .await
}

#[tauri::command]
pub async fn open_track_plugin_editor(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<(), String> {
    dispatch(
        app,
        "plugin.editor.open",
        json!({ "trackId": track_id, "deviceId": device_id }),
    )
    .await
}
