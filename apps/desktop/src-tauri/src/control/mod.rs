mod router;
mod server;

use std::path::PathBuf;

use tauri::AppHandle;

/// Starts the local control endpoint after Desktop state has been registered.
pub(crate) fn start(app: AppHandle, data_root: PathBuf) -> Result<(), String> {
    server::start(app, data_root)
}
