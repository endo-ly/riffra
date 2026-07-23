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
use crate::session::{
    AudioClipMove, AudioClipPatch, AudioTakeVariant, CreativeSession, DesignTool, FrameRange,
    MidiClipMove, MidiClipPatch, MidiInputRoute, ProjectTimebase, TimelineTick, TrackKind,
    Workspace,
};

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
pub fn add_midi_clip_to_arrangement(
    asset_id: String,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::add_midi_clip(&context(&state), asset_id, name, start_tick, track_id)
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
pub fn remove_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_timeline_clips(&context(&state), &audio_clip_ids, &midi_clip_ids)
}

#[tauri::command]
pub fn trim_audio_clip(
    clip_id: String,
    start_tick: TimelineTick,
    source_range: FrameRange,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::trim_audio_clip(&context(&state), &clip_id, start_tick, source_range)
}

#[tauri::command]
pub fn split_audio_clip(
    clip_id: String,
    split_tick: TimelineTick,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        let id = format!("clip:split:{stamp}:{}", arrangement.revision + 1);
        arrangement.split_audio_clip(&clip_id, split_tick, id)
    })
}

#[tauri::command]
pub fn duplicate_audio_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        let id = format!("clip:duplicate:{stamp}:{}", arrangement.revision + 1);
        arrangement.duplicate_audio_clip(&clip_id, id)
    })
}

#[tauri::command]
pub fn move_audio_clips(
    moves: Vec<AudioClipMove>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.move_audio_clips(moves)
    })
}

#[tauri::command]
pub fn update_midi_clip(
    clip_id: String,
    patch: MidiClipPatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_midi_clip(&clip_id, patch)
    })
}

#[tauri::command]
pub fn move_midi_clips(
    moves: Vec<MidiClipMove>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.move_midi_clips(moves)
    })
}

#[tauri::command]
pub fn trim_midi_clip(
    clip_id: String,
    start_tick: TimelineTick,
    duration_ticks: u64,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.trim_midi_clip(&clip_id, start_tick, duration_ticks)
    })
}

#[tauri::command]
pub fn split_midi_clip(
    clip_id: String,
    split_tick: TimelineTick,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.split_midi_clip(
            &clip_id,
            split_tick,
            format!("midi-clip:split:{stamp}:{}", arrangement.revision + 1),
        )
    })
}

#[tauri::command]
pub fn duplicate_midi_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.duplicate_midi_clip(
            &clip_id,
            format!("midi-clip:duplicate:{stamp}:{}", arrangement.revision + 1),
        )
    })
}

#[tauri::command]
pub fn paste_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: TimelineTick,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::paste_timeline_clips(
        &context(&state),
        &audio_clip_ids,
        &midi_clip_ids,
        start_tick,
    )
}

