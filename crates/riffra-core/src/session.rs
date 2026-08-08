//! Canonical CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! active workspace, design context, play state (including the live sample
//! instrument), the [`Arrangement`], the running rack, snapshots, and session
//! settings. It deliberately does not own audio/MIDI file bodies, the Library
//! index, recording files, or background-job state.

use crate::DomainError;
use crate::asset::AssetId;
use crate::rack::{DeviceKind, RackDevice, RackInstance, RackMacro};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Pulses per quarter note used by every session timeline.
pub const TIMELINE_PPQ: u32 = 960;

/// An exact position in musical time.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TimelineTick(pub u64);

/// A half-open range of source-audio frames.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FrameRange {
    pub start: u64,
    pub end: u64,
}

impl FrameRange {
    fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// A real-time duration expressed against its source sample rate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FrameDuration {
    pub frames: u64,
    pub sample_rate: u32,
}

/// Musical clock shared by the ruler, snapping, MIDI, and transport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTimebase {
    pub ppq: u32,
    pub bpm: f64,
    pub time_signature_numerator: u8,
    pub time_signature_denominator: u8,
}

impl Default for ProjectTimebase {
    fn default() -> Self {
        Self {
            ppq: TIMELINE_PPQ,
            bpm: 120.0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        }
    }
}

impl ProjectTimebase {
    /// Converts a real-time millisecond offset to the nearest timeline tick.
    pub fn milliseconds_to_ticks(self, milliseconds: f64) -> TimelineTick {
        let ticks = milliseconds.max(0.0) * self.bpm * f64::from(self.ppq) / 60_000.0;
        TimelineTick(ticks.round().max(0.0) as u64)
    }

    /// Converts source frames at `sample_rate` to the nearest Timeline tick.
    pub fn frames_to_ticks(self, frames: u64, sample_rate: u32) -> TimelineTick {
        self.milliseconds_to_ticks(frames as f64 * 1000.0 / f64::from(sample_rate))
    }

    fn ticks_to_frames(self, ticks: u64, sample_rate: u32) -> u64 {
        (ticks as f64 * f64::from(sample_rate) * 60.0 / (self.bpm * f64::from(self.ppq)))
            .round()
            .max(0.0) as u64
    }
}

/// Persisted loop selection. Disabled ranges retain their endpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimelineLoopRange {
    pub enabled: bool,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub end_tick: TimelineTick,
}

/// Optional non-destructive punch recording range on the project timeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePunchRange {
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub end_tick: TimelineTick,
}

/// The two fixed workspaces. `Sample`, `Analyze`, and `Separate` are not
/// workspaces; they are [`DesignTool`]s reached from [`Workspace::Design`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    Design,
    Arrange,
}

/// A design surface reached from the Design workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DesignTool {
    Sample,
    Analyze,
    Separate,
}

/// What the Design workspace is currently aimed at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesignContext {
    pub active_tool: DesignTool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_asset_id: Option<AssetId>,
}

impl Default for DesignContext {
    fn default() -> Self {
        Self {
            active_tool: DesignTool::Sample,
            target_asset_id: None,
        }
    }
}

/// The production source hosted by a timeline track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Instrument,
}

/// A physical input channel routed to one Audio Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputRoute {
    pub channel_index: u32,
}

/// A MIDI source and channel filter routed to one Instrument Track.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiInputRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<u8>,
}

/// A timeline track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub monitoring: MonitoringState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio_input: Option<AudioInputRoute>,
    #[serde(default)]
    pub midi_input: MidiInputRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instrument: Option<RackDevice>,
    pub rack: RackInstance,
}

/// Audio Track input monitoring state. `Auto` monitors only while the track is
/// armed; `On` always monitors; `Off` never monitors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringState {
    #[default]
    Off,
    Auto,
    On,
}

/// A Track mix parameter controlled on the Arrangement timeline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AutomationParameter {
    Volume,
    Pan,
}

/// A single value on an Automation Lane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPoint {
    pub id: String,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub value: f64,
}

/// Timeline control data for one Track parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationLane {
    pub id: String,
    pub track_id: String,
    pub parameter: AutomationParameter,
    pub points: Vec<AutomationPoint>,
}

fn empty_track_rack() -> RackInstance {
    RackInstance {
        devices: Vec::new(),
        macros: Vec::new(),
    }
}

impl Track {
    /// Creates a neutral audio track.
    pub fn audio(id: String, name: String) -> Self {
        Self {
            id,
            name,
            kind: TrackKind::Audio,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: MonitoringState::Off,
            audio_input: None,
            midi_input: MidiInputRoute::default(),
            instrument: None,
            rack: empty_track_rack(),
        }
    }

    /// Creates a neutral Instrument Track with no assigned instrument.
    pub fn instrument(id: String, name: String) -> Self {
        Self {
            kind: TrackKind::Instrument,
            ..Self::audio(id, name)
        }
    }
}

/// A single MIDI note inside a [`MidiClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub id: String,
    pub note: u8,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// A MIDI event which has no dedicated piano-roll editing representation.
///
/// Note events are represented by [`MidiNote`] so their duration remains
/// editable. The other event kinds are retained verbatim and are scheduled by
/// the native timeline runtime at their musical tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MidiEventKind {
    ControlChange,
    PitchBend,
    ChannelPressure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    pub id: String,
    pub kind: MidiEventKind,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub channel: u8,
    pub data1: u8,
    pub data2: u8,
}

/// A non-destructive MIDI clip on the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    pub name: String,
    pub track_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset_id: Option<AssetId>,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    #[serde(default)]
    pub events: Vec<MidiEvent>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording_take_id: Option<String>,
}

/// Whether a recorded audio take is using its original or processed variant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AudioTakeVariant {
    #[default]
    Raw,
    Processed,
}

/// A persisted recording attempt group. Its takes remain available even when
/// only one take is currently placed on the timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionRecord {
    pub id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[serde(default)]
    pub track_slots: Vec<RecordingSessionTrackSlot>,
    #[serde(default)]
    pub pass_ids: Vec<String>,
}

/// The active take and stable Timeline Clip owned by one recorded Track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionTrackSlot {
    pub track_id: String,
    pub active_take_id: String,
    pub timeline_clip_id: String,
}

/// One pass through the recording range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPassRecord {
    pub id: String,
    pub session_id: String,
    pub ordinal: u32,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    #[serde(default)]
    pub partial_start: bool,
    #[serde(default)]
    pub partial_end: bool,
    #[serde(default)]
    pub track_take_ids: Vec<String>,
}

/// A recorded Track product belonging to one recording pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TakeAudioSource {
    pub asset_id: AssetId,
    #[ts(type = "number")]
    pub source_start_sample: u64,
    #[ts(type = "number")]
    pub source_end_sample: u64,
    #[serde(default)]
    #[ts(type = "number")]
    pub tail_end_sample: u64,
    #[serde(default)]
    pub sample_rate: u32,
}

/// A recorded Track product belonging to one recording pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingTakeRecord {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub pass_id: String,
    pub track_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub duration_ticks: u64,
    #[serde(default)]
    pub source_start_sample: u64,
    #[serde(default)]
    pub source_end_sample: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub raw_audio: Option<TakeAudioSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub processed_audio: Option<TakeAudioSource>,
    /// Legacy v1 field. It is accepted during import and migrated to
    /// `raw_audio`, but new Sessions do not write it.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub raw_audio_asset_id: Option<AssetId>,
    /// Legacy v1 field. It is accepted during import and migrated to
    /// `processed_audio`, but new Sessions do not write it.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub processed_audio_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub midi_asset_id: Option<AssetId>,
}

impl RecordingTakeRecord {
    pub fn audio_source(&self, variant: AudioTakeVariant) -> Option<&TakeAudioSource> {
        match variant {
            AudioTakeVariant::Raw => self.raw_audio.as_ref(),
            AudioTakeVariant::Processed => self.processed_audio.as_ref(),
        }
    }

    pub fn preferred_audio_source(&self, variant: AudioTakeVariant) -> Option<&TakeAudioSource> {
        self.audio_source(variant).or(match variant {
            AudioTakeVariant::Raw => self.processed_audio.as_ref(),
            AudioTakeVariant::Processed => self.raw_audio.as_ref(),
        })
    }

    fn migrate_legacy_audio_sources(&mut self) {
        if self.raw_audio.is_none()
            && let Some(asset_id) = self.raw_audio_asset_id.take()
        {
            self.raw_audio = Some(TakeAudioSource {
                asset_id,
                source_start_sample: self.source_start_sample,
                source_end_sample: self.source_end_sample,
                tail_end_sample: self.source_end_sample,
                sample_rate: 0,
            });
        }
        if self.processed_audio.is_none()
            && let Some(asset_id) = self.processed_audio_asset_id.take()
        {
            self.processed_audio = Some(TakeAudioSource {
                asset_id,
                source_start_sample: self.source_start_sample,
                source_end_sample: self.source_end_sample,
                tail_end_sample: self.source_end_sample,
                sample_rate: 0,
            });
        }
    }
}

/// A non-destructive audio clip referencing an [`AssetId`].
///
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: String,
    pub track_id: String,
    pub asset_id: AssetId,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub source_range: FrameRange,
    pub source_sample_rate: u32,
    pub timeline_duration: FrameDuration,
    pub gain_db: f64,
    pub pan: f64,
    pub fade_in: FrameDuration,
    pub fade_out: FrameDuration,
    pub loop_enabled: bool,
    pub muted: bool,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording_take_id: Option<String>,
    #[serde(default)]
    pub take_variant: AudioTakeVariant,
}

impl AudioClip {
    /// Creates a clip that references an entire source at its native rate.
    pub fn full_source(
        id: String,
        name: String,
        track_id: String,
        asset_id: AssetId,
        start_tick: TimelineTick,
        sample_rate: u32,
        source_frames: u64,
    ) -> Self {
        let duration = FrameDuration {
            frames: source_frames,
            sample_rate,
        };
        Self {
            id,
            name,
            track_id,
            asset_id,
            start_tick,
            source_range: FrameRange {
                start: 0,
                end: source_frames,
            },
            source_sample_rate: sample_rate,
            timeline_duration: duration,
            gain_db: 0.0,
            pan: 0.0,
            fade_in: FrameDuration {
                frames: 0,
                sample_rate,
            },
            fade_out: FrameDuration {
                frames: 0,
                sample_rate,
            },
            loop_enabled: false,
            muted: false,
            recording_take_id: None,
            take_variant: AudioTakeVariant::Raw,
        }
    }

    /// Clamps and normalizes the production-managed numeric fields in place.
    ///
    /// This is the single canonical place where clip gain, pan, and fade
    /// limits live; callers supply raw values and rely on this method instead
    /// of replicating the rule.
    pub fn normalize_fields(&mut self) {
        if !self.gain_db.is_finite() {
            self.gain_db = 0.0;
        }
        self.gain_db = self.gain_db.clamp(-90.0, 24.0);
        if !self.pan.is_finite() {
            self.pan = 0.0;
        }
        self.pan = self.pan.clamp(-1.0, 1.0);
        self.fade_in.frames = self.fade_in.frames.min(self.timeline_duration.frames);
        self.fade_out.frames = self.fade_out.frames.min(self.timeline_duration.frames);
    }
}

/// A partial update for an existing [`AudioClip`].
///
/// Only the supplied fields are written; `None` fields keep the clip's current
/// value. Numeric normalization (gain, pan, fade clamping) is applied by the
/// domain, so callers may pass unclamped values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_tick: Option<TimelineTick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeline_duration: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_range: Option<FrameRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gain_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pan: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fade_in: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fade_out: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loop_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub muted: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipMove {
    pub clip_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub track_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipMove {
    pub clip_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub track_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_tick: Option<TimelineTick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub duration_ticks: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notes: Option<Vec<MidiNote>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub events: Option<Vec<MidiEvent>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loop_enabled: Option<bool>,
}

