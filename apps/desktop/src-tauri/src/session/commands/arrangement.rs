use super::*;

#[tauri::command]
pub async fn send_midi_to_track(
    track_id: String,
    bytes: Vec<u8>,
    app: AppHandle,
) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        validate_target_instrument_track(state, &track_id)?;
        state
            .core
            .audio()
            .send_track_midi(&track_id, &bytes)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn panic_midi_track(track_id: String, app: AppHandle) -> Result<(), String> {
    run_runtime_control(app, move |state| {
        validate_target_instrument_track(state, &track_id)?;
        state
            .core
            .audio()
            .panic_track_midi(&track_id)
            .map_err(|error| error.to_string())
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
) -> Result<ArrangementMutationResult, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        adapter::add_audio_clip(&app_context(state), asset_id, name, start_tick, track_id)
    })
    .await
}

#[tauri::command]
pub async fn create_midi_clip(
    track_id: String,
    start_tick: TimelineTick,
    duration_ticks: u64,
    name: Option<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::create_midi_clip(
            &app_context(state),
            &track_id,
            start_tick,
            duration_ticks,
            name,
        )
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
) -> Result<ArrangementMutationResult, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking(app, move |state| {
        adapter::add_midi_clip(&app_context(state), asset_id, name, start_tick, track_id)
    })
    .await
}

#[tauri::command]
pub async fn update_audio_clip(
    clip_id: String,
    patch: AudioClipPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_audio_clip(&app_context(state), &clip_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn remove_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_timeline_clips(&app_context(state), &audio_clip_ids, &midi_clip_ids)
    })
    .await
}

#[tauri::command]
pub async fn trim_audio_clip(
    clip_id: String,
    start_tick: TimelineTick,
    source_range: FrameRange,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::trim_audio_clip(&app_context(state), &clip_id, start_tick, source_range)
    })
    .await
}

#[tauri::command]
pub async fn split_audio_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::split_audio_clip(&app_context(state), &clip_id, split_tick)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_audio_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::duplicate_audio_clip(&app_context(state), &clip_id)
    })
    .await
}

#[tauri::command]
pub async fn move_audio_clips(
    moves: Vec<AudioClipMove>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::move_audio_clips(&app_context(state), moves)
    })
    .await
}

#[tauri::command]
pub async fn update_midi_clip(
    clip_id: String,
    patch: MidiClipPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_midi_clip(&app_context(state), &clip_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn move_midi_clips(
    moves: Vec<MidiClipMove>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::move_midi_clips(&app_context(state), moves)
    })
    .await
}

#[tauri::command]
pub async fn trim_midi_clip(
    clip_id: String,
    start_tick: TimelineTick,
    duration_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::trim_midi_clip(&app_context(state), &clip_id, start_tick, duration_ticks)
    })
    .await
}

#[tauri::command]
pub async fn split_midi_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::split_midi_clip(&app_context(state), &clip_id, split_tick)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_midi_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::duplicate_midi_clip(&app_context(state), &clip_id)
    })
    .await
}

#[tauri::command]
pub async fn paste_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::paste_timeline_clips(
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
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::crossfade_audio_clips(&app_context(state), &first_id, &second_id)
    })
    .await
}

#[tauri::command]
pub async fn update_arrangement_timebase(
    timebase: ProjectTimebase,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_timebase(&app_context(state), timebase)
    })
    .await
}

#[tauri::command]
pub async fn update_timeline_loop_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_loop_range(&app_context(state), enabled, start_tick, end_tick)
    })
    .await
}

#[tauri::command]
pub async fn update_timeline_punch_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_punch_range(&app_context(state), enabled, start_tick, end_tick)
    })
    .await
}

#[tauri::command]
pub async fn add_track(
    name: String,
    kind: TrackKind,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::add_track(&app_context(state), name, kind)
    })
    .await
}

