//! CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! active workspace, design context, play state (including the live sample
//! instrument), the [`Arrangement`], the running rack, snapshots, and session
//! settings. It deliberately does not own audio/MIDI file bodies, the Library
//! index, recording files, or background-job state.

use crate::asset::AssetId;
use crate::errors::DomainError;
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    pub(crate) fn milliseconds_to_ticks(self, milliseconds: f64) -> TimelineTick {
        let ticks = milliseconds.max(0.0) * self.bpm * f64::from(self.ppq) / 60_000.0;
        TimelineTick(ticks.round().max(0.0) as u64)
    }

    pub(crate) fn frames_to_ticks(self, frames: u64, sample_rate: u32) -> TimelineTick {
        self.milliseconds_to_ticks(frames as f64 * 1000.0 / f64::from(sample_rate))
    }

    fn ticks_to_frames(self, ticks: u64, sample_rate: u32) -> u64 {
        (ticks as f64 * f64::from(sample_rate) * 60.0 / (self.bpm * f64::from(self.ppq)))
            .round()
            .max(0.0) as u64
    }
}

/// Persisted loop selection. Disabled ranges retain their endpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineLoopRange {
    pub enabled: bool,
    pub start_tick: TimelineTick,
    pub end_tick: TimelineTick,
}

/// The four fixed workspaces. `Sample`, `Analyze`, and `Separate` are not
/// workspaces; they are [`DesignTool`]s reached from [`Workspace::Design`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    Home,
    Play,
    Design,
    Arrange,
}

/// A design surface reached from the Design workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignTool {
    Sample,
    Analyze,
    Separate,
}

/// What the Design workspace is currently aimed at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignContext {
    pub active_tool: DesignTool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Instrument,
}

/// A timeline track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub rack: RackInstance,
}

/// Audio Track input monitoring state. `Auto` monitors only while the track is
/// armed; `On` always monitors; `Off` never monitors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringState {
    #[default]
    Off,
    Auto,
    On,
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
            rack: empty_track_rack(),
        }
    }
}

/// A single MIDI note inside a [`MidiClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub id: String,
    pub note: u8,
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// A non-destructive MIDI clip on the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    pub name: String,
    pub track_id: String,
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    #[serde(default)]
    pub muted: bool,
}

/// A non-destructive audio clip referencing an [`AssetId`].
///
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: String,
    pub track_id: String,
    pub asset_id: AssetId,
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
}

impl AudioClip {
    /// Creates a clip that references an entire source at its native rate.
    pub(crate) fn full_source(
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
        }
    }

    /// Clamps and normalizes the production-managed numeric fields in place.
    ///
    /// This is the single canonical place where clip gain, pan, and fade
    /// limits live; callers supply raw values and rely on this method instead
    /// of replicating the rule.
    pub(crate) fn normalize_fields(&mut self) {
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<TimelineTick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_duration: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<FrameRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<FrameDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipMove {
    pub clip_id: String,
    pub start_tick: TimelineTick,
    pub track_id: String,
}

/// The Arrange workspace's production state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub revision: u64,
    pub timebase: ProjectTimebase,
    pub loop_range: TimelineLoopRange,
    pub tracks: Vec<Track>,
    pub audio_clips: Vec<AudioClip>,
    pub midi_clips: Vec<MidiClip>,
    #[serde(default)]
    pub markers: Vec<Marker>,
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
/// live *playback* state on the Play side, distinct from a saved
/// [`crate::asset::AssetKind::Sample`] asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleInstrumentState {
    #[serde(default)]
    pub pads: Vec<SamplePad>,
}

/// Play-side live state (instrument and performance configuration).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayState {
    #[serde(default)]
    pub sample_instrument: SampleInstrumentState,
}

