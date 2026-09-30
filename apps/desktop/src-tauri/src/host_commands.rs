use super::*;
use riffra_runtime::api::ControlCommand;
use serde_json::Value;

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
