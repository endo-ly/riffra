//! Thin Tauri command boundary for Session Application Operations.
//!
//! Each command receives an `AppHandle`, moves synchronous work to the
//! blocking pool, and builds a
//! [`SessionContext`](super::application::SessionContext) of concrete
//! dependencies, delegates to the matching Application Operation, and returns
//! the resulting DTO. The production workflow (arrangement edit, design
//! navigation, sample pad runtime sync, validate/persist) lives entirely in
//! [`super::application`]; nothing here re-implements it.

use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::asset::AssetId;
use crate::missing::MissingDependency;
use crate::model::{RuntimeProjectionStatus, SessionAudioPair};
use crate::session::application::{self, SessionContext};
use crate::session::{
    AudioClipMove, AudioClipPatch, AudioTakeVariant, AutomationParameter, AutomationPoint,
    CreativeSession, DesignTool, FrameRange, MidiClipMove, MidiClipPatch, MidiInputRoute,
    ProjectTimebase, TimelineTick, TrackKind, Workspace,
};

async fn run_blocking<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _session_operation = state.session_actor.enter()?;
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Session blocking operation failed: {error}"))?
}

/// Runtime controls must not queue behind canonical Session persistence or a
/// slow VST/native operation. They only read the current snapshot when needed
/// and never mutate the durable Session.
async fn run_runtime_control<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Runtime control operation failed: {error}"))?
}

/// Workspace navigation sends a stop intent and a processing-mode update as
/// one short runtime transaction. It remains independent from Session
/// persistence, but the pair must not interleave across rapid navigation
/// commands.
async fn run_workspace_control<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _workspace_runtime_gate = state
            .workspace_runtime_gate
            .lock()
            .map_err(|error| format!("Workspace runtime gate was poisoned: {error}"))?;
        operation(state.inner())
    })
    .await
    .map_err(|error| format!("Workspace control operation failed: {error}"))?
}

fn app_context(state: &AppState) -> SessionContext<'_> {
    SessionContext {
        audio: state.core.audio(),
        runtime: &state.runtime,
        session_actor: &state.session_actor,
        data_root: state.core.data_root(),
        session: state.core.session(),
        safe_mode: state.core.safe_mode(),
    }
}

#[tauri::command]
pub async fn save_scratch_session(
    session: CreativeSession,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::save_session(&app_context(state), session)
    })
    .await
}

#[tauri::command]
pub async fn restore_recovery_generation(
    file_name: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::restore_generation(&app_context(state), &file_name)
    })
    .await
}

#[tauri::command]
pub async fn import_scratch_session(
    path: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        let path = std::path::PathBuf::from(path);
        application::import_session(&app_context(state), &path)
    })
    .await
}

#[tauri::command]
pub async fn create_sample_pad(
    asset_id: String,
    name: String,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::create_sample_pad(&app_context(state), asset_id, name)
    })
    .await
}

#[tauri::command]
pub async fn update_sample_pad(
    pad_id: String,
    patch: application::SamplePadPatch,
    app: AppHandle,
) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::update_sample_pad(&app_context(state), &pad_id, &patch)
    })
    .await
}

#[tauri::command]
pub async fn remove_sample_pad(pad_id: String, app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::remove_sample_pad(&app_context(state), &pad_id)
    })
    .await
}

#[tauri::command]
pub async fn add_audio_clip_to_arrangement(
    asset_id: String,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::add_audio_clip(&app_context(state), asset_id, name, start_tick, track_id)
    })
    .await
}

#[tauri::command]
pub async fn add_midi_clip_to_arrangement(
    asset_id: String,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::add_midi_clip(&app_context(state), asset_id, name, start_tick, track_id)
    })
    .await
}

#[tauri::command]
pub async fn update_audio_clip(
    clip_id: String,
    patch: AudioClipPatch,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.update_audio_clip(&clip_id, patch)
        })
    })
    .await
}

#[tauri::command]
pub async fn remove_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::remove_timeline_clips(&app_context(state), &audio_clip_ids, &midi_clip_ids)
    })
    .await
}

