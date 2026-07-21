//! Thin Tauri command boundary for MIDI export.

use tauri::State;

use crate::AppState;
use crate::midi::{self, MidiExportResult};
use crate::storage::now_ms;

// #[tauri::command]
// pub fn export_midi(state: State<'_, AppState>) -> Result<MidiExportResult, String> {
//     let session = state.session.lock().map_err(lock_error)?.clone();
//     midi::export(&state.data_root, &session, now_ms())
// }

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}
