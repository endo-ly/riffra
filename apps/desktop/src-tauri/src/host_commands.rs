use super::*;

/// Runs a synchronous host operation without blocking the async worker pool.
async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Native blocking operation failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn get_bootstrap_state(app: AppHandle) -> Result<BootstrapState, String> {
    run_blocking(app, |state| {
        let plugin_catalog = match plugin_catalog::load(state.host.data_root()) {
            Ok(catalog) => catalog,
            Err(error) => {
                let _ = diagnostics::record(
                    state.host.data_root(),
                    "plugin-catalog",
                    &error.to_string(),
                );
                Vec::new()
            }
        };
        let recovered_from_generation = state.host.core().recovered_from_generation();
        let canonical = state
            .host
            .core()
            .canonical_state()
            .map_err(|error| error.to_string())?;
        Ok(BootstrapState {
            canonical,
            plugin_catalog,
            runtime_started: state.host.core().audio().startup_completed(),
            runtime_startup_finished: state.host.core().audio().startup_finished(),
            recovered_from_generation,
            safe_mode: state.host.core().safe_mode(),
            native_available: true,
            recovery_candidates: bootstrap_recovery_candidates(
                state.host.data_root(),
                recovered_from_generation,
            )?,
            data_root: state.host.data_root().to_string_lossy().into_owned(),
            vst3_root: default_vst3_root(),
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn export_scratch_session(
    app: AppHandle,
) -> Result<projects::ProjectExport, String> {
    run_blocking(app, |state| {
        let session = state
            .host
            .core()
            .snapshot()
            .map_err(|error| error.to_string())?
            .session;
        projects::export(state.host.data_root(), &session, storage::now_ms())
    })
    .await
}

#[tauri::command]
pub(crate) fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .host
        .background_job(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .host
        .cancel_background_job(&id)
        .map_err(|error| error.to_string())
}

// App-level audio device discovery. The native sidecar owns the actual probe;
// Rust parses its stdout into the same DTOs the React layer consumes. These
// commands are pure device queries — they touch neither the canonical session
// nor the Asset registry.

#[tauri::command]
pub(crate) fn probe_audio_devices(state: State<'_, AppState>) -> Result<AudioDeviceProbe, String> {
    state
        .host
        .probe_devices()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn probe_device_channels(
    state: State<'_, AppState>,
    driver: String,
    input_device: String,
    output_device: String,
) -> Result<model::DeviceChannels, String> {
    state
        .host
        .probe_device_channels(&driver, &input_device, &output_device)
        .map_err(|error| error.to_string())
}

// Low-level Audio Runtime passthroughs with no canonical-session mutation.

#[tauri::command]
pub(crate) async fn get_audio_status(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host
            .core()
            .audio()
            .refresh_meters()
            .map_err(String::from)
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
            .host
            .core()
            .audio()
            .preview_master_gain_db(gain_db)
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_emergency_mute(muted: bool, app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        state
            .host
            .core()
            .audio()
            .set_emergency_mute_from_user(muted)
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn recover_audio_device(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        if state.host.core().safe_mode() {
            return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
        }
        if !state.host.core().audio().startup_completed() {
            return state.host.retry_runtime_startup().map_err(|error| error.to_string());
        }
        state.host.recover_audio_device().map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub(crate) async fn retry_startup_runtime(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, move |state| {
        if state.host.core().safe_mode() {
            return Err(
                "Safe Mode keeps external audio devices isolated; restart normally to restore the Session runtime."
                    .into(),
            );
        }
        state.host.retry_runtime_startup().map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub(crate) async fn enable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        if state.host.core().safe_mode() {
            return Err(
                "Safe Mode blocks MIDI input; offline MIDI and audio export remain available."
                    .into(),
            );
        }
        state
            .host
            .core()
            .audio()
            .enable_midi_listening()
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn disable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host
            .core()
            .audio()
            .disable_midi_listening()
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_preview(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state
            .host
            .core()
            .audio()
            .stop_preview()
            .map_err(String::from)
    })
    .await
}
