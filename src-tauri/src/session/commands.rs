//! Thin Tauri command boundary for Session Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`SessionContext`](super::application::SessionContext) of concrete
//! dependencies, delegates to the matching Application Operation, and returns
//! the resulting DTO. The production workflow (arrangement edit, design
//! navigation, sample pad runtime sync, validate/persist) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::State;

use crate::AppState;
use crate::asset::AssetId;
use crate::missing::MissingDependency;
use crate::model::SessionAudioPair;
use crate::session::application::{self, SessionContext};
use crate::session::{AudioClipPatch, CreativeSession, DesignTool, TimelineTick, Workspace};

fn context<'a>(state: &'a State<'_, AppState>) -> SessionContext<'a> {
    SessionContext {
        audio: &state.audio,
        data_root: &state.data_root,
        session: &state.session,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn save_scratch_session(
    session: CreativeSession,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::save_session(&context(&state), session.clone())?;
    Ok(session)
}

#[tauri::command]
pub fn restore_recovery_generation(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::restore_generation(&context(&state), &file_name)
}

#[tauri::command]
pub fn import_scratch_session(
    path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let path = std::path::PathBuf::from(path);
    application::import_session(&context(&state), &path)
}

#[tauri::command]
pub fn create_sample_pad(
    asset_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::create_sample_pad(&context(&state), asset_id, name)
}

#[tauri::command]
pub fn update_sample_pad(
    pad_id: String,
    patch: application::SamplePadPatch,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::update_sample_pad(&context(&state), &pad_id, &patch)
}

#[tauri::command]
pub fn remove_sample_pad(
    pad_id: String,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::remove_sample_pad(&context(&state), &pad_id)
}

#[tauri::command]
pub fn add_audio_clip_to_arrangement(
    asset_id: String,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::add_audio_clip(&context(&state), asset_id, name, start_tick, track_id)
}

#[tauri::command]
pub fn update_audio_clip(
    clip_id: String,
    patch: AudioClipPatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_audio_clip(&clip_id, patch)
    })
}

#[tauri::command]
pub fn remove_audio_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.remove_audio_clip(&clip_id)
    })
}

#[tauri::command]
pub fn sync_arrangement_runtime(state: State<'_, AppState>) -> Result<(), String> {
    application::sync_arrangement_runtime(&context(&state))
}

#[tauri::command]
pub fn play_timeline(state: State<'_, AppState>) -> Result<(), String> {
    application::play_timeline(&context(&state))
}

#[tauri::command]
pub fn stop_timeline(state: State<'_, AppState>) -> Result<(), String> {
    application::stop_timeline(&context(&state))
}

#[tauri::command]
pub fn seek_timeline(tick: TimelineTick, state: State<'_, AppState>) -> Result<(), String> {
    application::seek_timeline(&context(&state), tick)
}

#[tauri::command]
pub fn update_timeline_loop_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_loop_range(enabled, start_tick, end_tick)
    })
}

#[tauri::command]
pub fn open_asset_in_design(
    asset_id: String,
    tool: DesignTool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::open_asset_in_design(&context(&state), asset_id, tool)
}

#[tauri::command]
pub fn switch_workspace(
    workspace: Workspace,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::switch_workspace(&context(&state), workspace)
}

#[tauri::command]
pub fn update_session_settings(
    patch: application::SessionSettingsPatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_session_settings(&context(&state), patch)
}

#[tauri::command]
pub fn add_track(name: String, state: State<'_, AppState>) -> Result<CreativeSession, String> {
    application::add_track(&context(&state), name)
}

#[tauri::command]
pub fn update_track(
    track_id: String,
    patch: application::TrackPatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_track(&context(&state), &track_id, patch)
}

#[tauri::command]
pub fn apply_ai_suggestion(
    clip_id: String,
    proposed_gain_db: f64,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_ai_suggestion(&context(&state), &clip_id, proposed_gain_db)
}

#[tauri::command]
pub fn set_master_gain_db(
    gain_db: f64,
    state: State<'_, AppState>,
) -> Result<SessionAudioPair, String> {
    application::set_master_gain_db(&context(&state), gain_db)
}

#[tauri::command]
pub fn relink_missing_dependency(
    asset_id: String,
    new_path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::relink_missing_dependency(&context(&state), asset_id, &new_path)
}

#[tauri::command]
pub fn disable_missing_plugin(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::disable_missing_plugin(&context(&state), &device_id)
}

#[tauri::command]
pub fn get_missing_dependencies(
    state: State<'_, AppState>,
) -> Result<Vec<MissingDependency>, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    Ok(crate::missing::collect_missing(&state.data_root, &session))
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}