#[tauri::command]
pub async fn trim_audio_clip(
    clip_id: String,
    start_tick: TimelineTick,
    source_range: FrameRange,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::trim_audio_clip(&app_context(state), &clip_id, start_tick, source_range)
    })
    .await
}

#[tauri::command]
pub async fn split_audio_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            let id = format!("clip:split:{stamp}:{}", arrangement.revision + 1);
            arrangement.split_audio_clip(&clip_id, split_tick, id)
        })
    })
    .await
}

#[tauri::command]
pub async fn duplicate_audio_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            let id = format!("clip:duplicate:{stamp}:{}", arrangement.revision + 1);
            arrangement.duplicate_audio_clip(&clip_id, id)
        })
    })
    .await
}

#[tauri::command]
pub async fn move_audio_clips(
    moves: Vec<AudioClipMove>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.move_audio_clips(moves)
        })
    })
    .await
}

#[tauri::command]
pub async fn update_midi_clip(
    clip_id: String,
    patch: MidiClipPatch,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.update_midi_clip(&clip_id, patch)
        })
    })
    .await
}

#[tauri::command]
pub async fn move_midi_clips(
    moves: Vec<MidiClipMove>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.move_midi_clips(moves)
        })
    })
    .await
}

#[tauri::command]
pub async fn trim_midi_clip(
    clip_id: String,
    start_tick: TimelineTick,
    duration_ticks: u64,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.trim_midi_clip(&clip_id, start_tick, duration_ticks)
        })
    })
    .await
}

#[tauri::command]
pub async fn split_midi_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.split_midi_clip(
                &clip_id,
                split_tick,
                format!("midi-clip:split:{stamp}:{}", arrangement.revision + 1),
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn duplicate_midi_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let stamp = crate::storage::now_ms();
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.duplicate_midi_clip(
                &clip_id,
                format!("midi-clip:duplicate:{stamp}:{}", arrangement.revision + 1),
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn paste_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: TimelineTick,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::paste_timeline_clips(
            &app_context(state),
            &audio_clip_ids,
            &midi_clip_ids,
            start_tick,
        )
    })
    .await
}

#[tauri::command]
pub async fn crossfade_audio_clips(
    first_id: String,
    second_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.crossfade_audio_clips(&first_id, &second_id)
        })
    })
    .await
}

#[tauri::command]
pub async fn sync_arrangement_runtime(app: AppHandle) -> Result<RuntimeProjectionStatus, String> {
    run_runtime_control(app, |state| {
        application::sync_arrangement_runtime(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn restore_sample_pads(app: AppHandle) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, |state| {
        application::restore_sample_pads(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn play_timeline(app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, |state| application::play_timeline(&app_context(state))).await
}

#[tauri::command]
pub async fn stop_timeline(app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, |state| application::stop_timeline(&app_context(state))).await
}

#[tauri::command]
pub async fn seek_timeline(tick: TimelineTick, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        application::seek_timeline(&app_context(state), tick)
    })
    .await
}

#[tauri::command]
pub async fn update_arrangement_timebase(
    timebase: ProjectTimebase,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_timebase(&app_context(state), timebase)
    })
    .await
}

#[tauri::command]
pub async fn update_timeline_loop_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.update_loop_range(enabled, start_tick, end_tick)
        })
    })
    .await
}

#[tauri::command]
pub async fn update_timeline_punch_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_arrangement_edit(&app_context(state), |arrangement| {
            arrangement.update_punch_range(enabled, start_tick, end_tick)
        })
    })
    .await
}

#[tauri::command]
pub async fn open_asset_in_design(
    asset_id: String,
    tool: DesignTool,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::open_asset_in_design(&app_context(state), asset_id, tool)
    })
    .await
}

#[tauri::command]
pub async fn switch_workspace(
    workspace: Workspace,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    // Navigation is view state and must not wait behind a durable Session
    // operation (or a slow VST-related command). The application operation
    // only performs a short in-memory workspace update and sends a best-effort
    // runtime mode.
    run_workspace_control(app, move |state| {
        application::switch_workspace(&app_context(state), workspace)
    })
    .await
}

