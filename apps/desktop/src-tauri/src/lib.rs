//! Tauri Application Composition Root.
//!
//! `lib.rs` deliberately hosts only:
//!
//! - `mod` declarations,
//! - the `AppState` struct containing the Host connection manager,
//! - the Tauri `setup` hook that creates and registers the shared Host,
//! - the `invoke_handler` registration that wires Tauri commands to their
//!   feature-level implementations,
//! - startup state construction and the invoke registration table.
//!
//! The Runtime crate owns the live DAW services. This crate contains only
//! Tauri command adapters, Desktop bootstrap DTOs, resource-path resolution,
//! and the Host event bridge that forwards active Host events to the WebView.

mod asset;
mod host_commands;
mod host_connection;
mod model;
#[cfg(test)]
mod types;
mod update_commands;

use host_commands::*;
use host_connection::{EmbeddedHostSettings, HostConnectionManager, NativeCommandError};
use model::BootstrapState;
use riffra_runtime::RuntimeBinaries;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

struct AppState {
    pub(crate) host_connection: Arc<HostConnectionManager>,
}

fn safe_mode_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref().eq_ignore_ascii_case("--safe-mode"))
}

fn safe_mode_requested() -> bool {
    safe_mode_from_args(std::env::args())
        || std::env::var("RIFFRA_SAFE_MODE")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
}

fn monitor_shutdown_request(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("riffra-desktop-shutdown".into())
        .spawn(move || {
            loop {
                let requested = app
                    .try_state::<AppState>()
                    .is_some_and(|state| state.host_connection.shutdown_requested());
                if requested {
                    if let Some(state) = app.try_state::<AppState>() {
                        state.host_connection.shutdown();
                    }
                    app.exit(0);
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed)
                && let Some(state) = window.try_state::<AppState>()
            {
                // The window is actually gone. The Host-owned persistence
                // coordinator has already received its shutdown opportunity;
                // attached Hosts remain alive because the manager only closes
                // the Desktop-side event connection.
                state.host_connection.shutdown();
            }
        })
        .setup(|app| {
            let safe_mode = safe_mode_requested();
            let data_root = app
                .path()
                .audio_dir()
                .map_err(|error| format!("User Music folder is unavailable: {error}"))?
                .join("Riffra");
            let built_in_instruments_root = app
                .path()
                .resource_dir()
                .map_err(|error| format!("Application resources are unavailable: {error}"))?
                .join("riffra-resources")
                .join("instruments")
                .join("builtin");
            let binaries = RuntimeBinaries::beside_current_executable()?;
            let host_connection = HostConnectionManager::open(
                app.handle().clone(),
                EmbeddedHostSettings {
                    data_root,
                    built_in_instruments_root,
                    safe_mode,
                    binaries,
                },
            )?;
            app.manage(AppState { host_connection });
            monitor_shutdown_request(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            dispatch_control,
            host_connection::get_host_connection_state,
            host_connection::list_local_hosts,
            host_connection::switch_host,
            host_connection::reconnect_host,
            asset::commands::import_midi_bytes,
            update_commands::check_for_app_update,
            update_commands::install_app_update
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}
