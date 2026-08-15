use super::*;

#[tauri::command]
pub async fn retry_runtime_projection(app: AppHandle) -> Result<RuntimeProjectionStatus, String> {
    run_runtime_control(app, |state| {
        adapter::sync_arrangement_runtime(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn play_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        adapter::play_timeline(&app_context(state), transport_sequence)
    })
    .await
}

#[tauri::command]
pub async fn stop_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        adapter::stop_timeline(&app_context(state), transport_sequence)
    })
    .await
}

#[tauri::command]
pub async fn go_to_start_timeline(transport_sequence: u64, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        adapter::go_to_start_timeline(&app_context(state), transport_sequence)
    })
    .await
}

#[tauri::command]
pub async fn seek_timeline(tick: TimelineTick, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        adapter::seek_timeline(&app_context(state), tick)
    })
    .await
}