#[tauri::command]
pub async fn update_session_settings(
    patch: application::SessionSettingsPatch,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_session_settings(&app_context(state), patch)
    })
    .await
}

#[tauri::command]
pub async fn add_track(
    name: String,
    kind: TrackKind,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::add_track(&app_context(state), name, kind)
    })
    .await
}

#[tauri::command]
pub async fn update_track(
    track_id: String,
    patch: application::TrackPatch,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_track(&app_context(state), &track_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn set_track_automation(
    track_id: String,
    parameter: AutomationParameter,
    points: Vec<AutomationPoint>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_automation(&app_context(state), &track_id, parameter, points)
    })
    .await
}

#[tauri::command]
pub async fn set_track_audio_input(
    track_id: String,
    channel_index: Option<u32>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_audio_input(&app_context(state), &track_id, channel_index)
    })
    .await
}

#[tauri::command]
pub async fn set_track_midi_input(
    track_id: String,
    route: MidiInputRoute,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_midi_input(&app_context(state), &track_id, route)
    })
    .await
}

#[tauri::command]
pub async fn set_track_instrument(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_instrument(&app_context(state), &track_id, &plugin_path)
    })
    .await
}

#[tauri::command]
pub async fn clear_track_instrument(
    track_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::clear_track_instrument(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn add_track_effect(
    track_id: String,
    plugin_path: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::add_track_effect(&app_context(state), &track_id, &plugin_path)
    })
    .await
}

#[tauri::command]
pub async fn remove_track_effect(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::remove_track_effect(&app_context(state), &track_id, &device_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_track_effects(
    track_id: String,
    ordered_device_ids: Vec<String>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::reorder_track_effects(&app_context(state), &track_id, &ordered_device_ids)
    })
    .await
}

#[tauri::command]
pub async fn set_track_device_bypassed(
    track_id: String,
    device_id: String,
    bypassed: bool,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_device_bypassed(&app_context(state), &track_id, &device_id, bypassed)
    })
    .await
}

#[tauri::command]
pub async fn set_track_device_parameter(
    track_id: String,
    device_id: String,
    parameter_index: u32,
    value: f32,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_track_device_parameter(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_index,
            value,
        )
    })
    .await
}

#[tauri::command]
pub async fn open_track_plugin_editor(
    track_id: String,
    device_id: String,
    app: AppHandle,
) -> Result<(), String> {
    // Opening an editor is a native lifecycle operation, not a canonical
    // Session mutation. It must not occupy the Session Actor while JUCE waits
    // for a third-party editor on the Message Thread.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        application::open_track_plugin_editor(&app_context(state.inner()), &track_id, &device_id)
    })
    .await
    .map_err(|error| format!("Track plugin editor operation failed: {error}"))?
}

#[tauri::command]
pub async fn persist_track_plugin_state(
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::persist_track_plugin_state(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_values,
            state_data,
            bypassed,
        )
    })
    .await
}

#[tauri::command]
pub async fn persist_track_plugin_parameter(
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::persist_track_plugin_parameter(
            &app_context(state),
            &track_id,
            &device_id,
            parameter_index,
            value,
        )
    })
    .await
}

#[tauri::command]
pub async fn remove_track(track_id: String, app: AppHandle) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::remove_track(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_track(track_id: String, app: AppHandle) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::duplicate_track(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_track(
    track_id: String,
    target_index: usize,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::reorder_track(&app_context(state), &track_id, target_index)
    })
    .await
}

#[tauri::command]
pub async fn add_marker(
    tick: TimelineTick,
    name: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::add_marker(&app_context(state), tick, name)
    })
    .await
}

#[tauri::command]
pub async fn update_marker(
    marker_id: String,
    name: Option<String>,
    tick: Option<TimelineTick>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_marker(&app_context(state), &marker_id, name, tick)
    })
    .await
}

