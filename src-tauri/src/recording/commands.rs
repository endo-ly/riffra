//! Thin Tauri command boundary for Recording Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`RecordingContext`](super::application::RecordingContext) of concrete
//! dependencies, delegates to the matching Application Operation, and returns
//! the resulting DTO. The production workflow (audio capture lifecycle,
//! Filesystem + Asset + Library relocation) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::State;

use crate::AppState;
use crate::library;
use crate::model::AudioStatus;
use crate::recording::RecordingAsset;
use crate::recording::application::{self, RecordingContext};

fn context<'a>(state: &'a State<'_, AppState>) -> RecordingContext<'a> {
    RecordingContext {
        audio: &state.audio,
        data_root: &state.data_root,
        session: &state.session,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn list_recordings(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<RecordingAsset>, String> {
    application::list_recordings(&context(&state), query.as_deref())
}

#[tauri::command]
pub fn rename_recording(
    id: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    application::rename_recording(&context(&state), &id, &new_name)
}

#[tauri::command]
pub fn delete_recording(id: String, state: State<'_, AppState>) -> Result<(), String> {
    application::delete_recording(&context(&state), &id)
}

#[tauri::command]
pub fn archive_recording(id: String, state: State<'_, AppState>) -> Result<String, String> {
    application::archive_recording(&context(&state), &id)
}

#[tauri::command]
pub fn promote_recording(id: String, state: State<'_, AppState>) -> Result<String, String> {
    application::promote_recording(&context(&state), &id)
}

#[tauri::command]
pub fn detect_duplicate_recordings(state: State<'_, AppState>) -> Result<Vec<Vec<String>>, String> {
    application::detect_duplicate_recordings(&context(&state))
}

#[tauri::command]
pub fn tag_recording(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<library::LibraryAsset, String> {
    application::tag_recording(&context(&state), &id, tag, note)
}

#[tauri::command]
pub fn start_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    application::start_recording(&context(&state))
}

#[tauri::command]
pub fn record_another_take(
    recording_session_id: String,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    application::record_another_take(&context(&state), &recording_session_id)
}

#[tauri::command]
pub fn stop_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    application::stop_recording(&context(&state))
}