#[tauri::command]
pub async fn update_track(
    track_id: String,
    patch: riffra_core::TrackPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_track(&app_context(state), &track_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn set_track_automation(
    track_id: String,
    parameter: AutomationParameter,
    points: Vec<AutomationPoint>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::set_track_automation(&app_context(state), &track_id, parameter, points)
    })
    .await
}

#[tauri::command]
pub async fn set_track_audio_input(
    track_id: String,
    channel_index: Option<u32>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::set_track_audio_input(&app_context(state), &track_id, channel_index)
    })
    .await
}

#[tauri::command]
pub async fn set_track_midi_input(
    track_id: String,
    route: MidiInputRoute,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::set_track_midi_input(&app_context(state), &track_id, route)
    })
    .await
}

#[tauri::command]
pub async fn remove_track(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_track(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_track(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::duplicate_track(&app_context(state), &track_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_track(
    track_id: String,
    target_index: usize,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::reorder_track(&app_context(state), &track_id, target_index)
    })
    .await
}

#[tauri::command]
pub async fn add_marker(
    tick: TimelineTick,
    name: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::add_marker(&app_context(state), tick, name)
    })
    .await
}

#[tauri::command]
pub async fn update_marker(
    marker_id: String,
    name: Option<String>,
    tick: Option<TimelineTick>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_marker(&app_context(state), &marker_id, name, tick)
    })
    .await
}

#[tauri::command]
pub async fn remove_marker(
    marker_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_marker(&app_context(state), &marker_id)
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
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::add_midi_note(
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
pub async fn insert_midi_notes(
    clip_id: String,
    notes: Vec<MidiNoteInput>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::insert_midi_notes(&app_context(state), &clip_id, notes)
    })
    .await
}

#[tauri::command]
pub async fn update_midi_note(
    clip_id: String,
    note_id: String,
    patch: MidiNotePatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_midi_note(&app_context(state), &clip_id, &note_id, patch)
    })
    .await
}

#[tauri::command]
pub async fn update_midi_notes(
    clip_id: String,
    updates: Vec<MidiNoteUpdate>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::update_midi_notes(&app_context(state), &clip_id, updates)
    })
    .await
}

#[tauri::command]
pub async fn remove_midi_note(
    clip_id: String,
    note_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_midi_note(&app_context(state), &clip_id, &note_id)
    })
    .await
}

#[tauri::command]
pub async fn remove_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::remove_midi_notes(&app_context(state), &clip_id, &note_ids)
    })
    .await
}

#[tauri::command]
pub async fn quantize_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::quantize_midi_notes(&app_context(state), &clip_id, &note_ids, grid_ticks)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::duplicate_midi_notes(&app_context(state), &clip_id, &note_ids, offset_ticks)
    })
    .await
}

#[tauri::command]
pub async fn set_audio_clip_take_variant(
    clip_id: String,
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::set_audio_clip_take_variant(&app_context(state), &clip_id, variant)
    })
    .await
}

#[tauri::command]
pub async fn start_take_comparison(
    take_id: String,
    app: AppHandle,
) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, move |state| {
        adapter::start_take_comparison(&app_context(state), &take_id)
    })
    .await
}

#[tauri::command]
pub async fn switch_take_comparison_variant(
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, move |state| {
        adapter::switch_take_comparison_variant(&app_context(state), variant)
    })
    .await
}

#[tauri::command]
pub async fn stop_take_comparison(app: AppHandle) -> Result<crate::model::AudioStatus, String> {
    run_blocking(app, |state| {
        adapter::stop_take_comparison(&app_context(state))
    })
    .await
}

#[tauri::command]
pub async fn activate_take(
    session_id: String,
    take_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::activate_take(&app_context(state), &session_id, &take_id)
    })
    .await
}

#[tauri::command]
pub async fn place_take_as_separate_clip(
    take_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    run_blocking(app, move |state| {
        adapter::place_take_as_separate_clip(&app_context(state), &take_id)
    })
    .await
}