#[tauri::command]
pub fn crossfade_audio_clips(
    first_id: String,
    second_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.crossfade_audio_clips(&first_id, &second_id)
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
pub fn update_arrangement_timebase(
    timebase: ProjectTimebase,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_timebase(&context(&state), timebase)
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
pub fn update_timeline_punch_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::apply_arrangement_edit(&context(&state), |arrangement| {
        arrangement.update_punch_range(enabled, start_tick, end_tick)
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
pub fn add_track(
    name: String,
    kind: TrackKind,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::add_track(&context(&state), name, kind)
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
pub fn set_track_audio_input(
    track_id: String,
    channel_index: Option<u32>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_track_audio_input(&context(&state), &track_id, channel_index)
}

#[tauri::command]
pub fn set_track_midi_input(
    track_id: String,
    route: MidiInputRoute,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_track_midi_input(&context(&state), &track_id, route)
}

#[tauri::command]
pub fn set_track_instrument(
    track_id: String,
    plugin_path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_track_instrument(&context(&state), &track_id, &plugin_path)
}

#[tauri::command]
pub fn clear_track_instrument(
    track_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::clear_track_instrument(&context(&state), &track_id)
}

#[tauri::command]
pub fn add_track_effect(
    track_id: String,
    plugin_path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::add_track_effect(&context(&state), &track_id, &plugin_path)
}

#[tauri::command]
pub fn remove_track_effect(
    track_id: String,
    device_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_track_effect(&context(&state), &track_id, &device_id)
}

#[tauri::command]
pub fn reorder_track_effects(
    track_id: String,
    ordered_device_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::reorder_track_effects(&context(&state), &track_id, &ordered_device_ids)
}

#[tauri::command]
pub fn set_track_device_bypassed(
    track_id: String,
    device_id: String,
    bypassed: bool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_track_device_bypassed(&context(&state), &track_id, &device_id, bypassed)
}

#[tauri::command]
pub fn set_track_device_parameter(
    track_id: String,
    device_id: String,
    parameter_index: u32,
    value: f32,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_track_device_parameter(
        &context(&state),
        &track_id,
        &device_id,
        parameter_index,
        value,
    )
}

#[tauri::command]
pub fn open_track_plugin_editor(
    track_id: String,
    device_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    application::open_track_plugin_editor(&context(&state), &track_id, &device_id)
}

#[tauri::command]
pub fn remove_track(
    track_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_track(&context(&state), &track_id)
}

#[tauri::command]
pub fn duplicate_track(
    track_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::duplicate_track(&context(&state), &track_id)
}

#[tauri::command]
pub fn reorder_track(
    track_id: String,
    target_index: usize,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::reorder_track(&context(&state), &track_id, target_index)
}

#[tauri::command]
pub fn add_marker(
    tick: TimelineTick,
    name: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::add_marker(&context(&state), tick, name)
}

#[tauri::command]
pub fn update_marker(
    marker_id: String,
    name: Option<String>,
    tick: Option<TimelineTick>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_marker(&context(&state), &marker_id, name, tick)
}

#[tauri::command]
pub fn remove_marker(
    marker_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::remove_marker(&context(&state), &marker_id)
}

#[tauri::command]
pub fn add_midi_note(
    clip_id: String,
    start_tick: TimelineTick,
    pitch: u8,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::add_midi_note(
        &context(&state),
        &clip_id,
        start_tick,
        pitch,
        duration_ticks,
        velocity,
        channel,
    )
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
pub fn update_midi_notes(
    clip_id: String,
    updates: Vec<application::MidiNoteUpdate>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::update_midi_notes(&context(&state), &clip_id, updates)
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
pub fn quantize_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::quantize_midi_notes(&context(&state), &clip_id, &note_ids, grid_ticks)
}

#[tauri::command]
pub fn duplicate_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::duplicate_midi_notes(&context(&state), &clip_id, &note_ids, offset_ticks)
}

#[tauri::command]
pub fn set_take_variant(
    take_id: String,
    variant: AudioTakeVariant,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::set_take_variant(&context(&state), &take_id, variant)
}

#[tauri::command]
pub fn start_take_comparison(
    take_id: String,
    state: State<'_, AppState>,
) -> Result<crate::model::AudioStatus, String> {
    application::start_take_comparison(&context(&state), &take_id)
}

#[tauri::command]
pub fn switch_take_comparison_variant(
    variant: AudioTakeVariant,
    state: State<'_, AppState>,
) -> Result<crate::model::AudioStatus, String> {
    application::switch_take_comparison_variant(&context(&state), variant)
}

#[tauri::command]
pub fn stop_take_comparison(
    state: State<'_, AppState>,
) -> Result<crate::model::AudioStatus, String> {
    application::stop_take_comparison(&context(&state))
}

#[tauri::command]
pub fn activate_take(
    session_id: String,
    take_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::activate_take(&context(&state), &session_id, &take_id)
}

#[tauri::command]
pub fn place_take_as_separate_clip(
    take_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    application::place_take_as_separate_clip(&context(&state), &take_id)
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
