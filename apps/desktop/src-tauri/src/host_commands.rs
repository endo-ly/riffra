use super::*;
use crate::host_connection::{HostConnectionBootstrap, HostTarget, LocalHostInfo};
use crate::model::HostConnectionState;
use riffra_runtime::api::ControlCommand;
use serde_json::Value;
use tauri::State;

/// Runs a synchronous Host operation without blocking the async worker pool.
async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, NativeCommandError>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, NativeCommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || operation(&app.state::<AppState>()))
        .await
        .map_err(|error| {
            NativeCommandError::command_failed(format!("Native blocking operation failed: {error}"))
        })?
}

#[tauri::command]
pub(crate) async fn get_bootstrap_state(
    app: AppHandle,
) -> Result<BootstrapState, NativeCommandError> {
    run_blocking(app, |state| {
        state
            .host_connection
            .desktop_bootstrap()
            .map_err(NativeCommandError::command_failed)
    })
    .await
}

/// Runs one Control Command through the current Host and returns the value
/// of its result.
#[tauri::command]
pub(crate) async fn dispatch_control(
    command: String,
    params: Value,
    app: AppHandle,
) -> Result<Value, NativeCommandError> {
    run_blocking(app, move |state| {
        let command = ControlCommand::decode(&command, params)?;
        let output = state.host_connection.dispatch(command)?;
        let mut result = serde_json::to_value(output)
            .map_err(|error| NativeCommandError::command_failed(error.to_string()))?;
        Ok(result["value"].take())
    })
    .await
}

#[tauri::command]
pub(crate) fn get_host_connection_state(
    state: State<'_, crate::AppState>,
) -> Result<HostConnectionState, NativeCommandError> {
    Ok(state.host_connection.state())
}

#[tauri::command]
pub(crate) async fn list_local_hosts(
    app: AppHandle,
) -> Result<Vec<LocalHostInfo>, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::AppState>()
            .host_connection
            .list_local_hosts()
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("Local Host discovery failed: {error}"))
    })?
    .map_err(|error| NativeCommandError::host_unavailable(error.to_string()))
}

#[tauri::command]
pub(crate) async fn switch_host(
    target: HostTarget,
    app: AppHandle,
) -> Result<HostConnectionBootstrap, NativeCommandError> {
    let manager = Arc::clone(&app.state::<crate::AppState>().host_connection);
    tauri::async_runtime::spawn_blocking(move || manager.switch(target))
        .await
        .map_err(|error| {
            NativeCommandError::command_failed(format!("Host switch failed: {error}"))
        })?
        .map_err(|error| NativeCommandError::host_unavailable(error.to_string()))
}

#[tauri::command]
pub(crate) async fn reconnect_host(
    app: AppHandle,
) -> Result<HostConnectionBootstrap, NativeCommandError> {
    let manager = Arc::clone(&app.state::<crate::AppState>().host_connection);
    tauri::async_runtime::spawn_blocking(move || manager.reconnect())
        .await
        .map_err(|error| {
            NativeCommandError::command_failed(format!("Host reconnect failed: {error}"))
        })?
        .map_err(|error| NativeCommandError::host_unavailable(error.to_string()))
}
