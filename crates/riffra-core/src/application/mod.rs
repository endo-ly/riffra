//! User-intent application operations over the canonical production state.

mod arrangement;
pub(crate) mod history;
mod rack;
mod recording;
mod session;
pub mod transport;

use crate::app::AppCore;
use crate::domain::asset::AssetId;
use crate::domain::rack::RackDevice;
use crate::domain::{
    AiChangeSet, AiPermission, Arrangement, AudioClip, AudioClipMove, AudioClipPatch,
    AudioInputRoute, AudioTakeVariant, AutomationLane, AutomationParameter, AutomationPoint,
    CreativeSession, FrameRange, Marker, MidiClip, MidiClipMove, MidiClipPatch, MidiInputRoute,
    MidiNote, ProjectTimebase, SamplePad, TakeAudioSource, TimelineTick, Track, TrackKind,
    TrackPatch,
};
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Partial update for session-wide production settings.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettingsPatch {
    pub project_name: Option<Option<String>>,
    pub master_db: Option<f64>,
    pub loop_enabled: Option<bool>,
    pub count_in_beats: Option<u8>,
    pub metronome_enabled: Option<bool>,
    pub note: Option<String>,
    pub ai_permission: Option<AiPermission>,
    pub ai_context: Option<Vec<String>>,
}

/// Partial update for one MIDI note.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNotePatch {
    pub note: Option<u8>,
    pub start_tick: Option<TimelineTick>,
    pub duration_ticks: Option<u64>,
    pub velocity: Option<u8>,
}

/// Partial update for one Sample Pad.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplePadPatch {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub gain_db: Option<f64>,
    pub loop_enabled: Option<bool>,
}

/// One note update within an atomic MIDI edit.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdate {
    pub note_id: String,
    pub patch: MidiNotePatch,
}

/// Partial update for a timeline marker.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerPatch {
    pub name: Option<String>,
    pub tick: Option<TimelineTick>,
}

/// Core application facade bound to one host-provided persistence Port.
pub struct Application<'a, A, S: ?Sized> {
    core: &'a AppCore<A>,
    storage: &'a S,
}

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    pub(crate) fn new(core: &'a AppCore<A>, storage: &'a S) -> Self {
        Self { core, storage }
    }

    fn commit_arrangement<F>(&self, edit: F) -> Result<CreativeSession, ApplicationError>
    where
        F: FnOnce(&mut Arrangement) -> Result<(), ApplicationError>,
    {
        self.core
            .commit(self.storage, |session| edit(&mut session.arrangement))
    }
}

fn merge_recording_vector<T: Clone + PartialEq>(
    current: &[T],
    base: &[T],
    candidate: &[T],
    key: impl Fn(&T) -> String,
) -> Vec<T> {
    let mut merged = current.to_vec();
    for item in candidate {
        let item_key = key(item);
        let changed = base
            .iter()
            .find(|previous| key(previous) == item_key)
            .is_none_or(|previous| previous != item);
        if !changed {
            continue;
        }
        if let Some(index) = merged.iter().position(|existing| key(existing) == item_key) {
            merged[index] = item.clone();
        } else {
            merged.push(item.clone());
        }
    }
    merged
}

fn merge_recording_session(
    current: &CreativeSession,
    _base: &CreativeSession,
    candidate: CreativeSession,
) -> CreativeSession {
    let mut merged = current.clone();
    let mut arrangement = current.arrangement.clone();
    arrangement.midi_clips = merge_recording_vector(
        &current.arrangement.midi_clips,
        &_base.arrangement.midi_clips,
        &candidate.arrangement.midi_clips,
        |clip| clip.id.clone(),
    );
    arrangement.audio_clips = merge_recording_vector(
        &current.arrangement.audio_clips,
        &_base.arrangement.audio_clips,
        &candidate.arrangement.audio_clips,
        |clip| clip.id.clone(),
    );
    arrangement.recording_passes = merge_recording_vector(
        &current.arrangement.recording_passes,
        &_base.arrangement.recording_passes,
        &candidate.arrangement.recording_passes,
        |pass| pass.id.clone(),
    );
    arrangement.takes = merge_recording_vector(
        &current.arrangement.takes,
        &_base.arrangement.takes,
        &candidate.arrangement.takes,
        |take| take.id.clone(),
    );
    arrangement.recording_sessions = merge_recording_vector(
        &current.arrangement.recording_sessions,
        &_base.arrangement.recording_sessions,
        &candidate.arrangement.recording_sessions,
        |recording| recording.id.clone(),
    );
    arrangement.revision = current.arrangement.revision.saturating_add(1);
    merged.arrangement = arrangement;
    merged
}

fn find_track_device_mut<'a>(
    session: &'a mut CreativeSession,
    track_id: &str,
    device_id: &str,
) -> Result<&'a mut RackDevice, ApplicationError> {
    let track = session
        .arrangement
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
    if track
        .instrument
        .as_ref()
        .is_some_and(|device| device.id == device_id)
    {
        return track.instrument.as_mut().ok_or_else(|| {
            ApplicationError::InvalidCommand("track device is not registered".into())
        });
    }
    track
        .rack
        .devices
        .iter_mut()
        .find(|device| device.id == device_id)
        .ok_or_else(|| ApplicationError::InvalidCommand("track device is not registered".into()))
}

