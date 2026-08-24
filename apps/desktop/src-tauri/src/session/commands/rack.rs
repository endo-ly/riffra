use super::*;

#[tauri::command]
pub async fn set_track_instrument(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking_without_command_gate(app, move |state| {
        adapter::set_track_instrument(&app_context(state), &track_id, &plugin_path)
    })
    .await
}

#[tauri::command]
pub async fn clear_track_instrument(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::clear_track_instrument(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn add_track_effect(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking_without_command_gate(app, move |state| {
        adapter::add_track_effect(&app_context(state), &track_id, &plugin_path)
    })
    .await
}

#[tauri::command]
pub async fn remove_track_effect(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_track_effect(&app_context(state), &track_id, &device_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_track_effects(
    track_id: String,
    ordered_device_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::reorder_track_effects(&app_context(state), &track_id, &ordered_device_ids)
    })
    .await
}

#[tauri::command]
pub async fn set_track_device_bypassed(
    track_id: String,
    device_id: String,
    bypassed: bool,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::set_track_device_bypassed(&app_context(state), &track_id, &device_id, bypassed)
    })
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
    run_blocking(app, move |state| {
        adapter::set_track_device_parameter(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_index,
            value,
        )
    })
    .await
}

#[tauri::command]
pub async fn open_track_plugin_editor(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<(), String> {
    // Opening an editor is a native lifecycle operation, not a canonical
    // Session mutation. It must not occupy the Desktop command gate while JUCE waits
    // for a third-party editor on the Message Thread.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_host_lifecycle(|state| {
            adapter::open_track_plugin_editor(&app_context(state), &track_id, &device_id)
                .map_err(|error| error.to_string())
        })
    })
    .await
    .map_err(|error| format!("Track plugin editor operation failed: {error}"))?
}

#[tauri::command]
pub async fn persist_track_plugin_state(
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::persist_track_plugin_state(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_values,
            state_data,
            bypassed,
        )
    })
    .await
}

#[tauri::command]
pub async fn persist_track_plugin_parameter(
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::persist_track_plugin_parameter(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_index,
            value,
        )
    })
    .await
}