#[tauri::command]
pub async fn remove_marker(marker_id: String, app: AppHandle) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::remove_marker(&app_context(state), &marker_id)
    })
    .await
}

#[tauri::command]
pub async fn add_midi_note(
    clip_id: String,
    start_tick: TimelineTick,
    pitch: u8,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::add_midi_note(
            &app_context(state),
            &clip_id,
            start_tick,
            pitch,
            duration_ticks,
            velocity,
            channel,
        )
    })
    .await
}

#[tauri::command]
pub async fn update_midi_note(
    clip_id: String,
    note_id: String,
    patch: application::MidiNotePatch,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_midi_note(&app_context(state), &clip_id, &note_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn update_midi_notes(
    clip_id: String,
    updates: Vec<application::MidiNoteUpdate>,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::update_midi_notes(&app_context(state), &clip_id, updates)
    })
    .await
}

#[tauri::command]
pub async fn remove_midi_note(
    clip_id: String,
    note_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::remove_midi_note(&app_context(state), &clip_id, &note_id)
    })
    .await
}

#[tauri::command]
pub async fn quantize_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::quantize_midi_notes(&app_context(state), &clip_id, &note_ids, grid_ticks)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::duplicate_midi_notes(&app_context(state), &clip_id, &note_ids, offset_ticks)
    })
    .await
}

#[tauri::command]
pub async fn set_audio_clip_take_variant(
    clip_id: String,
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::set_audio_clip_take_variant(&app_context(state), &clip_id, variant)
    })
    .await
}

#[tauri::command]
pub async fn start_take_comparison(
    take_id: String,
    app: AppHandle,
) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, move |state| {
        application::start_take_comparison(&app_context(state), &take_id)
    })
    .await
}

#[tauri::command]
pub async fn switch_take_comparison_variant(
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, move |state| {
        application::switch_take_comparison_variant(&app_context(state), variant)
    })
    .await
}

#[tauri::command]
pub async fn stop_take_comparison(app: AppHandle) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, |state| {
        application::stop_take_comparison(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn activate_take(
    session_id: String,
    take_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::activate_take(&app_context(state), &session_id, &take_id)
    })
    .await
}

#[tauri::command]
pub async fn place_take_as_separate_clip(
    take_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::place_take_as_separate_clip(&app_context(state), &take_id)
    })
    .await
}

#[tauri::command]
pub async fn apply_ai_suggestion(
    clip_id: String,
    proposed_gain_db: f64,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::apply_ai_suggestion(&app_context(state), &clip_id, proposed_gain_db)
    })
    .await
}

#[tauri::command]
pub async fn set_master_gain_db(gain_db: f64, app: AppHandle) -> Result<SessionAudioPair, String> {
    run_blocking(app, move |state| {
        application::set_master_gain_db(&app_context(state), gain_db)
    })
    .await
}

#[tauri::command]
pub async fn relink_missing_dependency(
    asset_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        application::relink_missing_dependency(&app_context(state), asset_id, &new_path)
    })
    .await
}

#[tauri::command]
pub async fn disable_missing_plugin(
    device_id: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::disable_missing_plugin(&app_context(state), &device_id)
    })
    .await
}

#[tauri::command]
pub async fn replace_missing_track_plugin(
    device_id: String,
    new_path: String,
    app: AppHandle,
) -> Result<CreativeSession, String> {
    run_blocking(app, move |state| {
        application::replace_missing_track_plugin(&app_context(state), &device_id, &new_path)
    })
    .await
}

#[tauri::command]
pub async fn get_missing_dependencies(app: AppHandle) -> Result<Vec<MissingDependency>, String> {
    run_blocking(app, |state| {
        let session = state.core.session().lock().map_err(lock_error)?.clone();
        Ok(crate::missing::collect_missing(
            state.core.data_root(),
            &session,
        ))
    })
    .await
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    let message = format!("An internal state lock was poisoned: {error}");
    eprintln!("[riffra] {message}. Aborting to prevent corrupted state from propagating.");
    std::process::abort();
}