/// A captured A/B rack + master snapshot for quick comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// A reversible AI-proposed change record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChangeSet {
    pub id: String,
    pub created_at_ms: u64,
    pub permission: String,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    #[serde(default = "default_ai_permission")]
    pub ai_permission: String,
    #[serde(default = "default_ai_context")]
    pub ai_context: Vec<String>,
    #[serde(default)]
    pub ai_history: Vec<AiChangeSet>,
}

fn default_ai_permission() -> String {
    "Suggest".into()
}

fn default_ai_context() -> Vec<String> {
    vec!["analysis".into(), "selectedClip".into()]
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Creates a fresh session in the Home workspace with the default rack,
    /// arrangement, and safe (muted) settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            workspace: Workspace::Home,
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
                ai_permission: default_ai_permission(),
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
        if !matches!(
            settings.ai_permission.as_str(),
            "Explain" | "Suggest" | "Apply"
        ) {
            return Err("AI permission must be Explain, Suggest, or Apply.".into());
        }
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
    if !matches!(
        change_set.permission.as_str(),
        "Explain" | "Suggest" | "Apply"
    ) {
        return Err("AI ChangeSet permission is invalid.".into());
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
        if device.id.trim().is_empty() || device.name.trim().is_empty() {
            return Err("Rack devices require non-empty ids and names.".into());
        }
        if !device.gain_db.is_finite() {
            return Err(format!("Device '{}' has an invalid gain.", device.name));
        }
        device.gain_db = device.gain_db.clamp(-90.0, 24.0);
        if device.parameter_values.len() > 512 {
            device.parameter_values.truncate(512);
        }
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
    if arrangement.tracks.len() > 128 {
        return Err("An arrangement cannot contain more than 128 tracks.".into());
    }
    for track in &mut arrangement.tracks {
        if track.id.trim().is_empty() || track.name.trim().is_empty() {
            return Err("Tracks require non-empty ids and names.".into());
        }
        if !track.gain_db.is_finite() || !track.pan.is_finite() {
            return Err(format!("Track '{}' has invalid mix values.", track.name));
        }
        track.gain_db = track.gain_db.clamp(-90.0, 24.0);
        track.pan = track.pan.clamp(-1.0, 1.0);
        normalize_rack(&mut track.rack)?;
    }
    let audio_clips = std::mem::take(&mut arrangement.audio_clips);
    let mut normalized_clips = Vec::with_capacity(audio_clips.len());
    for mut clip in audio_clips {
        if clip.id.trim().is_empty()
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
            tracks: arrangement.tracks.clone(),
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
            markers: Vec::new(),
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
    for clip in &mut arrangement.midi_clips {
        if clip.id.trim().is_empty()
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
        for note in &clip.notes {
            if note.id.trim().is_empty()
                || note.note > 127
                || note.velocity > 127
                || note.channel == 0
                || note.channel > 16
                || note.duration_ticks == 0
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid note.",
                    clip.name
                ));
            }
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

    #[test]
    fn workspace_has_exactly_four_variants() {
        let all = [
            Workspace::Home,
            Workspace::Play,
            Workspace::Design,
            Workspace::Arrange,
        ];
        assert_eq!(all.len(), 4);
        assert!(matches!(CreativeSession::new(0).workspace, Workspace::Home));
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
            id: "instrument".into(),
            name: "Instrument".into(),
            kind: TrackKind::Instrument,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: MonitoringState::Off,
            rack: empty_track_rack(),
        });
        let error = arrangement
            .add_audio_clip(clip("instrument", mint_asset_id()), |_| true)
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
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
            tracks: vec![
                Track::audio("main".into(), "Main".into()),
                Track {
                    id: "extra".into(),
                    name: "Extra".into(),
                    kind: TrackKind::Audio,
                    gain_db: 0.0,
                    pan: 0.0,
                    muted: false,
                    solo: false,
                    armed: false,
                    monitoring: MonitoringState::Off,
                    rack: empty_track_rack(),
                },
            ],
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
            markers: Vec::new(),
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
            start_tick: TimelineTick(5_760),
            duration_ticks: 960,
            notes: Vec::new(),
            muted: false,
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
}