fn find_sample_pad_mut<'a>(
    session: &'a mut CreativeSession,
    pad_id: &str,
) -> Result<&'a mut SamplePad, ApplicationError> {
    session
        .play_state
        .sample_instrument
        .pads
        .iter_mut()
        .find(|pad| pad.id == pad_id)
        .ok_or_else(|| {
            ApplicationError::InvalidCommand(format!("sample pad is not registered: {pad_id}"))
        })
}

fn validate_sample_pad_add(
    session: &CreativeSession,
    pad: &SamplePad,
) -> Result<(), ApplicationError> {
    if session.play_state.sample_instrument.pads.len() >= 128 {
        return Err(ApplicationError::InvalidCommand(
            "sample instrument cannot contain more than 128 pads".into(),
        ));
    }
    if pad.id.trim().is_empty() || pad.name.trim().is_empty() {
        return Err(ApplicationError::InvalidCommand(
            "sample pads require non-empty ids and names".into(),
        ));
    }
    if session
        .play_state
        .sample_instrument
        .pads
        .iter()
        .any(|existing| existing.id == pad.id || existing.asset_id == pad.asset_id)
    {
        return Err(ApplicationError::InvalidCommand(
            "sample pad id and asset must be unique".into(),
        ));
    }
    Ok(())
}

fn apply_sample_pad_patch(pad: &mut SamplePad, patch: &SamplePadPatch) {
    if let Some(gain_db) = patch.gain_db {
        pad.gain_db = if gain_db.is_finite() {
            gain_db.clamp(-90.0, 24.0)
        } else {
            0.0
        };
    }
    if let Some(loop_enabled) = patch.loop_enabled {
        pad.loop_enabled = loop_enabled;
    }
    match (patch.start_ms, patch.end_ms) {
        (Some(start), None) => {
            pad.start_ms = start;
            pad.end_ms = pad.end_ms.max(start.saturating_add(1));
        }
        (None, Some(end)) => {
            let end = end.max(1);
            pad.end_ms = end;
            pad.start_ms = pad.start_ms.min(end - 1);
        }
        (Some(start), Some(end)) => {
            pad.start_ms = start;
            pad.end_ms = end.max(start.saturating_add(1));
        }
        (None, None) => {}
    }
}

fn find_any_track_device_mut<'a>(
    session: &'a mut CreativeSession,
    device_id: &str,
) -> Result<&'a mut RackDevice, ApplicationError> {
    session
        .arrangement
        .tracks
        .iter_mut()
        .find_map(|track| {
            if track
                .instrument
                .as_ref()
                .is_some_and(|device| device.id == device_id)
            {
                track.instrument.as_mut()
            } else {
                track
                    .rack
                    .devices
                    .iter_mut()
                    .find(|device| device.id == device_id)
            }
        })
        .ok_or_else(|| ApplicationError::InvalidCommand("track device is not registered".into()))
}

fn next_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::now_v7())
}

fn apply_audio_clip_take_variant(
    session: &mut CreativeSession,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<(), String> {
    let take_id = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .and_then(|clip| clip.recording_take_id.clone())
        .ok_or_else(|| format!("audio clip has no recording take: {clip_id}"))?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .ok_or_else(|| format!("recording take is not registered: {take_id}"))?;
    let source = take
        .audio_source(variant)
        .cloned()
        .ok_or_else(|| "the requested take variant is not available".to_string())?;
    let clip = session
        .arrangement
        .audio_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("audio clip is not registered: {clip_id}"))?;
    let prior_duration = clip.timeline_duration;
    let target_frames = if prior_duration.sample_rate > 0 && source.sample_rate > 0 {
        ((prior_duration.frames as f64 * f64::from(source.sample_rate)
            / f64::from(prior_duration.sample_rate))
        .round() as u64)
            .max(1)
    } else {
        source
            .source_end_sample
            .saturating_sub(source.source_start_sample)
    };
    let mut selected_source = source;
    if selected_source
        .source_end_sample
        .saturating_sub(selected_source.source_start_sample)
        != target_frames
        && selected_source
            .source_start_sample
            .saturating_add(target_frames)
            <= selected_source.source_end_sample
    {
        selected_source.source_end_sample = selected_source
            .source_start_sample
            .saturating_add(target_frames);
    }
    apply_audio_source_to_clip(clip, &selected_source);
    clip.take_variant = variant;
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    Ok(())
}

fn apply_audio_source_to_clip(clip: &mut AudioClip, source: &TakeAudioSource) {
    clip.asset_id = source.asset_id.clone();
    clip.source_range.start = source.source_start_sample;
    clip.source_range.end = source.source_end_sample;
    clip.timeline_duration.frames = source
        .source_end_sample
        .saturating_sub(source.source_start_sample);
    clip.source_sample_rate = source.sample_rate;
    clip.timeline_duration.sample_rate = source.sample_rate;
    clip.fade_in.sample_rate = source.sample_rate;
    clip.fade_out.sample_rate = source.sample_rate;
    clip.normalize_fields();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .min(u128::from(u64::MAX)) as u64
}
