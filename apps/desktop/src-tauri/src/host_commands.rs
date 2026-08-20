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
        let plugin_catalog = match plugin_catalog::load(state.core.data_root()) {
            Ok(catalog) => catalog,
            Err(error) => {
                let _ = diagnostics::record(
                    state.core.data_root(),
                    "plugin-catalog",
                    &error.to_string(),
                );
                Vec::new()
            }
        };
        let recovered_from_generation = state.core.recovered_from_generation();
        Ok(BootstrapState {
            session: state
                .core
                .snapshot()
                .map_err(|error| error.to_string())?
                .session,
            plugin_catalog,
            runtime_started: state.core.audio().startup_completed(),
            runtime_startup_finished: state.core.audio().startup_finished(),
            recovered_from_generation,
            safe_mode: state.core.safe_mode(),
            native_available: true,
            recovery_candidates: bootstrap_recovery_candidates(
                state.core.data_root(),
                recovered_from_generation,
            )?,
            data_root: state.core.data_root().to_string_lossy().into_owned(),
            vst3_root: DEFAULT_VST3_ROOT.into(),
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
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .session;
        projects::export(state.core.data_root(), &session, storage::now_ms())
    })
    .await
}

#[tauri::command]
pub(crate) fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .jobs
        .status(&id)
        .map(jobs::to_background_status)
        .transpose()
}

#[tauri::command]
pub(crate) fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::BackgroundJobStatus>, String> {
    state
        .jobs
        .cancel(&id)
        .map(jobs::to_background_status)
        .transpose()
}

// App-level audio device discovery. The native sidecar owns the actual probe;
// Rust parses its stdout into the same DTOs the React layer consumes. These
// commands are pure device queries — they touch neither the canonical session
// nor the Asset registry.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeAudioProbe {
    #[serde(default)]
    pub(crate) drivers: Vec<NativeAudioDriver>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeAudioDriver {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) access_mode: model::AudioAccessMode,
    pub(crate) device_pairing: model::AudioDevicePairing,
    #[serde(default)]
    pub(crate) inputs: Vec<model::AudioDeviceInfo>,
    #[serde(default)]
    pub(crate) outputs: Vec<model::AudioDeviceInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeDeviceChannels {
    driver: String,
    input_device: String,
    #[serde(default)]
    input_channels: Vec<model::AudioChannelInfo>,
    output_device: String,
    #[serde(default)]
    output_channels: Vec<model::AudioChannelInfo>,
}

#[tauri::command]
pub(crate) async fn probe_audio_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AudioDeviceProbe, String> {
    if state.core.safe_mode() {
        return Ok(AudioDeviceProbe {
            drivers: Vec::new(),
            refreshed_at_ms: storage::now_ms(),
            message: "Safe Mode skipped audio device discovery.".into(),
        });
    }
    let stdout = run_native_probe(app, &["--probe"]).await?;
    let probe = parse_stdout::<NativeAudioProbe>(&stdout, "audioDeviceProbe")?;
    Ok(AudioDeviceProbe {
        drivers: probe
            .drivers
            .into_iter()
            .map(|driver| AudioDriverInfo {
                name: driver.name,
                access_mode: driver.access_mode,
                device_pairing: driver.device_pairing,
                inputs: driver.inputs,
                outputs: driver.outputs,
            })
            .collect(),
        refreshed_at_ms: storage::now_ms(),
        message: "Audio device list refreshed.".into(),
    })
}

#[tauri::command]
pub(crate) async fn probe_device_channels(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    driver: String,
    input_device: String,
    output_device: String,
) -> Result<model::DeviceChannels, String> {
    if state.core.safe_mode() {
        return Err("Safe Mode skipped audio channel discovery.".into());
    }
    let stdout = run_native_probe(
        app,
        &[
            "--probe-channels",
            "--audio-driver",
            &driver,
            "--input-device",
            &input_device,
            "--output-device",
            &output_device,
        ],
    )
    .await?;
    let probe = parse_stdout::<NativeDeviceChannels>(&stdout, "deviceChannels")?;
    Ok(model::DeviceChannels {
        driver: probe.driver,
        input_device: probe.input_device,
        input_channels: probe.input_channels,
        output_device: probe.output_device,
        output_channels: probe.output_channels,
    })
}