/// The Arrange workspace's production state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub revision: u64,
    pub timebase: ProjectTimebase,
    pub loop_range: TimelineLoopRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub punch_range: Option<TimelinePunchRange>,
    pub tracks: Vec<Track>,
    pub audio_clips: Vec<AudioClip>,
    pub midi_clips: Vec<MidiClip>,
    #[serde(default)]
    pub automation_lanes: Vec<AutomationLane>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub recording_sessions: Vec<RecordingSessionRecord>,
    #[serde(default)]
    pub recording_passes: Vec<RecordingPassRecord>,
    #[serde(default)]
    pub takes: Vec<RecordingTakeRecord>,
}

/// A named timeline marker. Markers hold no audio processing impact; they are
/// authoring metadata rendered on the Time Ruler.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: String,
    pub name: String,
    pub tick: u64,
}

impl Arrangement {
    /// Replaces the project-wide musical clock used by the ruler and runtime.
    /// Audio clip frame durations remain unchanged; only their musical display
    /// positions are recalculated by consumers of the timebase.
    pub fn update_timebase(&mut self, timebase: ProjectTimebase) -> Result<(), DomainError> {
        if timebase.ppq != TIMELINE_PPQ
            || !timebase.bpm.is_finite()
            || !(20.0..=400.0).contains(&timebase.bpm)
            || timebase.time_signature_numerator == 0
            || !matches!(timebase.time_signature_denominator, 1 | 2 | 4 | 8 | 16 | 32)
        {
            return Err(DomainError::InvalidClip(
                "Arrangement timebase is invalid.".into(),
            ));
        }
        self.timebase = timebase;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Removes a track and every timeline object hosted by it.
    ///
    /// Source Assets are not touched; only arrangement references are removed.
    ///
    /// # Errors
    /// Returns [`DomainError::UnknownTrack`] when `track_id` is not registered.
    pub fn remove_track(&mut self, track_id: &str) -> Result<(), DomainError> {
        let index = self
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .ok_or_else(|| DomainError::UnknownTrack(track_id.to_owned()))?;
        self.tracks.remove(index);
        self.audio_clips.retain(|clip| clip.track_id != track_id);
        self.midi_clips.retain(|clip| clip.track_id != track_id);
        self.automation_lanes
            .retain(|lane| lane.track_id != track_id);
        self.remove_recording_track_references(None, track_id);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Moves a track to a zero-based position in the arrangement.
    ///
    /// # Errors
    /// Returns [`DomainError::UnknownTrack`] when `track_id` is not registered.
    pub fn reorder_track(
        &mut self,
        track_id: &str,
        target_index: usize,
    ) -> Result<(), DomainError> {
        let index = self
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .ok_or_else(|| DomainError::UnknownTrack(track_id.to_owned()))?;
        let track = self.tracks.remove(index);
        self.tracks
            .insert(target_index.min(self.tracks.len()), track);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Replaces the transport loop selection and advances the arrangement revision.
    pub fn update_loop_range(
        &mut self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<(), DomainError> {
        if enabled && end_tick <= start_tick {
            return Err(DomainError::InvalidClip(
                "Enabled loop range must have a positive duration.".into(),
            ));
        }
        self.loop_range = TimelineLoopRange {
            enabled,
            start_tick,
            end_tick,
        };
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Sets or clears the non-destructive punch recording range.
    pub fn update_punch_range(
        &mut self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<(), DomainError> {
        if enabled && end_tick <= start_tick {
            return Err(DomainError::InvalidClip(
                "Punch range must have a positive duration.".into(),
            ));
        }
        self.punch_range = enabled.then_some(TimelinePunchRange {
            start_tick,
            end_tick,
        });
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Validates the structural rules for an audio clip against the tracks,
    /// without consulting any asset store.
    ///
    /// # Errors
    /// Returns [`DomainError::UnknownTrack`] when the clip's track does not
    /// exist, or [`DomainError::InvalidClip`] for a negative-equivalent or
    /// inverted source range.
    pub fn validate_audio_clip(&self, clip: &AudioClip) -> Result<(), DomainError> {
        let track = self
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id)
            .ok_or_else(|| DomainError::UnknownTrack(clip.track_id.clone()))?;
        if track.kind != TrackKind::Audio {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' requires an Audio Track.",
                clip.id
            )));
        }
        if clip.source_range.end <= clip.source_range.start {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' has an invalid source range.",
                clip.id
            )));
        }
        if clip.source_sample_rate == 0
            || clip.timeline_duration.frames == 0
            || clip.timeline_duration.sample_rate != clip.source_sample_rate
            || clip.fade_in.sample_rate != clip.source_sample_rate
            || clip.fade_out.sample_rate != clip.source_sample_rate
            || (!clip.loop_enabled && clip.timeline_duration.frames != clip.source_range.len())
            || (clip.loop_enabled && clip.timeline_duration.frames < clip.source_range.len())
        {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' has inconsistent frame timing.",
                clip.id
            )));
        }
        Ok(())
    }

    /// Adds an audio clip after enforcing the arrangement rules, including
    /// asset existence.
    ///
    /// `asset_exists` is consulted so the rule lives in the domain rather than
    /// at the command boundary; the caller supplies the asset-store lookup.
    ///
    /// # Errors
    /// Propagates [`Arrangement::validate_audio_clip`] failures and returns
    /// [`DomainError::InvalidClip`] when the referenced asset is missing.
    pub fn add_audio_clip(
        &mut self,
        clip: AudioClip,
        asset_exists: impl Fn(&AssetId) -> bool,
    ) -> Result<(), DomainError> {
        self.validate_audio_clip(&clip)?;
        if !asset_exists(&clip.asset_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' references an unknown asset {}.",
                clip.id, clip.asset_id
            )));
        }
        self.audio_clips.push(clip);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Validates a MIDI clip against the instrument-track rules.
    pub fn validate_midi_clip(&self, clip: &MidiClip) -> Result<(), DomainError> {
        let track = self
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id)
            .ok_or_else(|| DomainError::UnknownTrack(clip.track_id.clone()))?;
        if track.kind != TrackKind::Instrument {
            return Err(DomainError::InvalidClip(format!(
                "MIDI clip '{}' requires an Instrument Track.",
                clip.id
            )));
        }
        if clip.id.trim().is_empty()
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || clip.duration_ticks == 0
        {
            return Err(DomainError::InvalidClip(
                "MIDI clips require non-empty identity and a positive duration.".into(),
            ));
        }
        if clip.notes.len() > 200_000 || clip.events.len() > 200_000 {
            return Err(DomainError::InvalidClip(format!(
                "MIDI clip '{}' contains too many events.",
                clip.name
            )));
        }
        for note in &clip.notes {
            if note.id.trim().is_empty()
                || note.note > 127
                || note.velocity > 127
                || note.channel == 0
                || note.channel > 16
                || note.duration_ticks == 0
                || note.start_tick.0 >= clip.duration_ticks
            {
                return Err(DomainError::InvalidClip(format!(
                    "MIDI clip '{}' contains an invalid note.",
                    clip.name
                )));
            }
        }
        for event in &clip.events {
            if event.id.trim().is_empty()
                || event.tick.0 >= clip.duration_ticks
                || event.channel == 0
                || event.channel > 16
            {
                return Err(DomainError::InvalidClip(format!(
                    "MIDI clip '{}' contains an invalid event.",
                    clip.name
                )));
            }
        }
        Ok(())
    }

    /// Adds a MIDI clip without converting its musical ticks to audio frames.
    pub fn add_midi_clip(&mut self, clip: MidiClip) -> Result<(), DomainError> {
        self.validate_midi_clip(&clip)?;
        self.midi_clips.push(clip);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn update_midi_clip(
        &mut self,
        clip_id: &str,
        patch: MidiClipPatch,
    ) -> Result<(), DomainError> {
        let index = self
            .midi_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        let mut clip = self.midi_clips[index].clone();
        if let Some(name) = patch.name {
            clip.name = name;
        }
        if let Some(track_id) = patch.track_id {
            if track_id != clip.track_id && self.is_recording_slot_clip(&clip.id) {
                return Err(DomainError::InvalidClip(
                    "A recording Session slot clip cannot be moved to another Track; place a copy instead."
                        .into(),
                ));
            }
            clip.track_id = track_id;
        }
        if let Some(start_tick) = patch.start_tick {
            clip.start_tick = start_tick;
        }
        if let Some(duration_ticks) = patch.duration_ticks {
            clip.duration_ticks = duration_ticks.max(1);
        }
        if let Some(notes) = patch.notes {
            clip.notes = notes;
        }
        if let Some(events) = patch.events {
            clip.events = events;
        }
        if let Some(muted) = patch.muted {
            clip.muted = muted;
        }
        if let Some(loop_enabled) = patch.loop_enabled {
            clip.loop_enabled = loop_enabled;
        }
        self.validate_midi_clip(&clip)?;
        self.midi_clips[index] = clip;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn move_midi_clips(&mut self, moves: Vec<MidiClipMove>) -> Result<(), DomainError> {
        if moves.is_empty() {
            return Err(DomainError::InvalidClip("No MIDI clips were moved.".into()));
        }
        for movement in &moves {
            let clip = self
                .midi_clips
                .iter()
                .find(|clip| clip.id == movement.clip_id)
                .ok_or_else(|| {
                    DomainError::InvalidClip(format!("MIDI clip '{}' not found.", movement.clip_id))
                })?;
            let track = self
                .tracks
                .iter()
                .find(|track| track.id == movement.track_id)
                .ok_or_else(|| DomainError::UnknownTrack(movement.track_id.clone()))?;
            if track.kind != TrackKind::Instrument {
                return Err(DomainError::InvalidClip(format!(
                    "MIDI clip '{}' requires an Instrument Track.",
                    clip.id
                )));
            }
            if movement.track_id != clip.track_id && self.is_recording_slot_clip(&clip.id) {
                return Err(DomainError::InvalidClip(
                    "A recording Session slot clip cannot be moved to another Track; place a copy instead."
                        .into(),
                ));
            }
        }
        for movement in moves {
            if let Some(clip) = self
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == movement.clip_id)
            {
                clip.start_tick = movement.start_tick;
                clip.track_id = movement.track_id;
            }
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn trim_midi_clip(
        &mut self,
        clip_id: &str,
        start_tick: TimelineTick,
        duration_ticks: u64,
    ) -> Result<(), DomainError> {
        let clip = self
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        let duration_ticks = duration_ticks.max(1);
        let relative_start = start_tick.0.saturating_sub(clip.start_tick.0);
        let relative_end = relative_start.saturating_add(duration_ticks);
        clip.start_tick = start_tick;
        clip.duration_ticks = duration_ticks;
        clip.notes.retain(|note| {
            note.start_tick.0 < relative_end
                && note.start_tick.0.saturating_add(note.duration_ticks) > relative_start
        });
        clip.events
            .retain(|event| event.tick.0 >= relative_start && event.tick.0 < relative_end);
        for note in &mut clip.notes {
            note.start_tick = TimelineTick(note.start_tick.0.saturating_sub(relative_start));
            note.duration_ticks = note
                .duration_ticks
                .min(duration_ticks.saturating_sub(note.start_tick.0))
                .max(1);
        }
        for event in &mut clip.events {
            event.tick = TimelineTick(event.tick.0.saturating_sub(relative_start));
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn split_midi_clip(
        &mut self,
        clip_id: &str,
        split_tick: TimelineTick,
        right_id: String,
    ) -> Result<(), DomainError> {
        if self.midi_clips.iter().any(|clip| clip.id == right_id) {
            return Err(DomainError::InvalidClip(format!(
                "MIDI clip id already exists: {right_id}"
            )));
        }
        let index = self
            .midi_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        let source = self.midi_clips[index].clone();
        let relative = split_tick.0.saturating_sub(source.start_tick.0);
        if relative == 0 || relative >= source.duration_ticks {
            return Err(DomainError::InvalidClip(
                "MIDI split must be inside the clip.".into(),
            ));
        }
        let mut left = source.clone();
        left.duration_ticks = relative;
        left.notes.retain(|note| note.start_tick.0 < relative);
        left.events.retain(|event| event.tick.0 < relative);
        for note in &mut left.notes {
            note.duration_ticks = note.duration_ticks.min(relative - note.start_tick.0).max(1);
        }
        let mut right = source;
        right.id = right_id;
        right.start_tick = split_tick;
        right.duration_ticks -= relative;
        right.notes.retain(|note| note.start_tick.0 >= relative);
        for note in &mut right.notes {
            note.start_tick = TimelineTick(note.start_tick.0 - relative);
            note.duration_ticks = note
                .duration_ticks
                .min(right.duration_ticks - note.start_tick.0)
                .max(1);
        }
        right.events.retain(|event| event.tick.0 >= relative);
        for event in &mut right.events {
            event.tick = TimelineTick(event.tick.0 - relative);
        }
        self.midi_clips[index] = left;
        self.midi_clips.insert(index + 1, right);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn duplicate_midi_clip(&mut self, clip_id: &str, id: String) -> Result<(), DomainError> {
        if self.audio_clips.iter().any(|clip| clip.id == id)
            || self.midi_clips.iter().any(|clip| clip.id == id)
        {
            return Err(DomainError::InvalidClip(format!(
                "Timeline clip id already exists: {id}"
            )));
        }
        let mut copy = self
            .midi_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        copy.id = id;
        copy.name = format!("{} copy", copy.name);
        copy.start_tick = TimelineTick(copy.start_tick.0.saturating_add(copy.duration_ticks));
        self.midi_clips.push(copy);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn quantize_midi_notes(
        &mut self,
        clip_id: &str,
        note_ids: &[String],
        grid_ticks: u64,
    ) -> Result<(), DomainError> {
        if grid_ticks == 0 {
            return Err(DomainError::InvalidClip(
                "MIDI quantize grid must be positive.".into(),
            ));
        }
        let clip = self
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        for note in &mut clip.notes {
            if note_ids.iter().any(|id| id == &note.id) {
                note.start_tick =
                    TimelineTick(((note.start_tick.0 + grid_ticks / 2) / grid_ticks) * grid_ticks);
                note.start_tick =
                    TimelineTick(note.start_tick.0.min(clip.duration_ticks.saturating_sub(1)));
                note.duration_ticks = note
                    .duration_ticks
                    .min(clip.duration_ticks.saturating_sub(note.start_tick.0).max(1));
            }
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn duplicate_midi_notes(
        &mut self,
        clip_id: &str,
        note_ids: &[String],
        offset_ticks: u64,
    ) -> Result<(), DomainError> {
        let clip = self
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        let selected = clip
            .notes
            .iter()
            .filter(|note| note_ids.iter().any(|id| id == &note.id))
            .cloned()
            .collect::<Vec<_>>();
        for (index, mut note) in selected.into_iter().enumerate() {
            note.id = format!("note:duplicate:{}:{index}", self.revision);
            note.start_tick = TimelineTick(note.start_tick.0.saturating_add(offset_ticks));
            if note.start_tick.0 >= clip.duration_ticks {
                continue;
            }
            note.duration_ticks = note
                .duration_ticks
                .min(clip.duration_ticks - note.start_tick.0)
                .max(1);
            clip.notes.push(note);
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Applies a partial update to an existing audio clip and normalizes the
    /// resulting values through the canonical domain rules (track existence,
    /// source range, gain/pan/fade clamps).
    ///
    /// The clip is identified by `clip_id`; missing fields on `patch` keep the
    /// clip's current value. Asset references are not changed here, so no
    /// asset-store lookup is needed.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidClip`] when the clip cannot be found, the
    /// target track is missing, the source range becomes inverted, or the
    /// clip's required identity fields end up empty.
    pub fn update_audio_clip(
        &mut self,
        clip_id: &str,
        patch: AudioClipPatch,
    ) -> Result<(), DomainError> {
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        // Take the clip out of the slice so we can mutate it while still
        // consulting `self.tracks` for the track existence rule.
        let mut clip = self.audio_clips[index].clone();
        if let Some(name) = patch.name {
            clip.name = name;
        }
        if let Some(track_id) = patch.track_id {
            if track_id != clip.track_id && self.is_recording_slot_clip(&clip.id) {
                return Err(DomainError::InvalidClip(
                    "A recording Session slot clip cannot be moved to another Track; place a copy instead."
                        .into(),
                ));
            }
            clip.track_id = track_id;
        }
        if let Some(start_tick) = patch.start_tick {
            clip.start_tick = start_tick;
        }
        if let Some(timeline_duration) = patch.timeline_duration {
            clip.timeline_duration = timeline_duration;
        }
        if let Some(source_range) = patch.source_range {
            clip.source_range = source_range;
        }
        if let Some(gain_db) = patch.gain_db {
            clip.gain_db = gain_db;
        }
        if let Some(pan) = patch.pan {
            clip.pan = pan;
        }
        if let Some(fade_in) = patch.fade_in {
            clip.fade_in = fade_in;
        }
        if let Some(fade_out) = patch.fade_out {
            clip.fade_out = fade_out;
        }
        if let Some(loop_enabled) = patch.loop_enabled {
            clip.loop_enabled = loop_enabled;
        }
        if let Some(muted) = patch.muted {
            clip.muted = muted;
        }
        if clip.id.trim().is_empty()
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || clip.asset_id.as_str().trim().is_empty()
        {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' requires non-empty id, name, track and asset id.",
                clip.id
            )));
        }
        if clip.timeline_duration.frames == 0 {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' must have a positive duration.",
                clip.id
            )));
        }
        clip.normalize_fields();
        self.validate_audio_clip(&clip)?;
        self.audio_clips[index] = clip;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Removes Audio and MIDI clips as one typed timeline edit.
    pub fn remove_timeline_clips(
        &mut self,
        audio_clip_ids: &[String],
        midi_clip_ids: &[String],
    ) -> Result<(), DomainError> {
        if audio_clip_ids.is_empty() && midi_clip_ids.is_empty() {
            return Err(DomainError::InvalidClip(
                "No timeline clips were selected.".into(),
            ));
        }
        if audio_clip_ids
            .iter()
            .any(|id| !self.audio_clips.iter().any(|clip| clip.id == *id))
            || midi_clip_ids
                .iter()
                .any(|id| !self.midi_clips.iter().any(|clip| clip.id == *id))
        {
            return Err(DomainError::InvalidClip(
                "One or more selected timeline clips were not found.".into(),
            ));
        }
        self.audio_clips
            .retain(|clip| !audio_clip_ids.iter().any(|id| id == &clip.id));
        self.midi_clips
            .retain(|clip| !midi_clip_ids.iter().any(|id| id == &clip.id));
        let removed_slot_tracks = self
            .recording_sessions
            .iter()
            .flat_map(|session| {
                session.track_slots.iter().filter_map(|slot| {
                    (audio_clip_ids.iter().any(|id| id == &slot.timeline_clip_id)
                        || midi_clip_ids.iter().any(|id| id == &slot.timeline_clip_id))
                    .then_some((session.id.clone(), slot.track_id.clone()))
                })
            })
            .collect::<Vec<_>>();
        for (session_id, track_id) in removed_slot_tracks {
            self.remove_recording_track_references(Some(&session_id), &track_id);
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Pastes typed Audio and MIDI clip selections at one musical anchor.
    pub fn paste_timeline_clips(
        &mut self,
        audio_clip_ids: &[String],
        midi_clip_ids: &[String],
        audio_ids: &[String],
        midi_ids: &[String],
        start_tick: TimelineTick,
    ) -> Result<(), DomainError> {
        if audio_clip_ids.len() != audio_ids.len()
            || midi_clip_ids.len() != midi_ids.len()
            || (audio_clip_ids.is_empty() && midi_clip_ids.is_empty())
        {
            return Err(DomainError::InvalidClip(
                "Clipboard selection is invalid.".into(),
            ));
        }
        let audio_sources = audio_clip_ids
            .iter()
            .map(|id| {
                self.audio_clips
                    .iter()
                    .find(|clip| clip.id == *id)
                    .cloned()
                    .ok_or_else(|| {
                        DomainError::InvalidClip(format!("Audio clip '{id}' not found."))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let midi_sources = midi_clip_ids
            .iter()
            .map(|id| {
                self.midi_clips
                    .iter()
                    .find(|clip| clip.id == *id)
                    .cloned()
                    .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{id}' not found.")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let anchor = audio_sources
            .iter()
            .map(|clip| clip.start_tick.0)
            .chain(midi_sources.iter().map(|clip| clip.start_tick.0))
            .min()
            .unwrap_or(start_tick.0);
        let mut copies = Vec::with_capacity(audio_sources.len());
        for (mut copy, id) in audio_sources.into_iter().zip(audio_ids) {
            if self.audio_clips.iter().any(|clip| clip.id == *id)
                || self.midi_clips.iter().any(|clip| clip.id == *id)
                || copies.iter().any(|clip: &AudioClip| clip.id == *id)
            {
                return Err(DomainError::InvalidClip(format!(
                    "Timeline clip id already exists: {id}"
                )));
            }
            copy.id = id.clone();
            copy.name = format!("{} copy", copy.name);
            copy.start_tick = TimelineTick(
                start_tick
                    .0
                    .saturating_add(copy.start_tick.0.saturating_sub(anchor)),
            );
            copies.push(copy);
        }
        let mut midi_copies = Vec::with_capacity(midi_sources.len());
        for (mut copy, id) in midi_sources.into_iter().zip(midi_ids) {
            if self.audio_clips.iter().any(|clip| clip.id == *id)
                || self.midi_clips.iter().any(|clip| clip.id == *id)
                || copies.iter().any(|clip| clip.id == *id)
                || midi_copies.iter().any(|clip: &MidiClip| clip.id == *id)
            {
                return Err(DomainError::InvalidClip(format!(
                    "Timeline clip id already exists: {id}"
                )));
            }
            copy.id = id.clone();
            copy.name = format!("{} copy", copy.name);
            copy.start_tick = TimelineTick(
                start_tick
                    .0
                    .saturating_add(copy.start_tick.0.saturating_sub(anchor)),
            );
            midi_copies.push(copy);
        }
        self.audio_clips.extend(copies);
        self.midi_clips.extend(midi_copies);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn trim_audio_clip(
        &mut self,
        clip_id: &str,
        start_tick: TimelineTick,
        source_range: FrameRange,
        source_frames: u64,
    ) -> Result<(), DomainError> {
        if source_range.end > source_frames || source_range.end <= source_range.start {
            return Err(DomainError::InvalidClip(
                "Trim range must stay inside the source Asset.".into(),
            ));
        }
        let clip = self
            .audio_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        if clip.loop_enabled {
            return Err(DomainError::InvalidClip(
                "Disable Clip Loop before trimming the source range.".into(),
            ));
        }
        clip.start_tick = start_tick;
        clip.source_range = source_range;
        clip.timeline_duration.frames = source_range.len();
        clip.fade_in.frames = clip.fade_in.frames.min(clip.timeline_duration.frames);
        clip.fade_out.frames = clip.fade_out.frames.min(clip.timeline_duration.frames);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn split_audio_clip(
        &mut self,
        clip_id: &str,
        split_tick: TimelineTick,
        right_id: String,
    ) -> Result<(), DomainError> {
        if self.audio_clips.iter().any(|clip| clip.id == right_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip id already exists: {right_id}"
            )));
        }
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        let mut left = self.audio_clips[index].clone();
        if left.loop_enabled {
            return Err(DomainError::InvalidClip(
                "Disable Clip Loop before splitting the clip.".into(),
            ));
        }
        let tick_offset = split_tick
            .0
            .checked_sub(left.start_tick.0)
            .ok_or_else(|| DomainError::InvalidClip("Split must be inside the clip.".into()))?;
        let frame_offset = self
            .timebase
            .ticks_to_frames(tick_offset, left.source_sample_rate);
        if frame_offset == 0 || frame_offset >= left.timeline_duration.frames {
            return Err(DomainError::InvalidClip(
                "Split must leave audio on both sides.".into(),
            ));
        }
        let mut right = left.clone();
        left.source_range.end = left.source_range.start + frame_offset;
        left.timeline_duration.frames = frame_offset;
        left.fade_out.frames = 0;
        left.fade_in.frames = left.fade_in.frames.min(frame_offset);
        right.id = right_id;
        right.name = format!("{} split", right.name);
        right.start_tick = split_tick;
        right.source_range.start += frame_offset;
        right.timeline_duration.frames -= frame_offset;
        right.fade_in.frames = 0;
        right.fade_out.frames = right.fade_out.frames.min(right.timeline_duration.frames);
        self.audio_clips[index] = left;
        self.audio_clips.insert(index + 1, right);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn duplicate_audio_clip(
        &mut self,
        clip_id: &str,
        duplicate_id: String,
    ) -> Result<(), DomainError> {
        if self.audio_clips.iter().any(|clip| clip.id == duplicate_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip id already exists: {duplicate_id}"
            )));
        }
        let mut duplicate = self
            .audio_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        duplicate.id = duplicate_id;
        duplicate.name = format!("{} copy", duplicate.name);
        duplicate.start_tick = TimelineTick(
            duplicate.start_tick.0.saturating_add(
                self.timebase
                    .frames_to_ticks(
                        duplicate.timeline_duration.frames,
                        duplicate.timeline_duration.sample_rate,
                    )
                    .0,
            ),
        );
        self.audio_clips.push(duplicate);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Moves a selection as one edit so relative timing and revision stay coherent.
    pub fn move_audio_clips(&mut self, moves: Vec<AudioClipMove>) -> Result<(), DomainError> {
        if moves.is_empty() {
            return Err(DomainError::InvalidClip(
                "No clips were selected to move.".into(),
            ));
        }
        let mut next = self.audio_clips.clone();
        for movement in moves {
            let clip = next
                .iter_mut()
                .find(|clip| clip.id == movement.clip_id)
                .ok_or_else(|| {
                    DomainError::InvalidClip(format!(
                        "Audio clip '{}' not found.",
                        movement.clip_id
                    ))
                })?;
            if movement.track_id != clip.track_id && self.is_recording_slot_clip(&clip.id) {
                return Err(DomainError::InvalidClip(
                    "A recording Session slot clip cannot be moved to another Track; place a copy instead."
                        .into(),
                ));
            }
            clip.start_tick = movement.start_tick;
            clip.track_id = movement.track_id;
        }
        for clip in &next {
            self.validate_audio_clip(clip)?;
        }
        self.audio_clips = next;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    fn is_recording_slot_clip(&self, clip_id: &str) -> bool {
        self.recording_sessions.iter().any(|session| {
            session
                .track_slots
                .iter()
                .any(|slot| slot.timeline_clip_id == clip_id)
        })
    }

    /// Removes recording metadata owned by a track. The source assets remain
    /// registered: this only removes canonical arrangement references.
    fn remove_recording_track_references(&mut self, session_id: Option<&str>, track_id: &str) {
        let stable_slot_clip_ids = self
            .recording_sessions
            .iter()
            .filter(|recording| session_id.is_none_or(|id| recording.id == id))
            .flat_map(|recording| {
                recording
                    .track_slots
                    .iter()
                    .filter(|slot| slot.track_id == track_id)
                    .map(|slot| slot.timeline_clip_id.clone())
            })
            .collect::<std::collections::HashSet<_>>();
        let removed_take_ids = self
            .takes
            .iter()
            .filter(|take| {
                take.track_id == track_id && session_id.is_none_or(|id| take.session_id == id)
            })
            .map(|take| take.id.clone())
            .collect::<std::collections::HashSet<_>>();
        if !removed_take_ids.is_empty() {
            self.takes
                .retain(|take| !removed_take_ids.contains(&take.id));
            for clip in &mut self.audio_clips {
                if clip
                    .recording_take_id
                    .as_ref()
                    .is_some_and(|id| removed_take_ids.contains(id))
                    && !stable_slot_clip_ids.contains(&clip.id)
                {
                    clip.recording_take_id = None;
                }
            }
            for clip in &mut self.midi_clips {
                if clip
                    .recording_take_id
                    .as_ref()
                    .is_some_and(|id| removed_take_ids.contains(id))
                    && !stable_slot_clip_ids.contains(&clip.id)
                {
                    clip.recording_take_id = None;
                }
            }
            self.recording_passes.iter_mut().for_each(|pass| {
                pass.track_take_ids
                    .retain(|id| !removed_take_ids.contains(id));
            });
        }
        let removed_pass_ids = self
            .recording_passes
            .iter()
            .filter(|pass| !removed_take_ids.is_empty() && pass.track_take_ids.is_empty())
            .map(|pass| pass.id.clone())
            .collect::<std::collections::HashSet<_>>();
        self.recording_passes
            .retain(|pass| !removed_pass_ids.contains(&pass.id));
        self.recording_sessions.iter_mut().for_each(|recording| {
            if session_id.is_none_or(|id| recording.id == id) {
                recording
                    .track_slots
                    .retain(|slot| slot.track_id != track_id);
            }
            recording
                .pass_ids
                .retain(|id| !removed_pass_ids.contains(id));
        });
        self.recording_sessions.retain(|recording| {
            !(recording.track_slots.is_empty() && recording.pass_ids.is_empty())
        });
    }

    /// Applies an explicit equal-power crossfade to two overlapping clips.
    pub fn crossfade_audio_clips(
        &mut self,
        first_id: &str,
        second_id: &str,
    ) -> Result<(), DomainError> {
        if first_id == second_id {
            return Err(DomainError::InvalidClip(
                "Crossfade requires two different clips.".into(),
            ));
        }
        let first_index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == first_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{first_id}' not found."))
            })?;
        let second_index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == second_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{second_id}' not found."))
            })?;
        let first = &self.audio_clips[first_index];
        let second = &self.audio_clips[second_index];
        if first.track_id != second.track_id {
            return Err(DomainError::InvalidClip(
                "Crossfade clips must be on the same track.".into(),
            ));
        }
        let first_end = first.start_tick.0.saturating_add(
            self.timebase
                .frames_to_ticks(first.timeline_duration.frames, first.source_sample_rate)
                .0,
        );
        let second_end = second.start_tick.0.saturating_add(
            self.timebase
                .frames_to_ticks(second.timeline_duration.frames, second.source_sample_rate)
                .0,
        );
        let overlap_start = first.start_tick.0.max(second.start_tick.0);
        let overlap_end = first_end.min(second_end);
        if overlap_end <= overlap_start {
            return Err(DomainError::InvalidClip(
                "Crossfade clips must overlap in time.".into(),
            ));
        }
        let (left_index, right_index) = if first.start_tick <= second.start_tick {
            (first_index, second_index)
        } else {
            (second_index, first_index)
        };
        let overlap_ticks = overlap_end - overlap_start;
        let left_rate = self.audio_clips[left_index].source_sample_rate;
        let right_rate = self.audio_clips[right_index].source_sample_rate;
        self.audio_clips[left_index].fade_out = FrameDuration {
            frames: self.timebase.ticks_to_frames(overlap_ticks, left_rate),
            sample_rate: left_rate,
        };
        self.audio_clips[right_index].fade_in = FrameDuration {
            frames: self.timebase.ticks_to_frames(overlap_ticks, right_rate),
            sample_rate: right_rate,
        };
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

/// A MIDI-triggered pad mapping a key to a slice of a sample [`Asset`]. This is
/// live *playback* state for sample performance, distinct from a saved
/// [`crate::asset::AssetKind::Sample`] asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SamplePad {
    pub id: String,
    pub name: String,
    pub asset_id: AssetId,
    pub start_ms: u64,
    pub end_ms: u64,
    pub midi_key: u8,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
}

/// The set of sample pads currently loaded for performance.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SampleInstrumentState {
    #[serde(default)]
    pub pads: Vec<SamplePad>,
}

/// Live sample performance state (instrument and performance configuration).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayState {
    #[serde(default)]
    pub sample_instrument: SampleInstrumentState,
}

/// A captured A/B rack + master snapshot for quick comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub description: String,
    pub tag: Option<String>,
    pub parent_id: Option<String>,
    pub master_db: f64,
    pub rack: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

/// Permitted scope of AI-proposed changes applied to a session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
pub enum AiPermission {
    Explain,
    #[default]
    Suggest,
    Apply,
}

/// A reversible AI-proposed change record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AiChangeSet {
    pub id: String,
    pub created_at_ms: u64,
    pub permission: AiPermission,
    pub target: String,
    pub current_gain_db: f64,
    pub proposed_gain_db: f64,
    pub reason: String,
    pub expected_effect: String,
    pub risk: String,
    #[serde(default)]
    pub context: Vec<String>,
    pub applied: bool,
}

/// Session-wide settings that are not clip/track/rack structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettings {
    pub master_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub count_in_beats: u8,
    #[serde(default)]
    pub metronome_enabled: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub ai_permission: AiPermission,
    #[serde(default = "default_ai_context")]
    pub ai_context: Vec<String>,
    #[serde(default)]
    pub ai_history: Vec<AiChangeSet>,
}

fn default_ai_context() -> Vec<String> {
    vec!["analysis".into(), "selectedClip".into()]
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreativeSession {
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    pub workspace: Workspace,
    #[serde(default)]
    pub design_context: DesignContext,
    #[serde(default)]
    pub play_state: PlayState,
    #[serde(default)]
    pub arrangement: Arrangement,
    pub rack: RackInstance,
    #[serde(default)]
    pub snapshots: Vec<SessionSnapshot>,
    pub settings: SessionSettings,
}

/// Deserializes a session and upgrades the former recording-session shape at
/// the persistence boundary. The returned value contains only the canonical
/// Session / Pass / Track Slot / Take representation and therefore serializes
/// without legacy fields.
///
/// # Errors
/// Returns a JSON error when the payload cannot be decoded as a valid session.
pub fn deserialize_session(payload: &[u8]) -> Result<CreativeSession, serde_json::Error> {
    let mut value = serde_json::from_slice::<serde_json::Value>(payload)?;
    if value.get("workspace").and_then(serde_json::Value::as_str) == Some("play")
        && let Some(object) = value.as_object_mut()
    {
        object.insert("workspace".into(), serde_json::json!("arrange"));
    }
    let mut session = serde_json::from_value::<CreativeSession>(value.clone())?;
    let Some(arrangement) = value.get("arrangement") else {
        return Ok(session);
    };
    let legacy_takes = arrangement
        .get("takes")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !session
        .arrangement
        .takes
        .iter()
        .any(|take| take.pass_id.is_empty())
    {
        for take in &mut session.arrangement.takes {
            take.migrate_legacy_audio_sources();
        }
        return Ok(session);
    }

    let mut pass_keys = Vec::<(String, u64, u64)>::new();
    for take in &session.arrangement.takes {
        let key = (
            take.session_id.clone(),
            take.start_tick.0,
            take.duration_ticks,
        );
        if !pass_keys.contains(&key) {
            pass_keys.push(key);
        }
    }
    for (index, (session_id, start_tick, duration_ticks)) in pass_keys.iter().enumerate() {
        let pass_id = format!("pass:migrated:{session_id}:{}", index + 1);
        let ordinal = u32::try_from(
            pass_keys[..=index]
                .iter()
                .filter(|(candidate, _, _)| candidate == session_id)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let track_take_ids = session
            .arrangement
            .takes
            .iter()
            .filter(|take| {
                take.session_id == *session_id
                    && take.start_tick.0 == *start_tick
                    && take.duration_ticks == *duration_ticks
            })
            .map(|take| take.id.clone())
            .collect();
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: pass_id.clone(),
                session_id: session_id.clone(),
                ordinal,
                start_tick: TimelineTick(*start_tick),
                duration_ticks: *duration_ticks,
                partial_start: false,
                partial_end: false,
                track_take_ids,
            });
        for take in &mut session.arrangement.takes {
            if take.session_id == *session_id
                && take.start_tick.0 == *start_tick
                && take.duration_ticks == *duration_ticks
            {
                take.pass_id = pass_id.clone();
            }
        }
    }

    for (index, take) in session.arrangement.takes.iter_mut().enumerate() {
        let legacy = legacy_takes.get(index);
        let clip_id = legacy
            .and_then(|value| value.get("clipId"))
            .and_then(serde_json::Value::as_str);
        let variant = match legacy
            .and_then(|value| value.get("activeVariant"))
            .and_then(serde_json::Value::as_str)
        {
            Some("processed") => AudioTakeVariant::Processed,
            _ => AudioTakeVariant::Raw,
        };
        if let Some(clip_id) = clip_id {
            if let Some(clip) = session
                .arrangement
                .audio_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
            {
                clip.recording_take_id = Some(take.id.clone());
                clip.take_variant = variant;
                take.source_start_sample = clip.source_range.start;
                take.source_end_sample = clip.source_range.end;
            }
            if let Some(clip) = session
                .arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
            {
                clip.recording_take_id = Some(take.id.clone());
            }
        }
    }

    for recording in &mut session.arrangement.recording_sessions {
        recording.pass_ids = session
            .arrangement
            .recording_passes
            .iter()
            .filter(|pass| pass.session_id == recording.id)
            .map(|pass| pass.id.clone())
            .collect();
        let mut track_ids = session
            .arrangement
            .takes
            .iter()
            .filter(|take| take.session_id == recording.id)
            .map(|take| take.track_id.clone())
            .collect::<Vec<_>>();
        track_ids.sort();
        track_ids.dedup();
        for track_id in track_ids {
            let matching = session
                .arrangement
                .takes
                .iter()
                .enumerate()
                .filter(|(_, take)| take.session_id == recording.id && take.track_id == track_id)
                .collect::<Vec<_>>();
            let selected = matching
                .iter()
                .find(|(index, _)| {
                    legacy_takes
                        .get(*index)
                        .and_then(|value| value.get("active"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                })
                .copied()
                .or_else(|| matching.last().copied());
            let Some((index, take)) = selected else {
                continue;
            };
            let Some(timeline_clip_id) = legacy_takes
                .get(index)
                .and_then(|value| value.get("clipId"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            recording.track_slots.push(RecordingSessionTrackSlot {
                track_id,
                active_take_id: take.id.clone(),
                timeline_clip_id: timeline_clip_id.to_owned(),
            });
        }
    }
    for take in &mut session.arrangement.takes {
        take.migrate_legacy_audio_sources();
    }
    Ok(session)
}

fn default_rack() -> RackInstance {
    RackInstance {
        devices: vec![
            RackDevice {
                id: "input".into(),
                name: "Input 1".into(),
                kind: DeviceKind::Input,
                path: None,
                bypassed: false,
                gain_db: 0.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
            RackDevice {
                id: "safety".into(),
                name: "Safety Limiter".into(),
                kind: DeviceKind::Utility,
                path: None,
                bypassed: false,
                gain_db: 0.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
            RackDevice {
                id: "output".into(),
                name: "Main Out".into(),
                kind: DeviceKind::Output,
                path: None,
                bypassed: false,
                gain_db: -18.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
        ],
        macros: default_macros(),
    }
}

fn default_macros() -> Vec<RackMacro> {
    ["Brightness", "Gain", "Space", "Width"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| RackMacro {
            id: format!("macro:{index}"),
            name: name.into(),
            value: 0.5,
            parameter_index: None,
        })
        .collect()
}

impl CreativeSession {
    /// Creates a fresh session in the Arrange workspace with the default rack,
    /// arrangement, and safe (muted) settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            workspace: Workspace::Arrange,
            design_context: DesignContext::default(),
            play_state: PlayState::default(),
            arrangement: Arrangement::default(),
            rack: default_rack(),
            snapshots: Vec::new(),
            settings: SessionSettings {
                master_db: -18.0,
                loop_enabled: false,
                count_in_beats: 0,
                metronome_enabled: false,
                note: String::new(),
                ai_permission: AiPermission::default(),
                ai_context: default_ai_context(),
                ai_history: Vec::new(),
            },
        }
    }

    /// Validates production rules and normalizes clamped values, mirroring the
    /// guarantees the canonical session model enforces on load/save.
    ///
    /// # Errors
    /// Returns a description of the first violated rule.
    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        if self.session_id.trim().is_empty() {
            return Err("Session id must not be empty.".into());
        }
        let settings = &mut self.settings;
        if !settings.master_db.is_finite() {
            return Err("Master gain must be finite.".into());
        }
        settings.master_db = settings.master_db.clamp(-90.0, 0.0);
        if settings.count_in_beats > 8 {
            return Err("Count-in must be between 0 and 8 beats.".into());
        }
        settings.note.truncate(16_384);
        settings.ai_context.truncate(16);
        settings.ai_context.retain(|item| {
            !item.trim().is_empty() && item.len() <= 64 && AI_CONTEXT_IDS.contains(&item.as_str())
        });
        settings.ai_context.dedup();
        if settings.ai_history.len() > 128 {
            return Err("AI history cannot contain more than 128 ChangeSets.".into());
        }
        for change_set in &mut settings.ai_history {
            normalize_ai_change_set(change_set)?;
        }

        normalize_rack(&mut self.rack)?;
        normalize_snapshots(&mut self.snapshots)?;
        normalize_arrangement(&mut self.arrangement)?;
        normalize_sample_pads(&mut self.play_state.sample_instrument.pads)?;
        Ok(self)
    }
}

const AI_CONTEXT_IDS: &[&str] = &[
    "selectedRack",
    "parameterList",
    "analysis",
    "selectedClip",
    "project",
    "userNote",
    "snapshot",
    "previewAudio",
    "errorLog",
];

fn normalize_ai_change_set(change_set: &mut AiChangeSet) -> Result<(), String> {
    if change_set.id.trim().is_empty() || change_set.target.trim().is_empty() {
        return Err("AI ChangeSets require non-empty ids and targets.".into());
    }
    if !change_set.current_gain_db.is_finite() || !change_set.proposed_gain_db.is_finite() {
        return Err(format!(
            "AI ChangeSet '{}' has invalid gain values.",
            change_set.id
        ));
    }
    change_set.current_gain_db = change_set.current_gain_db.clamp(-90.0, 24.0);
    change_set.proposed_gain_db = change_set.proposed_gain_db.clamp(-90.0, 24.0);
    change_set.reason.truncate(4_096);
    change_set.expected_effect.truncate(4_096);
    change_set.risk.truncate(256);
    change_set.context.truncate(16);
    change_set
        .context
        .retain(|item| AI_CONTEXT_IDS.contains(&item.as_str()));
    change_set.context.dedup();
    Ok(())
}

fn normalize_rack(rack: &mut RackInstance) -> Result<(), String> {
    if rack.devices.len() > 256 {
        return Err("A rack cannot contain more than 256 devices.".into());
    }
    if rack.macros.len() > 64 {
        return Err("A session cannot contain more than 64 rack macros.".into());
    }
    for device in &mut rack.devices {
        normalize_rack_device(device)?;
    }
    for macro_control in &mut rack.macros {
        if macro_control.id.trim().is_empty() || macro_control.name.trim().is_empty() {
            return Err("Rack macros require non-empty ids and names.".into());
        }
        if !macro_control.value.is_finite() {
            return Err(format!(
                "Rack macro '{}' has an invalid value.",
                macro_control.name
            ));
        }
        macro_control.value = macro_control.value.clamp(0.0, 1.0);
    }
    Ok(())
}

fn normalize_rack_device(device: &mut RackDevice) -> Result<(), String> {
    if device.id.trim().is_empty() || device.name.trim().is_empty() {
        return Err("Rack devices require non-empty ids and names.".into());
    }
    if !device.gain_db.is_finite() {
        return Err(format!("Device '{}' has an invalid gain.", device.name));
    }
    device.gain_db = device.gain_db.clamp(-90.0, 24.0);
    for value in &mut device.parameter_values {
        *value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    if let Some(state) = device.state_data.as_ref()
        && state.len() > 4_000_000
    {
        device.state_data = Some(state.chars().take(4_000_000).collect());
    }
    Ok(())
}

fn normalize_snapshots(snapshots: &mut [SessionSnapshot]) -> Result<(), String> {
    if snapshots.len() > 16 {
        return Err("A session cannot contain more than 16 snapshots.".into());
    }
    for snapshot in snapshots {
        if snapshot.id.trim().is_empty() || snapshot.name.trim().is_empty() {
            return Err("Snapshots require non-empty ids and names.".into());
        }
        if !snapshot.master_db.is_finite() {
            return Err(format!(
                "Snapshot '{}' has an invalid master gain.",
                snapshot.name
            ));
        }
        snapshot.master_db = snapshot.master_db.clamp(-90.0, 0.0);
        snapshot.description.truncate(16_384);
        if snapshot.rack.len() > 256 {
            return Err(format!(
                "Snapshot '{}' contains too many rack devices.",
                snapshot.name
            ));
        }
    }
    Ok(())
}

fn normalize_arrangement(arrangement: &mut Arrangement) -> Result<(), String> {
    if arrangement.audio_clips.len() > 512 {
        return Err("An arrangement cannot contain more than 512 audio clips.".into());
    }
    let timebase = &arrangement.timebase;
    if timebase.ppq != TIMELINE_PPQ
        || !timebase.bpm.is_finite()
        || !(20.0..=400.0).contains(&timebase.bpm)
        || timebase.time_signature_numerator == 0
        || !matches!(timebase.time_signature_denominator, 1 | 2 | 4 | 8 | 16 | 32)
    {
        return Err("Arrangement timebase is invalid.".into());
    }
    if arrangement.loop_range.enabled
        && arrangement.loop_range.end_tick <= arrangement.loop_range.start_tick
    {
        return Err("Enabled loop range must have a positive duration.".into());
    }
    if let Some(punch_range) = arrangement.punch_range
        && punch_range.end_tick <= punch_range.start_tick
    {
        return Err("Punch range must have a positive duration.".into());
    }
    if arrangement.tracks.len() > 128 {
        return Err("An arrangement cannot contain more than 128 tracks.".into());
    }
    let mut unique_track_ids = std::collections::HashSet::new();
    for track in &mut arrangement.tracks {
        if track.id.trim().is_empty()
            || track.name.trim().is_empty()
            || !unique_track_ids.insert(track.id.as_str())
        {
            return Err("Tracks require non-empty ids and names.".into());
        }
        if !track.gain_db.is_finite() || !track.pan.is_finite() {
            return Err(format!("Track '{}' has invalid mix values.", track.name));
        }
        track.gain_db = track.gain_db.clamp(-90.0, 24.0);
        track.pan = track.pan.clamp(-1.0, 1.0);
        match track.kind {
            TrackKind::Audio if track.instrument.is_some() => {
                return Err(format!(
                    "Audio Track '{}' cannot host an Instrument.",
                    track.name
                ));
            }
            TrackKind::Instrument if track.audio_input.is_some() => {
                return Err(format!(
                    "Instrument Track '{}' cannot route a physical Audio Input.",
                    track.name
                ));
            }
            _ => {}
        }
        if track
            .midi_input
            .channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(format!(
                "Track '{}' has an invalid MIDI channel.",
                track.name
            ));
        }
        if let Some(instrument) = &mut track.instrument {
            normalize_rack_device(instrument)?;
        }
        normalize_rack(&mut track.rack)?;
    }
    let audio_clips = std::mem::take(&mut arrangement.audio_clips);
    let mut normalized_clips = Vec::with_capacity(audio_clips.len());
    let mut audio_clip_ids = std::collections::HashSet::new();
    for mut clip in audio_clips {
        if clip.id.trim().is_empty()
            || !audio_clip_ids.insert(clip.id.clone())
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || clip.asset_id.as_str().trim().is_empty()
        {
            return Err("Audio clips require ids, names, tracks and asset ids.".into());
        }
        if !clip.gain_db.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid gain.", clip.id));
        }
        if !clip.pan.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid pan.", clip.id));
        }
        clip.normalize_fields();
        let mut candidate = Arrangement {
            revision: arrangement.revision,
            timebase: arrangement.timebase,
            loop_range: arrangement.loop_range,
            punch_range: arrangement.punch_range,
            tracks: arrangement.tracks.clone(),
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
            automation_lanes: Vec::new(),
            markers: Vec::new(),
            recording_sessions: arrangement.recording_sessions.clone(),
            recording_passes: arrangement.recording_passes.clone(),
            takes: arrangement.takes.clone(),
        };
        candidate
            .add_audio_clip(clip, |_| true)
            .map_err(|error| error.to_string())?;
        normalized_clips.push(candidate.audio_clips.pop().expect("validated clip"));
    }
    arrangement.audio_clips = normalized_clips;
    if arrangement.midi_clips.len() > 256 {
        return Err("An arrangement cannot contain more than 256 MIDI clips.".into());
    }
    let track_ids = arrangement
        .tracks
        .iter()
        .map(|track| track.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut midi_clip_ids = std::collections::HashSet::new();
    for clip in &mut arrangement.midi_clips {
        if clip.id.trim().is_empty()
            || !midi_clip_ids.insert(clip.id.as_str())
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || !track_ids.contains(clip.track_id.as_str())
        {
            return Err("MIDI clips require non-empty ids and names.".into());
        }
        if clip.duration_ticks == 0 {
            return Err(format!("MIDI clip '{}' must have a duration.", clip.name));
        }
        if clip.notes.len() > 200_000 {
            return Err(format!(
                "MIDI clip '{}' contains too many notes.",
                clip.name
            ));
        }
        if clip.events.len() > 200_000 {
            return Err(format!(
                "MIDI clip '{}' contains too many events.",
                clip.name
            ));
        }
        let track = arrangement
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id)
            .expect("track id was checked above");
        if track.kind != TrackKind::Instrument {
            return Err(format!(
                "MIDI clip '{}' requires an Instrument Track.",
                clip.name
            ));
        }
        for note in &clip.notes {
            if note.id.trim().is_empty()
                || note.note > 127
                || note.velocity > 127
                || note.channel == 0
                || note.channel > 16
                || note.duration_ticks == 0
                || note.start_tick.0 >= clip.duration_ticks
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid note.",
                    clip.name
                ));
            }
        }
        for event in &clip.events {
            if event.id.trim().is_empty()
                || event.tick.0 >= clip.duration_ticks
                || event.channel == 0
                || event.channel > 16
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid event.",
                    clip.name
                ));
            }
        }
    }
    if arrangement.automation_lanes.len() > arrangement.tracks.len().saturating_mul(2) {
        return Err(
            "An arrangement cannot contain more than two Automation Lanes per Track.".into(),
        );
    }
    let mut lane_keys = std::collections::HashSet::new();
    let mut lane_ids = std::collections::HashSet::new();
    for lane in &mut arrangement.automation_lanes {
        if lane.id.trim().is_empty()
            || !lane_ids.insert(lane.id.as_str())
            || !track_ids.contains(lane.track_id.as_str())
            || !lane_keys.insert((lane.track_id.as_str(), lane.parameter))
        {
            return Err("Automation Lanes contain invalid or duplicate references.".into());
        }
        if lane.points.len() > 16_384 {
            return Err(format!(
                "Automation Lane '{}' contains too many points.",
                lane.id
            ));
        }
        lane.points.sort_by_key(|point| point.tick);
        let mut point_ids = std::collections::HashSet::new();
        let mut previous_tick = None;
        for point in &mut lane.points {
            if point.id.trim().is_empty()
                || !point_ids.insert(point.id.as_str())
                || previous_tick == Some(point.tick)
                || !point.value.is_finite()
            {
                return Err(format!(
                    "Automation Lane '{}' contains an invalid point.",
                    lane.id
                ));
            }
            point.value = match lane.parameter {
                AutomationParameter::Volume => point.value.clamp(-90.0, 24.0),
                AutomationParameter::Pan => point.value.clamp(-1.0, 1.0),
            };
            previous_tick = Some(point.tick);
        }
    }
    if arrangement.markers.len() > 256 {
        return Err("An arrangement cannot contain more than 256 markers.".into());
    }
    arrangement.markers.sort_by_key(|marker| marker.tick);
    arrangement
        .markers
        .retain(|marker| !marker.id.trim().is_empty());
    for marker in &mut arrangement.markers {
        let normalized_name: String = marker.name.trim().chars().take(80).collect();
        marker.name = if normalized_name.is_empty() {
            "Marker".into()
        } else {
            normalized_name
        };
    }
    if arrangement.recording_sessions.len() > 256
        || arrangement.recording_passes.len() > 4096
        || arrangement.takes.len() > 16_384
    {
        return Err(
            "An arrangement contains too many recording sessions, passes, or takes.".into(),
        );
    }
    for take in &mut arrangement.takes {
        take.migrate_legacy_audio_sources();
    }
    let session_ids = arrangement
        .recording_sessions
        .iter()
        .map(|recording| recording.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let pass_ids = arrangement
        .recording_passes
        .iter()
        .map(|pass| pass.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let take_ids = arrangement
        .takes
        .iter()
        .map(|take| take.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if session_ids.len() != arrangement.recording_sessions.len()
        || pass_ids.len() != arrangement.recording_passes.len()
        || take_ids.len() != arrangement.takes.len()
    {
        return Err("Recording Session, Pass, and Take IDs must be unique.".into());
    }
    for recording in &arrangement.recording_sessions {
        let mut slot_track_ids = std::collections::HashSet::new();
        if recording.id.trim().is_empty()
            || recording.track_slots.iter().any(|slot| {
                let take = arrangement
                    .takes
                    .iter()
                    .find(|take| take.id == slot.active_take_id);
                let audio_clip = arrangement
                    .audio_clips
                    .iter()
                    .find(|clip| clip.id == slot.timeline_clip_id);
                let midi_clip = arrangement
                    .midi_clips
                    .iter()
                    .find(|clip| clip.id == slot.timeline_clip_id);
                !slot_track_ids.insert(slot.track_id.as_str())
                    || !track_ids.contains(slot.track_id.as_str())
                    || take.is_none_or(|take| {
                        take.session_id != recording.id || take.track_id != slot.track_id
                    })
                    || match (audio_clip, midi_clip) {
                        (Some(clip), None) => {
                            clip.track_id != slot.track_id
                                || clip.recording_take_id.as_deref()
                                    != Some(slot.active_take_id.as_str())
                        }
                        (None, Some(clip)) => {
                            clip.track_id != slot.track_id
                                || clip.recording_take_id.as_deref()
                                    != Some(slot.active_take_id.as_str())
                        }
                        _ => true,
                    }
            })
            || recording.pass_ids.iter().any(|id| {
                arrangement
                    .recording_passes
                    .iter()
                    .find(|pass| pass.id == *id)
                    .is_none_or(|pass| pass.session_id != recording.id)
            })
        {
            return Err(
                "Recording Sessions contain invalid track, take, or range references.".into(),
            );
        }
    }
    for pass in &arrangement.recording_passes {
        let mut pass_take_ids = std::collections::HashSet::new();
        if pass.id.trim().is_empty()
            || !session_ids.contains(pass.session_id.as_str())
            || pass.ordinal == 0
            || pass.duration_ticks == 0
            || pass.track_take_ids.iter().any(|id| {
                !pass_take_ids.insert(id.as_str())
                    || arrangement
                        .takes
                        .iter()
                        .find(|take| take.id == *id)
                        .is_none_or(|take| {
                            take.pass_id != pass.id || take.session_id != pass.session_id
                        })
            })
        {
            return Err("Recording Passes contain invalid references or ranges.".into());
        }
    }
    for take in &arrangement.takes {
        let pass = arrangement
            .recording_passes
            .iter()
            .find(|pass| pass.id == take.pass_id);
        let audio_sources_valid = take
            .raw_audio
            .iter()
            .chain(take.processed_audio.iter())
            .all(|source| {
                !source.asset_id.as_str().trim().is_empty()
                    && source.source_end_sample > source.source_start_sample
                    && source.sample_rate > 0
                    && (source.tail_end_sample == 0
                        || source.tail_end_sample >= source.source_end_sample)
            });
        let has_audio_source = take.raw_audio.is_some() || take.processed_audio.is_some();
        if take.id.trim().is_empty()
            || !session_ids.contains(take.session_id.as_str())
            || pass.is_none_or(|pass| {
                pass.session_id != take.session_id
                    || !pass.track_take_ids.iter().any(|id| id == &take.id)
            })
            || !track_ids.contains(take.track_id.as_str())
            || take.duration_ticks == 0
            || take.source_end_sample <= take.source_start_sample
            || (!has_audio_source && take.midi_asset_id.is_none())
            || !audio_sources_valid
        {
            return Err(
                "Recording Takes contain invalid session, pass, track, or range references.".into(),
            );
        }
    }
    Ok(())
}

fn normalize_sample_pads(pads: &mut [SamplePad]) -> Result<(), String> {
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    for pad in pads {
        if pad.id.trim().is_empty()
            || pad.name.trim().is_empty()
            || pad.asset_id.as_str().trim().is_empty()
        {
            return Err("Sample pads require ids, names and asset ids.".into());
        }
        if pad.end_ms <= pad.start_ms {
            return Err(format!(
                "Sample pad '{}' has an invalid slice range.",
                pad.name
            ));
        }
        if !pad.gain_db.is_finite() {
            return Err(format!("Sample pad '{}' has an invalid gain.", pad.name));
        }
        pad.gain_db = pad.gain_db.clamp(-90.0, 24.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{Provenance, mint_asset_id};

    fn clip(track_id: &str, asset_id: AssetId) -> AudioClip {
        AudioClip::full_source(
            "clip:1".into(),
            "clip".into(),
            track_id.into(),
            asset_id,
            TimelineTick(0),
            1_000,
            1_000,
        )
    }

    fn midi_clip(track_id: &str) -> MidiClip {
        MidiClip {
            id: "midi-clip:1".into(),
            name: "MIDI".into(),
            track_id: track_id.into(),
            asset_id: None,
            start_tick: TimelineTick(960),
            duration_ticks: 1_920,
            notes: vec![MidiNote {
                id: "note:1".into(),
                note: 60,
                start_tick: TimelineTick(240),
                duration_ticks: 480,
                velocity: 100,
                channel: 1,
            }],
            events: vec![MidiEvent {
                id: "event:1".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(720),
                channel: 1,
                data1: 64,
                data2: 127,
            }],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        }
    }

    #[test]
    fn workspace_has_exactly_two_variants() {
        let all = [Workspace::Design, Workspace::Arrange];
        assert_eq!(all.len(), 2);
        assert!(matches!(
            CreativeSession::new(0).workspace,
            Workspace::Arrange
        ));
    }

    #[test]
    fn workspace_serializes_to_stable_lowercase_values() {
        assert_eq!(
            serde_json::to_string(&Workspace::Arrange).unwrap(),
            "\"arrange\""
        );
        assert_eq!(
            serde_json::to_string(&Workspace::Design).unwrap(),
            "\"design\""
        );
    }

    #[test]
    fn legacy_play_workspace_loads_as_arrange_and_serializes_canonically() {
        let mut value = serde_json::to_value(CreativeSession::new(0)).unwrap();
        value["workspace"] = serde_json::json!("play");

        let session = deserialize_session(&serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(session.workspace, Workspace::Arrange);
        assert_eq!(
            serde_json::to_value(session).unwrap()["workspace"],
            serde_json::json!("arrange")
        );
    }

    #[test]
    fn design_context_holds_tool_and_target_asset() {
        let id = mint_asset_id();
        let ctx = DesignContext {
            active_tool: DesignTool::Separate,
            target_asset_id: Some(id.clone()),
        };
        assert_eq!(ctx.active_tool, DesignTool::Separate);
        assert_eq!(ctx.target_asset_id, Some(id));
    }

    #[test]
    fn arrangement_cannot_add_a_clip_to_an_unknown_track() {
        let mut arrangement = Arrangement::default();
        let asset = mint_asset_id();
        let clip = clip("missing", asset);
        let error = arrangement.add_audio_clip(clip, |_| true).unwrap_err();
        assert!(matches!(error, DomainError::UnknownTrack(_)));
    }

    #[test]
    fn arrangement_cannot_add_an_audio_clip_to_an_instrument_track() {
        let mut arrangement = Arrangement::default();
        arrangement.tracks.push(Track {
            ..Track::instrument("instrument".into(), "Instrument".into())
        });
        let error = arrangement
            .add_audio_clip(clip("instrument", mint_asset_id()), |_| true)
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn midi_clip_requires_an_instrument_track_and_preserves_non_note_events() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        assert_eq!(
            arrangement.midi_clips[0].events[0].kind,
            MidiEventKind::ControlChange
        );
        assert_eq!(arrangement.revision, 1);
        let mut audio_arrangement = Arrangement::default();
        audio_arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        assert!(audio_arrangement.add_midi_clip(midi_clip("audio")).is_err());
    }

    #[test]
    fn trimming_midi_clip_rebases_notes_and_events_to_the_new_start() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        arrangement
            .trim_midi_clip("midi-clip:1", TimelineTick(1_200), 960)
            .unwrap();

        let clip = &arrangement.midi_clips[0];
        assert_eq!(clip.start_tick, TimelineTick(1_200));
        assert_eq!(clip.duration_ticks, 960);
        assert_eq!(clip.notes[0].start_tick, TimelineTick(0));
        assert_eq!(clip.events[0].tick, TimelineTick(480));
    }

    #[test]
    fn arrangement_rejects_inverted_source_range() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::audio("main".into(), "Main".into()));
        let asset = mint_asset_id();
        let mut clip = clip("main", asset);
        clip.source_range = FrameRange {
            start: 800,
            end: 500,
        };
        let error = arrangement.add_audio_clip(clip, |_| true).unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn arrangement_rejects_clip_with_unknown_asset() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::audio("main".into(), "Main".into()));
        let asset = mint_asset_id();
        let clip = clip("main", asset);
        let error = arrangement.add_audio_clip(clip, |_| false).unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn arrangement_accepts_a_valid_clip_and_carries_asset_id() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::audio("main".into(), "Main".into()));
        let asset = mint_asset_id();
        let clip = clip("main", asset.clone());
        arrangement
            .add_audio_clip(clip.clone(), |id| id == &asset)
            .unwrap();
        assert_eq!(arrangement.audio_clips.len(), 1);
        assert_eq!(arrangement.audio_clips[0].asset_id, asset);
    }

    #[test]
    fn update_timebase_changes_the_project_clock_once() {
        let mut arrangement = Arrangement::default();
        let revision = arrangement.revision;
        arrangement
            .update_timebase(ProjectTimebase {
                ppq: TIMELINE_PPQ,
                bpm: 98.5,
                time_signature_numerator: 7,
                time_signature_denominator: 8,
            })
            .unwrap();

        assert_eq!(arrangement.timebase.bpm, 98.5);
        assert_eq!(arrangement.timebase.time_signature_numerator, 7);
        assert_eq!(arrangement.revision, revision + 1);
        assert!(
            arrangement
                .update_timebase(ProjectTimebase {
                    bpm: 10.0,
                    ..arrangement.timebase
                })
                .is_err()
        );
    }

    fn arrangement_with_clip(asset: AssetId) -> Arrangement {
        let mut arrangement = Arrangement {
            revision: 0,
            timebase: ProjectTimebase::default(),
            loop_range: TimelineLoopRange::default(),
            punch_range: None,
            tracks: vec![
                Track::audio("main".into(), "Main".into()),
                Track {
                    ..Track::audio("extra".into(), "Extra".into())
                },
            ],
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
            automation_lanes: Vec::new(),
            markers: Vec::new(),
            recording_sessions: Vec::new(),
            recording_passes: Vec::new(),
            takes: Vec::new(),
        };
        let mut clip = clip("main", asset);
        clip.id = "clip:1".into();
        clip.start_tick = TimelineTick(1_920);
        arrangement
            .add_audio_clip(clip, |_| true)
            .expect("seed clip is valid");
        arrangement
    }

    #[test]
    fn removing_a_track_removes_its_clips_but_keeps_other_tracks() {
        // Arrange
        let mut arrangement = arrangement_with_clip(mint_asset_id());

        // Act
        arrangement.remove_track("main").unwrap();

        // Assert
        assert_eq!(arrangement.tracks.len(), 1);
        assert_eq!(arrangement.tracks[0].id, "extra");
        assert!(arrangement.audio_clips.is_empty());
        assert_eq!(arrangement.revision, 2);
    }

    #[test]
    fn automation_points_are_canonical_and_follow_track_ownership() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::audio("main".into(), "Main".into()));
        arrangement.automation_lanes.push(AutomationLane {
            id: "automation:main:volume".into(),
            track_id: "main".into(),
            parameter: AutomationParameter::Volume,
            points: vec![
                AutomationPoint {
                    id: "late".into(),
                    tick: TimelineTick(960),
                    value: 100.0,
                },
                AutomationPoint {
                    id: "early".into(),
                    tick: TimelineTick(0),
                    value: -100.0,
                },
            ],
        });

        normalize_arrangement(&mut arrangement).unwrap();

        assert_eq!(arrangement.automation_lanes[0].points[0].id, "early");
        assert_eq!(arrangement.automation_lanes[0].points[0].value, -90.0);
        assert_eq!(arrangement.automation_lanes[0].points[1].value, 24.0);
        arrangement.remove_track("main").unwrap();
        assert!(arrangement.automation_lanes.is_empty());
    }

    #[test]
    fn reordering_a_track_keeps_clip_ownership_unchanged() {
        // Arrange
        let mut arrangement = arrangement_with_clip(mint_asset_id());

        // Act
        arrangement.reorder_track("extra", 0).unwrap();

        // Assert
        assert_eq!(arrangement.tracks[0].id, "extra");
        assert_eq!(arrangement.tracks[1].id, "main");
        assert_eq!(arrangement.audio_clips[0].track_id, "main");
        assert_eq!(arrangement.revision, 2);
    }

    #[test]
    fn update_audio_clip_applies_canonical_clamps_and_keeps_other_fields() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    gain_db: Some(999.0),
                    pan: Some(-5.0),
                    fade_in: Some(FrameDuration {
                        frames: 10_000,
                        sample_rate: 1_000,
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        let updated = arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == "clip:1")
            .expect("clip remains");
        assert_eq!(updated.gain_db, 24.0);
        assert_eq!(updated.pan, -1.0);
        assert_eq!(updated.fade_in.frames, 1_000);
        // Untouched fields are preserved.
        assert_eq!(updated.start_tick, TimelineTick(1_920));
    }

    #[test]
    fn update_audio_clip_rejects_move_to_missing_track() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    track_id: Some("ghost".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::UnknownTrack(_)));
        // The clip stays on its original track.
        assert_eq!(arrangement.audio_clips[0].track_id, "main");
    }

    #[test]
    fn moving_audio_clip_to_an_instrument_track_is_rejected() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));

        let error = arrangement
            .move_audio_clips(vec![AudioClipMove {
                clip_id: "clip:1".into(),
                start_tick: TimelineTick(0),
                track_id: "instrument".into(),
            }])
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidClip(_)));
        assert_eq!(arrangement.audio_clips[0].track_id, "main");
    }

    #[test]
    fn moving_midi_clip_to_an_audio_track_is_rejected() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        let error = arrangement
            .move_midi_clips(vec![MidiClipMove {
                clip_id: "midi-clip:1".into(),
                start_tick: TimelineTick(0),
                track_id: "audio".into(),
            }])
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidClip(_)));
        assert_eq!(arrangement.midi_clips[0].track_id, "instrument");
    }

    #[test]
    fn updating_midi_clip_to_an_audio_track_is_rejected() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        let error = arrangement
            .update_midi_clip(
                "midi-clip:1",
                MidiClipPatch {
                    track_id: Some("audio".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidClip(_)));
        assert_eq!(arrangement.midi_clips[0].track_id, "instrument");
    }

    #[test]
    fn update_audio_clip_rejects_inverted_source_range_after_patch() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    source_range: Some(FrameRange {
                        start: 800,
                        end: 100,
                    }),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn update_audio_clip_reports_unknown_clip() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "missing",
                AudioClipPatch {
                    muted: Some(true),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn remove_timeline_clip_drops_the_target_and_advances_revision() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let previous_revision = arrangement.revision;

        arrangement
            .remove_timeline_clips(&["clip:1".into()], &[])
            .unwrap();

        assert!(arrangement.audio_clips.is_empty());
        assert_eq!(arrangement.revision, previous_revision + 1);
    }

    #[test]
    fn remove_timeline_clip_reports_unknown_clip() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        assert!(matches!(
            arrangement
                .remove_timeline_clips(&["missing".into()], &[])
                .unwrap_err(),
            DomainError::InvalidClip(_)
        ));
    }

    #[test]
    fn split_preserves_the_asset_and_partitions_the_source_range() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .split_audio_clip("clip:1", TimelineTick(2_880), "clip:right".into())
            .unwrap();

        assert_eq!(arrangement.audio_clips.len(), 2);
        assert_eq!(
            arrangement.audio_clips[0].source_range,
            FrameRange { start: 0, end: 500 }
        );
        assert_eq!(
            arrangement.audio_clips[1].source_range,
            FrameRange {
                start: 500,
                end: 1_000
            }
        );
        assert_eq!(arrangement.audio_clips[1].start_tick, TimelineTick(2_880));
        assert_eq!(
            arrangement.audio_clips[0].asset_id,
            arrangement.audio_clips[1].asset_id
        );
    }

    #[test]
    fn trim_and_duplicate_are_non_destructive_arrangement_edits() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .trim_audio_clip(
                "clip:1",
                TimelineTick(2_400),
                FrameRange {
                    start: 250,
                    end: 750,
                },
                1_000,
            )
            .unwrap();
        arrangement
            .duplicate_audio_clip("clip:1", "clip:copy".into())
            .unwrap();

        assert_eq!(
            arrangement.audio_clips[0].source_range,
            FrameRange {
                start: 250,
                end: 750
            }
        );
        assert_eq!(arrangement.audio_clips[0].timeline_duration.frames, 500);
        assert_eq!(arrangement.audio_clips[1].id, "clip:copy");
        assert_eq!(arrangement.audio_clips[1].start_tick, TimelineTick(3_360));
    }

    #[test]
    fn moving_multiple_clips_preserves_one_edit_revision() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .duplicate_audio_clip("clip:1", "clip:2".into())
            .unwrap();
        let revision = arrangement.revision;
        arrangement
            .move_audio_clips(vec![
                AudioClipMove {
                    clip_id: "clip:1".into(),
                    start_tick: TimelineTick(0),
                    track_id: "extra".into(),
                },
                AudioClipMove {
                    clip_id: "clip:2".into(),
                    start_tick: TimelineTick(1_920),
                    track_id: "extra".into(),
                },
            ])
            .unwrap();

        assert_eq!(arrangement.revision, revision + 1);
        assert!(
            arrangement
                .audio_clips
                .iter()
                .all(|clip| clip.track_id == "extra")
        );
        assert_eq!(
            arrangement.audio_clips[1].start_tick.0 - arrangement.audio_clips[0].start_tick.0,
            1_920
        );
    }

    #[test]
    fn paste_preserves_relative_timing_and_asset_references() {
        let asset = mint_asset_id();
        let mut arrangement = arrangement_with_clip(asset.clone());
        arrangement
            .duplicate_audio_clip("clip:1", "clip:2".into())
            .unwrap();
        arrangement.midi_clips.push(MidiClip {
            id: "midi:1".into(),
            name: "MIDI".into(),
            track_id: "main".into(),
            asset_id: None,
            start_tick: TimelineTick(5_760),
            duration_ticks: 960,
            notes: Vec::new(),
            events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        arrangement
            .paste_timeline_clips(
                &["clip:1".into(), "clip:2".into()],
                &["midi:1".into()],
                &["clip:3".into(), "clip:4".into()],
                &["midi:2".into()],
                TimelineTick(9_600),
            )
            .unwrap();

        assert_eq!(arrangement.audio_clips[2].start_tick, TimelineTick(9_600));
        assert_eq!(arrangement.audio_clips[3].start_tick, TimelineTick(11_520));
        assert!(
            arrangement.audio_clips[2..]
                .iter()
                .all(|clip| clip.asset_id == asset)
        );
        assert_eq!(arrangement.midi_clips[1].start_tick, TimelineTick(13_440));
    }

    #[test]
    fn explicit_crossfade_uses_the_overlap_on_both_clips() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .duplicate_audio_clip("clip:1", "clip:2".into())
            .unwrap();
        arrangement.audio_clips[1].start_tick = TimelineTick(2_880);

        arrangement
            .crossfade_audio_clips("clip:1", "clip:2")
            .unwrap();

        assert_eq!(arrangement.audio_clips[0].fade_out.frames, 500);
        assert_eq!(arrangement.audio_clips[1].fade_in.frames, 500);
    }

    #[test]
    fn new_session_has_arrangement_tracks_and_default_rack() {
        let session = CreativeSession::new(0);
        assert!(session.arrangement.tracks.is_empty());
        assert_eq!(session.rack.devices.len(), 3);
        assert_eq!(
            session.play_state.sample_instrument.pads,
            Vec::<SamplePad>::new()
        );
        // An unused provenance reference keeps the asset import meaningful here.
        let _ = Provenance::recorded_root();
    }

    #[test]
    fn legacy_recording_shape_migrates_to_passes_slots_and_clip_variants() {
        let mut value = serde_json::to_value(CreativeSession::new(0)).unwrap();
        value["arrangement"]["tracks"] =
            serde_json::json!([Track::audio("track:legacy".into(), "Legacy".into())]);
        value["arrangement"]["audioClips"] = serde_json::json!([]);
        value["arrangement"]["takes"] = serde_json::json!([{
            "id": "take:legacy",
            "sessionId": "recording:legacy",
            "trackId": "track:legacy",
            "startTick": 960,
            "durationTicks": 480,
            "active": true,
            "activeVariant": "processed",
            "clipId": "clip:legacy"
        }]);
        value["arrangement"]["recordingSessions"] = serde_json::json!([{
            "id": "recording:legacy",
            "startTick": 960,
            "takeIds": ["take:legacy"]
        }]);

        let migrated = deserialize_session(&serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(migrated.arrangement.recording_passes.len(), 1);
        assert_eq!(
            migrated.arrangement.recording_passes[0].track_take_ids,
            ["take:legacy"]
        );
        assert!(!migrated.arrangement.takes[0].pass_id.is_empty());
        assert_eq!(
            migrated.arrangement.recording_sessions[0].track_slots[0].active_take_id,
            "take:legacy"
        );
    }

    fn session_with_recording_relations() -> CreativeSession {
        let mut session = CreativeSession::new(0);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        let asset_id = mint_asset_id();
        let take = RecordingTakeRecord {
            id: "take:1".into(),
            session_id: "recording:1".into(),
            pass_id: "pass:1".into(),
            track_id: "track:audio".into(),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            source_start_sample: 0,
            source_end_sample: 48_000,
            raw_audio: Some(TakeAudioSource {
                asset_id: asset_id.clone(),
                source_start_sample: 0,
                source_end_sample: 48_000,
                tail_end_sample: 48_000,
                sample_rate: 48_000,
            }),
            processed_audio: None,
            raw_audio_asset_id: None,
            processed_audio_asset_id: None,
            midi_asset_id: None,
        };
        let mut clip = AudioClip::full_source(
            "clip:1".into(),
            "Take".into(),
            "track:audio".into(),
            asset_id,
            TimelineTick(0),
            48_000,
            48_000,
        );
        clip.recording_take_id = Some(take.id.clone());
        session.arrangement.audio_clips.push(clip);
        session.arrangement.takes.push(take);
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: "pass:1".into(),
                session_id: "recording:1".into(),
                ordinal: 1,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: vec!["take:1".into()],
            });
        session
            .arrangement
            .recording_sessions
            .push(RecordingSessionRecord {
                id: "recording:1".into(),
                start_tick: TimelineTick(0),
                track_slots: vec![RecordingSessionTrackSlot {
                    track_id: "track:audio".into(),
                    active_take_id: "take:1".into(),
                    timeline_clip_id: "clip:1".into(),
                }],
                pass_ids: vec!["pass:1".into()],
            });
        session
    }

    #[test]
    fn legacy_shared_audio_range_migrates_to_variant_sources() {
        let mut value = serde_json::to_value(session_with_recording_relations()).unwrap();
        let take = &mut value["arrangement"]["takes"][0];
        take.as_object_mut().unwrap().remove("rawAudio");
        take["rawAudioAssetId"] = serde_json::to_value(mint_asset_id()).unwrap();
        take["sourceStartSample"] = serde_json::json!(120);
        take["sourceEndSample"] = serde_json::json!(480);

        let migrated = deserialize_session(&serde_json::to_vec(&value).unwrap()).unwrap();
        let source = migrated.arrangement.takes[0].raw_audio.as_ref().unwrap();
        assert_eq!(source.source_start_sample, 120);
        assert_eq!(source.source_end_sample, 480);
        let persisted = serde_json::to_value(migrated).unwrap();
        assert!(
            persisted["arrangement"]["takes"][0]
                .get("rawAudioAssetId")
                .is_none()
        );
    }

    #[test]
    fn recording_relation_validation_rejects_duplicates_and_cross_links() {
        let valid = session_with_recording_relations()
            .validate_and_normalize()
            .unwrap();

        let mut duplicate = valid.clone();
        duplicate
            .arrangement
            .takes
            .push(duplicate.arrangement.takes[0].clone());
        assert!(duplicate.validate_and_normalize().is_err());

        let mut wrong_pass = valid.clone();
        wrong_pass.arrangement.recording_passes[0].track_take_ids = vec!["missing-take".into()];
        assert!(wrong_pass.validate_and_normalize().is_err());

        let mut wrong_slot = valid;
        wrong_slot.arrangement.recording_sessions[0].track_slots[0].active_take_id =
            "missing-take".into();
        assert!(wrong_slot.validate_and_normalize().is_err());

        let mut missing_from_pass = session_with_recording_relations();
        missing_from_pass.arrangement.recording_passes[0]
            .track_take_ids
            .clear();
        assert!(missing_from_pass.validate_and_normalize().is_err());
    }

    #[test]
    fn deleting_a_recording_slot_clip_prunes_only_its_canonical_take_links() {
        let mut session = session_with_recording_relations();
        let mut copy = session.arrangement.audio_clips[0].clone();
        copy.id = "clip:copy".into();
        session.arrangement.audio_clips.push(copy);

        session
            .arrangement
            .remove_timeline_clips(&["clip:copy".into()], &[])
            .unwrap();
        assert_eq!(session.arrangement.takes.len(), 1);
        assert_eq!(session.arrangement.recording_sessions.len(), 1);

        session
            .arrangement
            .remove_timeline_clips(&["clip:1".into()], &[])
            .unwrap();
        assert!(session.arrangement.takes.is_empty());
        assert!(session.arrangement.recording_passes.is_empty());
        assert!(session.arrangement.recording_sessions.is_empty());
        assert!(session.validate_and_normalize().is_ok());
    }

    #[test]
    fn deleting_a_slot_detaches_a_remaining_copy_clip_from_the_removed_take() {
        let mut session = session_with_recording_relations();
        let mut copy = session.arrangement.audio_clips[0].clone();
        copy.id = "clip:copy".into();
        let asset_id = copy.asset_id.clone();
        session.arrangement.audio_clips.push(copy);

        session
            .arrangement
            .remove_timeline_clips(&["clip:1".into()], &[])
            .unwrap();

        let copy = session
            .arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == "clip:copy")
            .unwrap();
        assert!(copy.recording_take_id.is_none());
        assert_eq!(copy.asset_id, asset_id);
        assert!(session.validate_and_normalize().is_ok());
    }

    #[test]
    fn deleting_a_recorded_track_prunes_relations_but_not_assets() {
        let mut session = session_with_recording_relations();
        let asset_id = session.arrangement.takes[0]
            .raw_audio
            .as_ref()
            .unwrap()
            .asset_id
            .clone();
        session.arrangement.remove_track("track:audio").unwrap();
        assert!(session.arrangement.takes.is_empty());
        assert!(session.arrangement.recording_passes.is_empty());
        assert!(session.arrangement.recording_sessions.is_empty());
        // The domain intentionally holds no asset store and therefore cannot
        // delete this source reference from storage.
        assert!(!asset_id.as_str().is_empty());
        assert!(session.validate_and_normalize().is_ok());
    }

    #[test]
    fn moving_a_recording_slot_clip_to_another_track_is_rejected() {
        let mut session = session_with_recording_relations();
        session
            .arrangement
            .tracks
            .push(Track::audio("track:other".into(), "Other".into()));
        let error = session
            .arrangement
            .move_audio_clips(vec![AudioClipMove {
                clip_id: "clip:1".into(),
                track_id: "track:other".into(),
                start_tick: TimelineTick(0),
            }])
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }
}
