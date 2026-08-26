use super::*;

#[tauri::command]
pub async fn send_midi_to_track(
    track_id: String,
    bytes: Vec<u8>,
    app: AppHandle,
) -> Result<(), String> {
    dispatch(
        app,
        "midi.send",
        json!({ "trackId": track_id, "bytes": bytes }),
    )
    .await
}

#[tauri::command]
pub async fn panic_midi_track(track_id: String, app: AppHandle) -> Result<(), String> {
    dispatch(app, "midi.panic", json!({ "trackId": track_id })).await
}

#[tauri::command]
pub async fn add_audio_clip_to_arrangement(
    asset_id: String,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.add-asset",
        json!({
            "assetId": asset_id,
            "name": name,
            "startTick": start_tick.map(|tick| tick.0),
            "trackId": track_id,
        }),
    )
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
    dispatch(
        app,
        "midi-clip.create",
        json!({
            "trackId": track_id,
            "startTick": start_tick.0,
            "durationTicks": duration_ticks,
            "name": name,
        }),
    )
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
    dispatch(
        app,
        "midi-clip.add-asset",
        json!({
            "assetId": asset_id,
            "name": name,
            "startTick": start_tick.map(|tick| tick.0),
            "trackId": track_id,
        }),
    )
    .await
}

#[tauri::command]
pub async fn update_audio_clip(
    clip_id: String,
    patch: AudioClipPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.update",
        json!({ "clipId": clip_id, "patch": patch }),
    )
    .await
}

#[tauri::command]
pub async fn remove_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "clip.remove",
        json!({ "audioClipIds": audio_clip_ids, "midiClipIds": midi_clip_ids }),
    )
    .await
}

#[tauri::command]
pub async fn trim_audio_clip(
    clip_id: String,
    start_tick: TimelineTick,
    source_range: FrameRange,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.trim",
        json!({ "clipId": clip_id, "startTick": start_tick.0, "sourceRange": source_range }),
    )
    .await
}

#[tauri::command]
pub async fn split_audio_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.split",
        json!({ "clipId": clip_id, "splitTick": split_tick.0 }),
    )
    .await
}

#[tauri::command]
pub async fn duplicate_audio_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "audio-clip.duplicate", json!({ "clipId": clip_id })).await
}

#[tauri::command]
pub async fn move_audio_clips(
    moves: Vec<AudioClipMove>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "audio-clip.move", json!({ "moves": moves })).await
}

#[tauri::command]
pub async fn update_midi_clip(
    clip_id: String,
    patch: MidiClipPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-clip.update",
        json!({ "clipId": clip_id, "patch": patch }),
    )
    .await
}

#[tauri::command]
pub async fn move_midi_clips(
    moves: Vec<MidiClipMove>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "midi-clip.move", json!({ "moves": moves })).await
}

#[tauri::command]
pub async fn trim_midi_clip(
    clip_id: String,
    start_tick: TimelineTick,
    duration_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-clip.trim",
        json!({ "clipId": clip_id, "startTick": start_tick.0, "durationTicks": duration_ticks }),
    )
    .await
}

#[tauri::command]
pub async fn split_midi_clip(
    clip_id: String,
    split_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-clip.split",
        json!({ "clipId": clip_id, "splitTick": split_tick.0 }),
    )
    .await
}

#[tauri::command]
pub async fn duplicate_midi_clip(
    clip_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "midi-clip.duplicate", json!({ "clipId": clip_id })).await
}

#[tauri::command]
pub async fn paste_timeline_clips(
    audio_clip_ids: Vec<String>,
    midi_clip_ids: Vec<String>,
    start_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "clip.paste",
        json!({
            "audioClipIds": audio_clip_ids,
            "midiClipIds": midi_clip_ids,
            "startTick": start_tick.0,
        }),
    )
    .await
}

#[tauri::command]
pub async fn crossfade_audio_clips(
    first_id: String,
    second_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.crossfade",
        json!({ "firstClipId": first_id, "secondClipId": second_id }),
    )
    .await
}

#[tauri::command]
pub async fn update_arrangement_timebase(
    timebase: ProjectTimebase,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "timebase.update", timebase).await
}

#[tauri::command]
pub async fn update_timeline_loop_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "loop-range.set",
        json!({ "enabled": enabled, "startTick": start_tick.0, "endTick": end_tick.0 }),
    )
    .await
}

#[tauri::command]
pub async fn update_timeline_punch_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "punch-range.set",
        json!({ "enabled": enabled, "startTick": start_tick.0, "endTick": end_tick.0 }),
    )
    .await
}

#[tauri::command]
pub async fn add_track(
    name: String,
    kind: TrackKind,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "track.add", json!({ "name": name, "kind": kind })).await
}

#[tauri::command]
pub async fn update_track(
    track_id: String,
    patch: riffra_core::TrackPatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    let mut params = serde_json::to_value(patch).map_err(|error| error.to_string())?;
    params["trackId"] = Value::String(track_id);
    dispatch_json(app, "track.update", params).await
}

#[tauri::command]
pub async fn set_track_automation(
    track_id: String,
    parameter: AutomationParameter,
    points: Vec<AutomationPoint>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "automation.set",
        json!({ "trackId": track_id, "parameter": parameter, "points": points }),
    )
    .await
}