async fn run_native_probe(app: tauri::AppHandle, args: &[&str]) -> Result<Vec<u8>, String> {
    let _permit = match tokio::time::timeout(
        NATIVE_PROBE_TIMEOUT,
        NATIVE_PROBE_GATE
            .get_or_init(|| tokio::sync::Semaphore::new(1))
            .acquire(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => return Err("Device probe coordinator is shutting down.".to_owned()),
        Err(_) => {
            return Err(format!(
                "Device probe timed out while waiting for another probe after {} seconds; no device state was changed.",
                NATIVE_PROBE_TIMEOUT.as_secs()
            ));
        }
    };
    let mut command = app
        .shell()
        .sidecar("riffra-audio")
        .map_err(|error| format!("Device probe sidecar could not be prepared: {error}"))?;
    if !args.is_empty() {
        command = command.args(args);
    }

    let (mut events, child) = command.spawn().map_err(|error| {
        format!("Device probe could not start; no device state was changed: {error}")
    })?;
    let collected = tokio::time::timeout(NATIVE_PROBE_TIMEOUT, async {
        let mut code = None;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Terminated(payload) => code = payload.code,
                CommandEvent::Stdout(line) => {
                    stdout.extend(line);
                    stdout.push(b'\n');
                }
                CommandEvent::Stderr(line) => {
                    stderr.extend(line);
                    stderr.push(b'\n');
                }
                CommandEvent::Error(error) => stderr.extend(error.as_bytes()),
                _ => {}
            }
        }
        (code, stdout, stderr)
    })
    .await;

    let (code, stdout, stderr) = match collected {
        Ok(output) => {
            drop(child);
            output
        }
        Err(_) => {
            let kill_error = child
                .kill()
                .err()
                .map(|error| format!(" Kill failed: {error}"));
            return Err(format!(
                "Device probe timed out after {} seconds; no device state was changed.{}",
                NATIVE_PROBE_TIMEOUT.as_secs(),
                kill_error.unwrap_or_default()
            ));
        }
    };

    let Some(code) = code else {
        return Err(
            "Device probe ended without an exit status; no device state was changed.".into(),
        );
    };
    if code != 0 {
        let detail = String::from_utf8_lossy(&stderr).trim().to_owned();
        let detail = if detail.is_empty() {
            stdout
                .split(|byte| *byte == b'\n')
                .find_map(|line| {
                    let value = serde_json::from_slice::<serde_json::Value>(line).ok()?;
                    (value.get("type").and_then(serde_json::Value::as_str) == Some("error"))
                        .then(|| value.get("message")?.as_str().map(str::to_owned))?
                })
                .unwrap_or_default()
        } else {
            detail
        };
        return Err(if detail.is_empty() {
            format!("Device probe exited with code {code}; no device state was changed.")
        } else {
            format!("Device probe failed: {detail}")
        });
    }
    Ok(stdout)
}

pub(crate) fn parse_stdout<T>(stdout: &[u8], expected_type: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let line = stdout
        .split(|byte| *byte == b'\n')
        .find(|line| {
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
                return false;
            };
            value.get("type").and_then(serde_json::Value::as_str) == Some(expected_type)
        })
        .ok_or_else(|| format!("Device probe returned no readable {expected_type} response."))?;
    serde_json::from_slice::<T>(line)
        .map_err(|_| format!("Device probe returned an unexpected {expected_type} response."))
}

// Low-level Audio Runtime passthroughs with no canonical-session mutation.

#[tauri::command]
pub(crate) async fn get_audio_status(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state.core.audio().refresh_meters().map_err(String::from)
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
            .core
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
            .core
            .audio()
            .set_emergency_mute_from_user(muted)
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn recover_audio_device(app: AppHandle) -> Result<AudioStatus, String> {
    let operation_app = app.clone();
    run_blocking(app, move |state| {
        if state.core.safe_mode() {
            return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
        }
        let reopen_outcome = state
            .core
            .audio()
            .recover_audio_device(&operation_app)
            .map_err(String::from)?;
        if !state.core.audio().startup_completed() {
            state.core.audio().mark_startup_pending();
            let initialization = startup::initialize_audio_runtime(state, || {});
            let succeeded = initialization
                .as_ref()
                .map(|result| result.runtime_error.is_none())
                .unwrap_or(false);
            let _ = operation_app.emit(
                "runtime-startup-finished",
                RuntimeStartupFinishedEvent { succeeded },
            );
            let initialization = initialization?;
            return Ok(initialization.status);
        }
        if let AudioDeviceReopenOutcome::SidecarRestarted(status) = reopen_outcome {
            return Ok(status);
        }
        session_adapter::reconcile_runtime_after_audio_device_change(
            &session::context::SessionContext {
                core: &state.core,
                audio: state.core.audio(),
                runtime: state.runtime.as_ref(),
                data_root: state.core.data_root(),
                safe_mode: false,
            },
        )
        .map_err(|error| {
            format!(
                "Audio device recovery succeeded, but the dependent Runtime could not be restored: {error}"
            )
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn retry_startup_runtime(app: AppHandle) -> Result<AudioStatus, String> {
    let operation_app = app.clone();
    run_blocking(app, move |state| {
        if state.core.safe_mode() {
            return Err(
                "Safe Mode keeps external audio devices isolated; restart normally to restore the Session runtime."
                    .into(),
            );
        }
        if state.core.audio().startup_completed() {
            return state
                .core
                .audio()
                .refresh_status()
                .map_err(String::from);
        }

        state.core.audio().mark_startup_pending();
        let initialization = startup::initialize_audio_runtime(state, || {});
        let succeeded = initialization
            .as_ref()
            .map(|result| result.runtime_error.is_none())
            .unwrap_or(false);
        let _ = operation_app.emit(
            "runtime-startup-finished",
            RuntimeStartupFinishedEvent { succeeded },
        );
        let initialization = initialization?;
        Ok(initialization.status)
    })
    .await
}

#[tauri::command]
pub(crate) async fn enable_midi_listening(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        if state.core.safe_mode() {
            return Err(
                "Safe Mode blocks MIDI input; offline MIDI and audio export remain available."
                    .into(),
            );
        }
        state
            .core
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
            .core
            .audio()
            .disable_midi_listening()
            .map_err(String::from)
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_preview(app: AppHandle) -> Result<AudioStatus, String> {
    run_blocking(app, |state| {
        state.core.audio().stop_preview().map_err(String::from)
    })
    .await
}
