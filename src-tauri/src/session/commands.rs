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
use crate::errors::DomainError;
use crate::missing::MissingDependency;
use crate::model::AudioStatus;
use crate::session::application::{self, SessionContext};
use crate::session::{AudioClipPatch, CreativeSession, DesignTool, Workspace};

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
) -> Result<(CreativeSession, AudioStatus), String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::create_sample_pad(&context(&state), asset_id, name)
}

#[tauri::command]
pub fn update_sample_pad(
    pad_id: String,
    patch: application::SamplePadPatch,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    application::update_sample_pad(&context(&state), &pad_id, &patch)
}

#[tauri::command]
pub fn remove_sample_pad(
    pad_id: String,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    application::remove_sample_pad(&context(&state), &pad_id)
}

#[tauri::command]
pub fn add_audio_clip_to_arrangement(
    asset_id: String,
    name: String,
    duration_ms: u64,
    track_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::add_audio_clip(&context(&state), asset_id, name, duration_ms, track_id)
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
pub fn move_audio_clip_to_track(
    clip_id: String,
    track_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    if track_id.trim().is_empty() {
        return Err("Target track id must not be empty.".into());
    }
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        if !arrangement.has_track(&track_id) {
            return Err(DomainError::UnknownTrack(track_id.clone()));
        }
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                track_id: Some(track_id),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
pub fn set_audio_clip_muted(
    clip_id: String,
    muted: bool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                muted: Some(muted),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
pub fn set_audio_clip_loop(
    clip_id: String,
    loop_enabled: bool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                loop_enabled: Some(loop_enabled),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
pub fn duplicate_audio_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let new_id = format!("{clip_id}:copy:{}", crate::storage::now_ms());
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.duplicate_audio_clip(&clip_id, new_id)
    })
}

#[tauri::command]
pub fn split_audio_clip(
    clip_id: String,
    at_offset_ms: Option<u64>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let new_id = format!("{clip_id}:split:{}", crate::storage::now_ms());
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        let offset = match at_offset_ms {
            Some(value) => value,
            None => {
                let Some(clip) = arrangement
                    .audio_clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                else {
                    return Err(DomainError::InvalidClip(format!(
                        "Audio clip '{clip_id}' not found."
                    )));
                };
                clip.duration_ms / 2
            }
        };
        arrangement.split_audio_clip(&clip_id, offset, new_id)
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
pub fn import_midi_clip(
    asset_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::import_midi_clip(&context(&state), asset_id, name)
}

#[tauri::command]
pub fn update_midi_note(
    clip_id: String,
    note_id: String,
    patch: application::MidiNotePatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_midi_note(&context(&state), &clip_id, &note_id, patch)
}

#[tauri::command]
pub fn remove_midi_note(
    clip_id: String,
    note_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_midi_note(&context(&state), &clip_id, &note_id)
}

#[tauri::command]
pub fn remove_midi_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_midi_clip(&context(&state), &clip_id)
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
) -> Result<(CreativeSession, AudioStatus), String> {
    application::set_master_gain_db(&context(&state), gain_db)
}

#[tauri::command]
pub fn set_emergency_mute(
    muted: bool,
    state: State<'_, AppState>,
) -> Result<(CreativeSession, AudioStatus), String> {
    application::set_emergency_mute(&context(&state), muted)
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
    format!("An internal state lock was poisoned: {error}")
}
