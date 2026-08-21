//! User-intent application operations over the canonical production state.

mod arrangement;
pub(crate) mod history;
mod rack;
mod recording;
mod session;
pub mod transport;

use crate::PreparedSession;
use crate::app::AppCore;
use crate::domain::asset::AssetId;
use crate::domain::rack::RackDevice;
use crate::domain::{
    Arrangement, AudioClip, AudioClipMove, AudioClipPatch, AudioInputRoute, AudioTakeVariant,
    AutomationLane, AutomationParameter, AutomationPoint, CreativeSession, DeviceKind, FrameRange,
    Marker, MidiClip, MidiClipMove, MidiClipPatch, MidiEvent, MidiInputRoute, MidiNote,
    ProjectTimebase, TakeAudioSource, TimelineTick, Track, TrackKind, TrackPatch,
};
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;
use std::collections::HashSet;
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

/// One note update within an atomic MIDI edit.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteUpdate {
    pub note_id: String,
    pub patch: MidiNotePatch,
}

/// Identity-free MIDI note data accepted by a Core insertion operation.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNoteInput {
    pub pitch: u8,
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// Partial update for a timeline marker.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerPatch {
    pub name: Option<String>,
    pub tick: Option<TimelineTick>,
}

/// Host-resolved audio metadata required to place one Asset on the timeline.
pub struct AudioAssetClipPlacement {
    /// Canonical Audio Asset identity.
    pub asset_id: AssetId,
    /// User-facing Clip name.
    pub name: String,
    /// Explicit timeline position, or the end of current Audio Clips.
    pub start_tick: Option<TimelineTick>,
    /// Requested Audio Track, or automatic Track selection.
    pub track_id: Option<String>,
    /// Source sample rate read by the host adapter.
    pub sample_rate: u32,
    /// Source frame count read by the host adapter.
    pub source_frames: u64,
}

/// Parsed MIDI content required to place one Asset on the timeline.
pub struct MidiAssetClipPlacement {
    /// Canonical MIDI Asset identity.
    pub asset_id: AssetId,
    /// User-facing Clip name.
    pub name: String,
    /// Explicit timeline position, or the arrangement origin.
    pub start_tick: Option<TimelineTick>,
    /// Requested Instrument Track, or automatic Track selection.
    pub track_id: Option<String>,
    /// Parsed Clip duration in project ticks.
    pub duration_ticks: u64,
    /// Parsed notes whose transient identities Core replaces.
    pub notes: Vec<MidiNote>,
    /// Parsed events whose transient identities Core replaces.
    pub events: Vec<MidiEvent>,
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

    /// Commits the exact Core-produced candidate previously validated by a
    /// host runtime, provided its canonical base is still current.
    ///
    /// # Errors
    /// Returns an error when the canonical base changed or persistence fails.
    pub fn commit_prepared(
        &self,
        prepared: PreparedSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit_prepared(self.storage, prepared)
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

fn is_valid_track_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

fn normalize_track_name(name: String) -> Result<String, ApplicationError> {
    let name = name.trim().chars().take(80).collect::<String>();
    if name.is_empty() {
        return Err(ApplicationError::InvalidCommand(
            "track name must not be empty".into(),
        ));
    }
    Ok(name)
}

fn normalize_midi_clip_name(name: Option<String>) -> String {
    let name = name
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect::<String>();
    if name.is_empty() {
        "MIDI Clip".into()
    } else {
        name
    }
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
