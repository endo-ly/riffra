//! Tauri boundary for the application update flow.
//!
//! The Windows updater terminates the process right after launching the
//! installer, so the Embedded Host shutdown is injected through the
//! `on_before_exit` hook; the installer restarts the application afterwards.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::{AppState, NativeCommandError};

/// Returns the version published on the release endpoint, or null when the
/// application is current.
#[tauri::command]
pub(crate) async fn check_for_app_update(
    app: AppHandle,
) -> Result<Option<String>, NativeCommandError> {
    let updater = app
        .updater()
        .map_err(|error| NativeCommandError::command_failed(error.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|error| NativeCommandError::command_failed(error.to_string()))?;
    Ok(update.map(|update| update.version))
}

/// Downloads and installs the update published on the release endpoint.
///
/// On Windows this call does not return: the updater runs the Embedded Host
/// shutdown hook, exits the process, and the installer applies the update and
/// relaunches the application.
#[tauri::command]
pub(crate) async fn install_app_update(app: AppHandle) -> Result<(), NativeCommandError> {
    let host_connection = Arc::clone(&app.state::<AppState>().host_connection);
    let app_handle = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            host_connection.shutdown();
            app_handle.cleanup_before_exit();
        })
        .build()
        .map_err(|error| NativeCommandError::command_failed(error.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|error| NativeCommandError::command_failed(error.to_string()))?;
    let Some(update) = update else {
        return Err(NativeCommandError::command_failed(
            "no application update is available",
        ));
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| NativeCommandError::command_failed(error.to_string()))
}