#[tauri::command]
pub async fn set_track_audio_input(
    track_id: String,
    channel_index: Option<u32>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    let command = if channel_index.is_some() {
        "track.audio-input.set"
    } else {
        "track.audio-input.clear"
    };
    dispatch(
        app,
        command,
        json!({ "trackId": track_id, "channelIndex": channel_index }),
    )
    .await
}

#[tauri::command]
pub async fn set_track_midi_input(
    track_id: String,
    route: MidiInputRoute,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    let command = if route.device_id.is_some() || route.channel.is_some() {
        "track.midi-input.set"
    } else {
        "track.midi-input.clear"
    };
    dispatch(
        app,
        command,
        json!({ "trackId": track_id, "deviceId": route.device_id, "channel": route.channel }),
    )
    .await
}

#[tauri::command]
pub async fn remove_track(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "track.remove", json!({ "trackId": track_id })).await
}

#[tauri::command]
pub async fn duplicate_track(
    track_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "track.duplicate", json!({ "trackId": track_id })).await
}

#[tauri::command]
pub async fn reorder_track(
    track_id: String,
    target_index: usize,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "track.reorder",
        json!({ "trackId": track_id, "targetIndex": target_index }),
    )
    .await
}

#[tauri::command]
pub async fn add_marker(
    tick: TimelineTick,
    name: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "marker.add", json!({ "tick": tick.0, "name": name })).await
}

#[tauri::command]
pub async fn update_marker(
    marker_id: String,
    name: Option<String>,
    tick: Option<TimelineTick>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "marker.update",
        json!({ "markerId": marker_id, "name": name, "tick": tick.map(|value| value.0) }),
    )
    .await
}

#[tauri::command]
pub async fn remove_marker(
    marker_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(app, "marker.remove", json!({ "markerId": marker_id })).await
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
    dispatch(
        app,
        "midi-note.add",
        json!({
            "clipId": clip_id,
            "startTick": start_tick.0,
            "pitch": pitch,
            "durationTicks": duration_ticks,
            "velocity": velocity,
            "channel": channel,
        }),
    )
    .await
}

#[tauri::command]
pub async fn insert_midi_notes(
    clip_id: String,
    notes: Vec<MidiNoteInput>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.insert",
        json!({ "clipId": clip_id, "notes": notes }),
    )
    .await
}

#[tauri::command]
pub async fn update_midi_note(
    clip_id: String,
    note_id: String,
    patch: MidiNotePatch,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.update",
        json!({ "clipId": clip_id, "noteId": note_id, "patch": patch }),
    )
    .await
}

#[tauri::command]
pub async fn update_midi_notes(
    clip_id: String,
    updates: Vec<MidiNoteUpdate>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.update-many",
        json!({ "clipId": clip_id, "updates": updates }),
    )
    .await
}

#[tauri::command]
pub async fn remove_midi_note(
    clip_id: String,
    note_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.remove",
        json!({ "clipId": clip_id, "noteId": note_id }),
    )
    .await
}

#[tauri::command]
pub async fn remove_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.remove-many",
        json!({ "clipId": clip_id, "noteIds": note_ids }),
    )
    .await
}

#[tauri::command]
pub async fn quantize_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    grid_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.quantize",
        json!({ "clipId": clip_id, "noteIds": note_ids, "gridTicks": grid_ticks }),
    )
    .await
}

#[tauri::command]
pub async fn transform_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    transpose_semitones: i16,
    velocity_offset: i16,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.transform",
        json!({
            "clipId": clip_id,
            "noteIds": note_ids,
            "transposeSemitones": transpose_semitones,
            "velocityOffset": velocity_offset,
        }),
    )
    .await
}

#[tauri::command]
pub async fn duplicate_midi_notes(
    clip_id: String,
    note_ids: Vec<String>,
    offset_ticks: u64,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "midi-note.duplicate",
        json!({ "clipId": clip_id, "noteIds": note_ids, "offsetTicks": offset_ticks }),
    )
    .await
}

#[tauri::command]
pub async fn set_audio_clip_take_variant(
    clip_id: String,
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "audio-clip.take-variant.set",
        json!({ "clipId": clip_id, "variant": variant }),
    )
    .await
}

#[tauri::command]
pub async fn start_take_comparison(take_id: String, app: AppHandle) -> Result<AudioStatus, String> {
    dispatch(app, "take.comparison.start", json!({ "takeId": take_id })).await
}

#[tauri::command]
pub async fn switch_take_comparison_variant(
    variant: AudioTakeVariant,
    app: AppHandle,
) -> Result<AudioStatus, String> {
    dispatch(app, "take.comparison.switch", json!({ "variant": variant })).await
}

#[tauri::command]
pub async fn stop_take_comparison(app: AppHandle) -> Result<AudioStatus, String> {
    dispatch(app, "take.comparison.stop", json!({})).await
}

#[tauri::command]
pub async fn activate_take(
    session_id: String,
    take_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "take.activate",
        json!({ "sessionId": session_id, "takeId": take_id }),
    )
    .await
}

#[tauri::command]
pub async fn place_take_as_separate_clip(
    take_id: String,
    app: AppHandle,
) -> Result<ArrangementMutationResult, String> {
    dispatch(
        app,
        "take.place-separate-clip",
        json!({ "takeId": take_id }),
    )
    .await
}
