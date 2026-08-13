//! Arrangement command adapters.

use super::*;

pub fn update_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    patch: AudioClipPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_audio_clip(clip_id, patch)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn split_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    split_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .split_audio_clip(clip_id, split_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_audio_clip(clip_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn move_audio_clips(
    context: &SessionContext<'_>,
    moves: Vec<AudioClipMove>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).move_audio_clips(moves)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    patch: MidiClipPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_midi_clip(clip_id, patch)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn move_midi_clips(
    context: &SessionContext<'_>,
    moves: Vec<MidiClipMove>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).move_midi_clips(moves)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn trim_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    duration_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .trim_midi_clip(clip_id, start_tick, duration_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn split_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    split_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).split_midi_clip(clip_id, split_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_midi_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_midi_clip(clip_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn crossfade_audio_clips(
    context: &SessionContext<'_>,
    first_id: &str,
    second_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .crossfade_audio_clips(first_id, second_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_timebase(
    context: &SessionContext<'_>,
    timebase: ProjectTimebase,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_timebase(timebase)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_loop_range(
    context: &SessionContext<'_>,
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .update_loop_range(enabled, start_tick, end_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_punch_range(
    context: &SessionContext<'_>,
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .update_punch_range(enabled, start_tick, end_tick)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_timeline_clips(
    context: &SessionContext<'_>,
    audio_clip_ids: &[String],
    midi_clip_ids: &[String],
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .remove_timeline_clips(audio_clip_ids.to_owned(), midi_clip_ids.to_owned())
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn paste_timeline_clips(
    context: &SessionContext<'_>,
    audio_clip_ids: &[String],
    midi_clip_ids: &[String],
    start_tick: TimelineTick,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).paste_timeline_clips(
            audio_clip_ids.to_owned(),
            midi_clip_ids.to_owned(),
            start_tick,
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn trim_audio_clip(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    source_range: riffra_core::FrameRange,
) -> Result<CreativeSession, String> {
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let clip = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Audio clip '{clip_id}' not found."))?;
    let source_asset = asset::load(context.data_root, &clip.asset_id)
        .ok_or_else(|| format!("Audio Asset is not registered: {}", clip.asset_id))?;
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
    let wav = crate::analysis::parse_wav(&bytes)?;
    let frame_bytes = usize::from(wav.bits_per_sample / 8) * usize::from(wav.channels);
    if frame_bytes == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    let committed = commit_core_application(context, |core, store| {
        core.application(store).trim_audio_clip(
            clip_id,
            start_tick,
            source_range,
            (wav.data_len / frame_bytes) as u64,
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

/// Adds an audio clip referencing a canonical Asset to the arrangement, then
/// commits the session and switches to the Arrange workspace.
pub fn add_audio_clip(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    name: String,
    start_tick: Option<TimelineTick>,
    track_id: Option<String>,
) -> Result<CreativeSession, String> {
    if name.trim().is_empty() {
        return Err("Audio clip name must not be empty.".into());
    }
    let source_asset = asset::load(context.data_root, &asset_id)
        .ok_or_else(|| format!("Audio Asset is not registered: {asset_id}"))?;
    if source_asset.kind != AssetKind::Audio {
        return Err(format!("Asset {asset_id} is not an audio Asset."));
    }
    let bytes = fs::read(&source_asset.content_location)
        .map_err(|error| format!("Audio Asset could not be read: {error}"))?;
    let wav = crate::analysis::parse_wav(&bytes)?;
    let bytes_per_sample = usize::from(wav.bits_per_sample / 8);
    let frame_bytes = bytes_per_sample.saturating_mul(usize::from(wav.channels));
    if frame_bytes == 0 || wav.sample_rate == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    let source_frames = (wav.data_len / frame_bytes) as u64;
    if source_frames == 0 {
        return Err("Audio Asset has no usable frames.".into());
    }
    let committed = commit_core_application(context, |core, store| {
        core.application(store).add_audio_asset_clip(
            AudioAssetClipPlacement {
                asset_id,
                name,
                start_tick,
                track_id,
                sample_rate: wav.sample_rate,
                source_frames,
            },
            |id| asset::load(context.data_root, id).is_some(),
        )
    })?;
    context.view_state.lock().map_err(lock_error)?.workspace = Workspace::Arrange;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_session_settings(
    context: &SessionContext<'_>,
    patch: SessionSettingsPatch,
) -> Result<CreativeSession, String> {
    let metronome_changed = patch.metronome_enabled.is_some();
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_session_settings(patch)
    })?;
    if metronome_changed {
        sync_arrangement(context)?;
    }
    Ok(committed)
}

pub fn add_track(
    context: &SessionContext<'_>,
    name: String,
    kind: TrackKind,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).add_track(name, kind)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_track<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    track_id: &str,
    patch: TrackPatch,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_track(track_id, patch)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

/// Replaces one Track Automation Lane in a single canonical edit.
///
/// The UI previews pointer movement locally and calls this once on pointer-up.
/// An empty point list removes the lane so the Track's regular value applies.
pub fn set_track_automation(
    context: &SessionContext<'_>,
    track_id: &str,
    parameter: AutomationParameter,
    points: Vec<AutomationPoint>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_track_automation(track_id, parameter, points)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Removes a Track and its Clips without deleting any referenced Asset.
pub fn remove_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).remove_track(track_id)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Duplicates a Track and its non-destructive Clip references.
pub fn duplicate_track(
    context: &SessionContext<'_>,
    track_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).duplicate_track(track_id)
    })?;
    let _ = sync_arrangement(context)?;
    Ok(committed)
}

/// Moves a Track to a zero-based position while preserving Clip ownership.
pub fn reorder_track(
    context: &SessionContext<'_>,
    track_id: &str,
    target_index: usize,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .reorder_track(track_id, target_index)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

// Marker operations. Markers are timeline authoring metadata with no audio
// runtime impact, so they skip the audio sync and go straight through Core.

pub fn add_marker(
    context: &SessionContext<'_>,
    tick: TimelineTick,
    name: String,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store).add_marker(tick, name)
    })
}

pub fn update_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
    name: Option<String>,
    tick: Option<TimelineTick>,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .update_marker(marker_id, MarkerPatch { name, tick })
    })
}

pub fn remove_marker(
    context: &SessionContext<'_>,
    marker_id: &str,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store).remove_marker(marker_id)
    })
}

/// Adds a single MIDI note to an existing MIDI clip. The note id is minted by
/// the Application layer so the React side never invents identity.
pub fn add_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    start_tick: TimelineTick,
    pitch: u8,
    duration_ticks: u64,
    velocity: u8,
    channel: u8,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).add_midi_note(
            clip_id,
            start_tick,
            pitch,
            duration_ticks,
            velocity,
            channel,
        )
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn update_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_id: &str,
    patch: MidiNotePatch,
) -> Result<CreativeSession, String> {
    update_midi_notes(
        context,
        clip_id,
        vec![MidiNoteUpdate {
            note_id: note_id.to_owned(),
            patch,
        }],
    )
}

pub fn update_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    updates: Vec<MidiNoteUpdate>,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).update_midi_notes(clip_id, updates)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn remove_midi_note(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_id: &str,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store).remove_midi_note(clip_id, note_id)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn quantize_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    grid_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .quantize_midi_notes(clip_id, note_ids.to_owned(), grid_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn duplicate_midi_notes(
    context: &SessionContext<'_>,
    clip_id: &str,
    note_ids: &[String],
    offset_ticks: u64,
) -> Result<CreativeSession, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .duplicate_midi_notes(clip_id, note_ids.to_owned(), offset_ticks)
    })?;
    sync_arrangement(context)?;
    Ok(committed)
}

pub fn apply_ai_suggestion(
    context: &SessionContext<'_>,
    clip_id: &str,
    proposed_gain_db: f64,
) -> Result<CreativeSession, String> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .apply_ai_suggestion(clip_id, proposed_gain_db)
    })
}

// Audio + Session coupling operations.
//
// `set_master_gain_db` changes an Audio Runtime setting and a session preference
// at the same time. Audio-device preferences are application settings and live
// outside the CreativeSession.
