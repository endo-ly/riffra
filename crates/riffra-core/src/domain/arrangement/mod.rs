//! Arrangement, Track, Clip, MIDI, and automation domain models.

use crate::DomainError;
use crate::domain::asset::AssetId;
use crate::domain::rack::{RackDevice, RackInstance};
use crate::domain::recording::*;
use crate::domain::timeline::{
    FrameDuration, FrameRange, ProjectTimebase, TIMELINE_PPQ, TimelineLoopRange,
    TimelinePunchRange, TimelineTick,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    /// Presentation color as `#rrggbb`. `None` keeps the Track's kind default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
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

/// A partial update for a timeline Track.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitoring: Option<MonitoringState>,
    /// Sets the presentation color; an empty string clears it back to the
    /// kind default.
    pub color: Option<String>,
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
            color: None,
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

/// The amplitude curve used by clip fades.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FadeShape {
    Linear,
    #[default]
    EqualPower,
    Smooth,
}

impl FadeShape {
    /// Engine-facing discriminant matching the audio sidecar contract.
    pub fn as_code(self) -> u8 {
        match self {
            FadeShape::Linear => 0,
            FadeShape::EqualPower => 1,
            FadeShape::Smooth => 2,
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
    #[serde(default)]
    pub fade_shape: FadeShape,
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
            fade_shape: FadeShape::EqualPower,
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
    pub fade_shape: Option<FadeShape>,
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

    /// Inserts identity-bearing MIDI notes after validating the resulting Clip.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is empty, note identities collide, or
    /// the resulting Clip violates MIDI validation rules.
    pub fn insert_midi_notes(
        &mut self,
        clip_id: &str,
        notes: Vec<MidiNote>,
    ) -> Result<(), DomainError> {
        if notes.is_empty() {
            return Err(DomainError::InvalidClip(
                "no midi notes were inserted.".into(),
            ));
        }
        let index = self
            .midi_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("midi clip '{clip_id}' not found.")))?;
        let mut candidate = self.midi_clips[index].clone();
        let mut note_ids = candidate
            .notes
            .iter()
            .map(|note| note.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if notes.iter().any(|note| !note_ids.insert(note.id.as_str())) {
            return Err(DomainError::InvalidClip(
                "midi note identities must be unique.".into(),
            ));
        }
        let required_duration = notes
            .iter()
            .map(|note| note.start_tick.0.saturating_add(note.duration_ticks))
            .max()
            .unwrap_or(1);
        candidate.duration_ticks = candidate.duration_ticks.max(required_duration);
        candidate.notes.extend(notes);
        self.validate_midi_clip(&candidate)?;
        self.midi_clips[index] = candidate;
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
            clip.duration_ticks = clip
                .duration_ticks
                .max(note.start_tick.0.saturating_add(note.duration_ticks.max(1)));
            clip.notes.push(note);
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Removes multiple MIDI notes as one domain edit.
    ///
    /// # Errors
    ///
    /// Returns an error when the selection is empty, the Clip is unknown, or
    /// any selected Note ID is missing.
    pub fn remove_midi_notes(
        &mut self,
        clip_id: &str,
        note_ids: &[String],
    ) -> Result<(), DomainError> {
        if note_ids.is_empty() {
            return Err(DomainError::InvalidClip(
                "no midi notes were selected.".into(),
            ));
        }
        let clip = self
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("midi clip '{clip_id}' not found.")))?;
        if note_ids
            .iter()
            .any(|id| !clip.notes.iter().any(|note| note.id == *id))
        {
            return Err(DomainError::InvalidClip(
                "one or more midi notes were not found.".into(),
            ));
        }
        clip.notes
            .retain(|note| !note_ids.iter().any(|id| id == &note.id));
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
        if let Some(fade_shape) = patch.fade_shape {
            clip.fade_shape = fade_shape;
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
