//! Arrangement, Track, Clip, MIDI, and automation domain models.

use crate::DomainError;
use crate::domain::asset::AssetId;
use crate::domain::music::HarmonyEvent;
use crate::domain::recording::*;
use crate::domain::timeline::{
    FrameDuration, FrameRange, ProjectTimebase, TIMELINE_PPQ, TimelineLoopRange,
    TimelinePunchRange, TimelineTick,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

pub(crate) const MAX_MIDI_NOTES_PER_CLIP: usize = 200_000;

mod audio_clip;
mod automation;
mod midi_clip;
mod track;

pub use audio_clip::{AudioClip, AudioClipMove, AudioClipPatch, FadeShape};
pub use automation::{AutomationLane, AutomationParameter, AutomationPoint};
pub use midi_clip::{MidiClip, MidiClipMove, MidiClipPatch, MidiEvent, MidiEventKind, MidiNote};
pub use track::{AudioInputRoute, MidiInputRoute, MonitoringState, Track, TrackKind, TrackPatch};

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
    pub regions: Vec<TimelineRegion>,
    #[serde(default)]
    pub harmony_events: Vec<HarmonyEvent>,
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

/// A named half-open range on the project timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRegion {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub end_tick: TimelineTick,
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

    /// Validates a named timeline range without imposing a section taxonomy.
    ///
    /// # Errors
    ///
    /// Returns an error when the region has an empty id or name, or when its
    /// end is not after its start.
    pub fn validate_region(&self, region: &TimelineRegion) -> Result<(), DomainError> {
        if region.id.trim().is_empty()
            || region.name.trim().is_empty()
            || region.end_tick <= region.start_tick
        {
            return Err(DomainError::InvalidTimelineRegion(
                "regions require an id, a name, and a positive range".into(),
            ));
        }
        Ok(())
    }

    /// Adds a named range to the arrangement.
    ///
    /// # Errors
    ///
    /// Returns an error when the region is invalid or its id is already in
    /// use.
    pub fn add_region(&mut self, region: TimelineRegion) -> Result<(), DomainError> {
        self.validate_region(&region)?;
        if self.regions.iter().any(|existing| existing.id == region.id) {
            return Err(DomainError::InvalidTimelineRegion(
                "region ids must be unique".into(),
            ));
        }
        self.regions.push(region);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Updates a named range while preserving its identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the region is missing or the updated region is
    /// invalid.
    pub fn update_region(
        &mut self,
        region_id: &str,
        name: Option<String>,
        start_tick: Option<TimelineTick>,
        end_tick: Option<TimelineTick>,
    ) -> Result<(), DomainError> {
        let index = self
            .regions
            .iter()
            .position(|region| region.id == region_id)
            .ok_or_else(|| {
                DomainError::InvalidTimelineRegion(format!(
                    "region '{region_id}' is not registered"
                ))
            })?;
        let mut region = self.regions[index].clone();
        if let Some(name) = name {
            region.name = name;
        }
        if let Some(start_tick) = start_tick {
            region.start_tick = start_tick;
        }
        if let Some(end_tick) = end_tick {
            region.end_tick = end_tick;
        }
        self.validate_region(&region)?;
        self.regions[index] = region;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Removes a named range from the arrangement.
    ///
    /// # Errors
    ///
    /// Returns an error when the region is not registered.
    pub fn remove_region(&mut self, region_id: &str) -> Result<(), DomainError> {
        let before = self.regions.len();
        self.regions.retain(|region| region.id != region_id);
        if self.regions.len() == before {
            return Err(DomainError::InvalidTimelineRegion(format!(
                "region '{region_id}' is not registered"
            )));
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Adds harmony events as one arrangement edit.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when an event is invalid or an
    /// id is duplicated.
    pub fn add_harmony_events(&mut self, events: Vec<HarmonyEvent>) -> Result<(), DomainError> {
        if events.is_empty() {
            return Err(DomainError::InvalidHarmony(
                "at least one harmony event is required".into(),
            ));
        }
        let mut ids = self
            .harmony_events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<HashSet<_>>();
        for event in &events {
            event.validate()?;
            if !ids.insert(event.id.as_str()) {
                return Err(DomainError::InvalidHarmony(
                    "harmony event ids must be unique".into(),
                ));
            }
        }
        self.harmony_events.extend(events);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Replaces one harmony event while preserving its identity.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when the event is missing or
    /// invalid.
    pub fn update_harmony_event(
        &mut self,
        event_id: &str,
        event: HarmonyEvent,
    ) -> Result<(), DomainError> {
        let index = self
            .harmony_events
            .iter()
            .position(|existing| existing.id == event_id)
            .ok_or_else(|| {
                DomainError::InvalidHarmony(format!("harmony event '{event_id}' is not registered"))
            })?;
        if event.id != event_id {
            return Err(DomainError::InvalidHarmony(
                "harmony event identity cannot be changed".into(),
            ));
        }
        event.validate()?;
        self.harmony_events[index] = event;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Removes one or more harmony events atomically.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidHarmony`] when the id list is empty,
    /// contains duplicates, or references a missing event.
    pub fn remove_harmony_events(&mut self, event_ids: Vec<String>) -> Result<(), DomainError> {
        if event_ids.is_empty() {
            return Err(DomainError::InvalidHarmony(
                "at least one harmony event id is required".into(),
            ));
        }
        let ids = event_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        if ids.len() != event_ids.len()
            || event_ids.iter().any(|id| id.trim().is_empty())
            || event_ids
                .iter()
                .any(|id| !self.harmony_events.iter().any(|event| event.id == *id))
        {
            return Err(DomainError::InvalidHarmony(
                "harmony event ids must be unique and registered".into(),
            ));
        }
        self.harmony_events
            .retain(|event| !ids.contains(event.id.as_str()));
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
        if clip.notes.len() > MAX_MIDI_NOTES_PER_CLIP || clip.events.len() > 200_000 {
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

    /// Duplicates selected MIDI notes within one Clip.
    ///
    /// # Errors
    ///
    /// Returns an error when the selection is empty, contains duplicate or
    /// missing Note IDs, or the Clip is unknown.
    pub fn duplicate_midi_notes(
        &mut self,
        clip_id: &str,
        note_ids: &[String],
        offset_ticks: u64,
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
            .ok_or_else(|| DomainError::InvalidClip(format!("MIDI clip '{clip_id}' not found.")))?;
        let mut selected_ids = std::collections::HashSet::with_capacity(note_ids.len());
        if note_ids
            .iter()
            .any(|note_id| !selected_ids.insert(note_id.as_str()))
        {
            return Err(DomainError::InvalidClip(
                "duplicate midi note ids were selected.".into(),
            ));
        }
        if note_ids
            .iter()
            .any(|note_id| !clip.notes.iter().any(|note| note.id == *note_id))
        {
            return Err(DomainError::InvalidClip(
                "one or more midi notes were not found.".into(),
            ));
        }
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

    /// Removes every note from a MIDI Clip while preserving its arrangement
    /// properties and non-note MIDI events.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidClip`] when the Clip is unknown.
    pub fn clear_midi_notes(&mut self, clip_id: &str) -> Result<(), DomainError> {
        let clip = self
            .midi_clips
            .iter_mut()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| DomainError::InvalidClip(format!("midi clip '{clip_id}' not found.")))?;
        clip.notes.clear();
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

impl Arrangement {
    /// Validates and normalizes the arrangement and its owned concepts.
    pub(crate) fn validate_and_normalize(arrangement: &mut Arrangement) -> Result<(), String> {
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
            if !unique_track_ids.insert(track.id.clone()) {
                return Err("Tracks require non-empty ids and names.".into());
            }
            track.validate_and_normalize()?;
        }
        let audio_clips = std::mem::take(&mut arrangement.audio_clips);
        let mut normalized_clips = Vec::with_capacity(audio_clips.len());
        let mut audio_clip_ids = std::collections::HashSet::new();
        for mut clip in audio_clips {
            if !audio_clip_ids.insert(clip.id.clone()) {
                return Err("Audio clips require ids, names, tracks and asset ids.".into());
            }
            clip.validate_and_normalize()?;
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
                regions: Vec::new(),
                harmony_events: arrangement.harmony_events.clone(),
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
            if !midi_clip_ids.insert(clip.id.clone()) {
                return Err("MIDI clips require non-empty ids and names.".into());
            }
            let track = arrangement
                .tracks
                .iter()
                .find(|track| track.id == clip.track_id);
            clip.validate_and_normalize(track)?;
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
                || !lane_ids.insert(lane.id.clone())
                || !track_ids.contains(lane.track_id.as_str())
                || !lane_keys.insert((lane.track_id.clone(), lane.parameter))
            {
                return Err("Automation Lanes contain invalid or duplicate references.".into());
            }
            lane.validate_and_normalize()?;
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
        if arrangement.regions.len() > 256 {
            return Err("an arrangement cannot contain more than 256 timeline regions".into());
        }
        arrangement
            .regions
            .sort_by_key(|region| (region.start_tick, region.end_tick));
        let mut region_ids = std::collections::HashSet::new();
        for region in &mut arrangement.regions {
            if region.id.trim().is_empty()
                || !region_ids.insert(region.id.as_str())
                || region.name.trim().is_empty()
                || region.end_tick <= region.start_tick
            {
                return Err(
                    "timeline regions require unique ids, names, and positive ranges".into(),
                );
            }
            region.name = region.name.trim().chars().take(80).collect();
        }
        if arrangement.harmony_events.len() > 16_384 {
            return Err("an arrangement cannot contain more than 16,384 harmony events".into());
        }
        arrangement
            .harmony_events
            .sort_by_key(|event| (event.start_tick, event.end_tick, event.id.clone()));
        let mut harmony_event_ids = std::collections::HashSet::new();
        for event in &arrangement.harmony_events {
            if !harmony_event_ids.insert(event.id.as_str()) || event.validate().is_err() {
                return Err(
                    "harmony events require unique ids, positive ranges, and valid chords".into(),
                );
            }
        }
        if arrangement.recording_sessions.len() > 256
            || arrangement.recording_passes.len() > 4096
            || arrangement.takes.len() > 16_384
        {
            return Err(
                "An arrangement contains too many recording sessions, passes, or takes.".into(),
            );
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
                    "Recording Takes contain invalid session, pass, track, or range references."
                        .into(),
                );
            }
        }
        Ok(())
    }
}
