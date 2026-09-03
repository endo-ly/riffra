use super::*;
use riffra_runtime::jobs::BackgroundJobStatus;
use riffra_runtime::projects::ProjectExport;
use serde_json::json;

/// Runs a synchronous Host operation without blocking the async worker pool.
async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || operation(&app.state::<AppState>()))
        .await
        .map_err(|error| format!("Native blocking operation failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn get_bootstrap_state(app: AppHandle) -> Result<BootstrapState, String> {
    run_blocking(app, |state| state.host_connection.desktop_bootstrap()).await
}

#[tauri::command]
pub(crate) async fn export_project(path: String, app: AppHandle) -> Result<ProjectExport, String> {
    run_blocking(app, move |state| {
        state
            .host_connection
            .dispatch("project.export", json!({ "output": path }))
    })
    .await
}

#[tauri::command]
pub(crate) fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<BackgroundJobStatus>, String> {
    state
        .host_connection
        .dispatch("job.get", json!({ "id": id }))
}

#[tauri::command]
pub(crate) fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<BackgroundJobStatus>, String> {
    state
        .host_connection
        .dispatch("job.cancel", json!({ "id": id }))
}

#[tauri::command]
pub(crate) async fn probe_audio_devices(app: AppHandle) -> Result<AudioDeviceProbe, String> {
    run_blocking(app, |state| {
        state.host_connection.dispatch("audio.probe", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn probe_device_channels(
    app: AppHandle,
    driver: String,
    input_device: String,
    output_device: String,
) -> Result<model::DeviceChannels, String> {
    run_blocking(app, move |state| {
        state.host_connection.dispatch(
            "audio.channels.probe",
            json!({
                "driver": driver,
                "inputDevice": input_device,
                "outputDevice": output_device,
            }),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_audio_status(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state.host_connection.dispatch("audio.status", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_master_gain_db(gain_db: f64, app: AppHandle) -> Result<(), String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    run_blocking(app, move |state| {
        state
            .host_connection
            .dispatch("audio.master-gain.preview", json!({ "gainDb": gain_db }))
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_emergency_mute(muted: bool, app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        state
            .host_connection
            .dispatch("audio.emergency-mute", json!({ "muted": muted }))
    })
    .await
}

#[tauri::command]
pub(crate) async fn recover_audio_device(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state.host_connection.dispatch("audio.recover", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn retry_startup_runtime(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host_connection
            .dispatch("audio.startup.retry", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn enable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host_connection
            .dispatch("midi.listening.enable", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn disable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host_connection
            .dispatch("midi.listening.disable", json!({}))
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_preview(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host_connection
            .dispatch("asset.preview.stop", json!({}))
    })
    .await
}
