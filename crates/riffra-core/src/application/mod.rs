//! User-intent application operations over the canonical production state.

use crate::app::AppCore;
use crate::asset::AssetId;
use crate::errors::ApplicationError;
use crate::ports::SessionStorage;
use crate::rack::RackDevice;
use crate::session::{
    AiChangeSet, AiPermission, Arrangement, AudioClip, AudioClipMove, AudioClipPatch,
    AudioInputRoute, AudioTakeVariant, AutomationLane, AutomationParameter, AutomationPoint,
    CreativeSession, FrameRange, Marker, MidiClip, MidiClipMove, MidiClipPatch, MidiInputRoute,
    MidiNote, ProjectTimebase, SamplePad, TakeAudioSource, TimelineTick, Track, TrackKind,
    TrackPatch,
};
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

    /// Returns the canonical production snapshot.
    pub fn get_session(&self) -> Result<CreativeSession, ApplicationError> {
        Ok(self.core.snapshot()?.session)
    }

    /// Imports a complete project through the canonical commit boundary.
    pub fn import_project(
        &self,
        session: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit_candidate(self.storage, session)
    }

    /// Restores a complete project generation through the canonical commit
    /// boundary.
    pub fn restore_project(
        &self,
        session: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit_candidate(self.storage, session)
    }

    /// Merges the production fields owned by a completed recording onto the
    /// latest canonical snapshot.
    pub fn commit_recording(
        &self,
        base: &CreativeSession,
        candidate: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core
            .commit_merged(self.storage, base, candidate, merge_recording_session)
    }

    /// Lists Tracks from the canonical production state.
    pub fn list_tracks(&self) -> Result<Vec<Track>, ApplicationError> {
        Ok(self.get_session()?.arrangement.tracks)
    }

    /// Lists all Timeline Clips from the canonical production state.
    pub fn list_audio_clips(&self) -> Result<Vec<AudioClip>, ApplicationError> {
        Ok(self.get_session()?.arrangement.audio_clips)
    }

    /// Lists all MIDI Clips from the canonical production state.
    pub fn list_midi_clips(&self) -> Result<Vec<MidiClip>, ApplicationError> {
        Ok(self.get_session()?.arrangement.midi_clips)
    }

    /// Adds a Track to the Arrangement.
    pub fn add_track(
        &self,
        name: impl Into<String>,
        kind: TrackKind,
    ) -> Result<CreativeSession, ApplicationError> {
        let name = name.into();
        self.commit_arrangement(|arrangement| {
            let id = next_id("track");
            arrangement.tracks.push(match kind {
                TrackKind::Audio => Track::audio(id, name),
                TrackKind::Instrument => Track::instrument(id, name),
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes a Track and its owned Timeline objects.
    pub fn remove_track(&self, track_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.remove_track(track_id).map_err(Into::into)
        })
    }

    /// Duplicates a Track and its owned timeline and automation objects.
    pub fn duplicate_track(&self, track_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let source_index = arrangement
                .tracks
                .iter()
                .position(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let operation_id = next_id("duplicate");
            let mut duplicate = arrangement.tracks[source_index].clone();
            duplicate.id = format!("track:{operation_id}");
            duplicate.name = format!("{} copy", duplicate.name);
            let duplicate_id = duplicate.id.clone();
            arrangement
                .tracks
                .insert(source_index.saturating_add(1), duplicate);

            let audio_clips = arrangement
                .audio_clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut clip)| {
                    clip.id = format!("clip:{operation_id}:{index}");
                    clip.track_id = duplicate_id.clone();
                    clip
                })
                .collect::<Vec<_>>();
            arrangement.audio_clips.extend(audio_clips);

            let midi_clips = arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut clip)| {
                    clip.id = format!("midi-clip:{operation_id}:{index}");
                    clip.track_id = duplicate_id.clone();
                    clip
                })
                .collect::<Vec<_>>();
            arrangement.midi_clips.extend(midi_clips);

            let automation_lanes = arrangement
                .automation_lanes
                .iter()
                .filter(|lane| lane.track_id == track_id)
                .cloned()
                .enumerate()
                .map(|(index, mut lane)| {
                    lane.id = format!("automation:{duplicate_id}:{index}");
                    lane.track_id = duplicate_id.clone();
                    for (point_index, point) in lane.points.iter_mut().enumerate() {
                        point.id = format!("automation-point:{operation_id}:{index}:{point_index}");
                    }
                    lane
                })
                .collect::<Vec<_>>();
            arrangement.automation_lanes.extend(automation_lanes);
            Ok(())
        })
    }

    /// Applies a validated Track mix and routing patch.
    pub fn update_track(
        &self,
        track_id: &str,
        patch: TrackPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if let Some(name) = patch.name {
                let name = name.trim().chars().take(80).collect::<String>();
                if name.is_empty() {
                    return Err(crate::DomainError::InvalidClip(
                        "track name must not be empty".into(),
                    )
                    .into());
                }
                track.name = name;
            }
            if let Some(gain_db) = patch.gain_db {
                track.gain_db = if gain_db.is_finite() {
                    gain_db.clamp(-90.0, 24.0)
                } else {
                    0.0
                };
            }
            if let Some(pan) = patch.pan {
                track.pan = if pan.is_finite() {
                    pan.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
            if let Some(muted) = patch.muted {
                track.muted = muted;
            }
            if let Some(solo) = patch.solo {
                track.solo = solo;
            }
            if let Some(armed) = patch.armed {
                track.armed = armed;
            }
            if let Some(monitoring) = patch.monitoring {
                track.monitoring = monitoring;
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Routes or clears a physical audio input on an Audio Track.
    pub fn set_track_audio_input(
        &self,
        track_id: &str,
        channel_index: Option<u32>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Audio {
                return Err(ApplicationError::InvalidCommand(
                    "only audio tracks can route a physical audio input".into(),
                ));
            }
            track.audio_input =
                channel_index.map(|channel_index| AudioInputRoute { channel_index });
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Routes or clears a MIDI input on an Instrument Track.
    pub fn set_track_midi_input(
        &self,
        track_id: &str,
        route: MidiInputRoute,
    ) -> Result<CreativeSession, ApplicationError> {
        if route
            .channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(ApplicationError::InvalidCommand(
                "midi channel must be between 1 and 16".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can route MIDI input".into(),
                ));
            }
            track.midi_input = route;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Assigns or clears an Instrument Track's instrument device.
    pub fn set_track_instrument(
        &self,
        track_id: &str,
        instrument: Option<RackDevice>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can host an instrument".into(),
                ));
            }
            track.instrument = instrument;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Commits an instrument assignment only if the prepared Core snapshot is
    /// still current.
    pub fn set_track_instrument_at_sequence(
        &self,
        track_id: &str,
        instrument: Option<RackDevice>,
        expected_sequence: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core
            .commit_at_sequence(self.storage, expected_sequence, |session| {
                let track = session
                    .arrangement
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
                if track.kind != TrackKind::Instrument {
                    return Err(ApplicationError::InvalidCommand(
                        "only instrument tracks can host an instrument".into(),
                    ));
                }
                track.instrument = instrument;
                session.arrangement.revision = session.arrangement.revision.saturating_add(1);
                Ok(())
            })
    }

    /// Appends an effect device to a Track rack.
    pub fn add_track_effect(
        &self,
        track_id: &str,
        device: RackDevice,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            track.rack.devices.push(device);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Commits an effect insertion only if the prepared Core snapshot is still
    /// current.
    pub fn add_track_effect_at_sequence(
        &self,
        track_id: &str,
        device: RackDevice,
        expected_sequence: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core
            .commit_at_sequence(self.storage, expected_sequence, |session| {
                let track = session
                    .arrangement
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
                track.rack.devices.push(device);
                session.arrangement.revision = session.arrangement.revision.saturating_add(1);
                Ok(())
            })
    }

    /// Removes one effect device from a Track rack.
    pub fn remove_track_effect(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let before = track.rack.devices.len();
            track.rack.devices.retain(|device| device.id != device_id);
            if before == track.rack.devices.len() {
                return Err(ApplicationError::InvalidCommand(
                    "track effect is not registered".into(),
                ));
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Reorders every effect in one Track rack.
    pub fn reorder_track_effects(
        &self,
        track_id: &str,
        ordered_device_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let unique_ids = ordered_device_ids
                .iter()
                .collect::<std::collections::HashSet<_>>();
            if ordered_device_ids.len() != track.rack.devices.len()
                || unique_ids.len() != ordered_device_ids.len()
                || ordered_device_ids
                    .iter()
                    .any(|id| !track.rack.devices.iter().any(|device| &device.id == id))
            {
                return Err(ApplicationError::InvalidCommand(
                    "effect order must contain every track effect exactly once".into(),
                ));
            }
            let mut reordered = Vec::with_capacity(track.rack.devices.len());
            for id in ordered_device_ids {
                let index = track
                    .rack
                    .devices
                    .iter()
                    .position(|device| device.id == id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand("track effect is not registered".into())
                    })?;
                reordered.push(track.rack.devices.remove(index));
            }
            track.rack.devices = reordered;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one device's bypass state.
    pub fn set_track_device_bypassed(
        &self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let device = find_track_device_mut(session, track_id, device_id)?;
            device.bypassed = bypassed;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one normalized device parameter.
    pub fn set_track_device_parameter(
        &self,
        track_id: &str,
        device_id: &str,
        parameter_index: usize,
        value: f32,
    ) -> Result<CreativeSession, ApplicationError> {
        if !value.is_finite() {
            return Err(ApplicationError::InvalidCommand(
                "track device parameter value must be finite".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let device = find_track_device_mut(session, track_id, device_id)?;
            if device.parameter_values.len() <= parameter_index {
                device.parameter_values.resize(parameter_index + 1, 0.0);
            }
            device.parameter_values[parameter_index] = value.clamp(0.0, 1.0);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces one Track automation lane with sorted points.
    pub fn set_track_automation(
        &self,
        track_id: &str,
        parameter: AutomationParameter,
        mut points: Vec<AutomationPoint>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if !session
                .arrangement
                .tracks
                .iter()
                .any(|track| track.id == track_id)
            {
                return Err(crate::DomainError::UnknownTrack(track_id.to_owned()).into());
            }
            points.sort_by_key(|point| point.tick);
            session
                .arrangement
                .automation_lanes
                .retain(|lane| lane.track_id != track_id || lane.parameter != parameter);
            if !points.is_empty() {
                let parameter_name = match parameter {
                    AutomationParameter::Volume => "volume",
                    AutomationParameter::Pan => "pan",
                };
                session.arrangement.automation_lanes.push(AutomationLane {
                    id: format!("automation:{track_id}:{parameter_name}"),
                    track_id: track_id.to_owned(),
                    parameter,
                    points,
                });
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Updates session-wide production settings.
    pub fn update_session_settings(
        &self,
        patch: SessionSettingsPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if let Some(project_name) = patch.project_name {
                session.project_name = project_name
                    .map(|value| value.trim().chars().take(160).collect::<String>())
                    .filter(|value| !value.is_empty());
            }
            if let Some(master_db) = patch.master_db {
                if !master_db.is_finite() {
                    return Err(ApplicationError::InvalidCommand(
                        "master gain must be finite".into(),
                    ));
                }
                session.settings.master_db = master_db.clamp(-90.0, 0.0);
            }
            if let Some(loop_enabled) = patch.loop_enabled {
                session.settings.loop_enabled = loop_enabled;
            }
            if let Some(count_in_beats) = patch.count_in_beats {
                session.settings.count_in_beats = count_in_beats.min(8);
            }
            if let Some(metronome_enabled) = patch.metronome_enabled {
                session.settings.metronome_enabled = metronome_enabled;
            }
            if let Some(note) = patch.note {
                session.settings.note = note.chars().take(16_384).collect();
            }
            if let Some(permission) = patch.ai_permission {
                session.settings.ai_permission = permission;
            }
            if let Some(context) = patch.ai_context {
                session.settings.ai_context = context;
            }
            Ok(())
        })
    }

    /// Applies an allowed AI gain suggestion and records its reversible change set.
    pub fn apply_ai_suggestion(
        &self,
        clip_id: &str,
        proposed_gain_db: f64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if session.settings.ai_permission != AiPermission::Apply {
                return Err(ApplicationError::InvalidCommand(
                    "ai suggestion application requires apply permission".into(),
                ));
            }
            let clip = session
                .arrangement
                .audio_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "audio clip '{clip_id}' is not registered"
                    ))
                })?;
            let current_gain_db = clip.gain_db;
            clip.gain_db = if proposed_gain_db.is_finite() {
                proposed_gain_db.clamp(-90.0, 24.0)
            } else {
                0.0
            };
            let applied_gain_db = clip.gain_db;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            let created_at_ms = now_ms();
            session.settings.ai_history.push(AiChangeSet {
                id: format!("ai:{created_at_ms}"),
                created_at_ms,
                permission: session.settings.ai_permission,
                target: clip_id.to_owned(),
                current_gain_db,
                proposed_gain_db: applied_gain_db,
                reason: "Match the selected reference RMS without changing the source WAV.".into(),
                expected_effect:
                    "A closer perceived level while clip position and source remain unchanged."
                        .into(),
                risk: "Low · reversible".into(),
                context: session.settings.ai_context.clone(),
                applied: true,
            });
            if session.settings.ai_history.len() > 128 {
                let excess = session.settings.ai_history.len() - 128;
                session.settings.ai_history.drain(..excess);
            }
            Ok(())
        })
    }

    /// Adds a validated Sample Pad to the canonical performance mapping.
    pub fn add_sample_pad(&self, pad: SamplePad) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            validate_sample_pad_add(session, &pad)?;
            session.play_state.sample_instrument.pads.push(pad);
            Ok(())
        })
    }

    /// Builds the post-add Sample Pad mapping without committing it.
    pub fn prepare_sample_pad_add(
        &self,
        pad: &SamplePad,
    ) -> Result<Vec<SamplePad>, ApplicationError> {
        let session = self.get_session()?;
        validate_sample_pad_add(&session, pad)?;
        let mut pads = session.play_state.sample_instrument.pads;
        pads.push(pad.clone());
        Ok(pads)
    }

    /// Updates one Sample Pad while preserving its valid slice range.
    pub fn update_sample_pad(
        &self,
        pad_id: &str,
        patch: SamplePadPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let pad = find_sample_pad_mut(session, pad_id)?;
            apply_sample_pad_patch(pad, &patch);
            Ok(())
        })
    }

    /// Builds the post-edit pad mapping without committing it.
    ///
    /// Hosts use this read-only preparation when an external runtime must be
    /// configured before the canonical commit can be accepted.
    pub fn prepare_sample_pad_update(
        &self,
        pad_id: &str,
        patch: &SamplePadPatch,
    ) -> Result<Vec<SamplePad>, ApplicationError> {
        let mut pads = self.get_session()?.play_state.sample_instrument.pads;
        let pad = pads
            .iter_mut()
            .find(|pad| pad.id == pad_id)
            .ok_or_else(|| {
                ApplicationError::InvalidCommand(format!("sample pad is not registered: {pad_id}"))
            })?;
        apply_sample_pad_patch(pad, patch);
        Ok(pads)
    }

    /// Removes one Sample Pad from the canonical performance mapping.
    pub fn remove_sample_pad(&self, pad_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let pads = &mut session.play_state.sample_instrument.pads;
            let before = pads.len();
            pads.retain(|pad| pad.id != pad_id);
            if pads.len() == before {
                return Err(ApplicationError::InvalidCommand(format!(
                    "sample pad is not registered: {pad_id}"
                )));
            }
            Ok(())
        })
    }

    /// Builds the post-removal Sample Pad mapping without committing it.
    pub fn prepare_sample_pad_removal(
        &self,
        pad_id: &str,
    ) -> Result<Vec<SamplePad>, ApplicationError> {
        let mut pads = self.get_session()?.play_state.sample_instrument.pads;
        let before = pads.len();
        pads.retain(|pad| pad.id != pad_id);
        if pads.len() == before {
            return Err(ApplicationError::InvalidCommand(format!(
                "sample pad is not registered: {pad_id}"
            )));
        }
        Ok(pads)
    }

    /// Selects the raw or processed source for an Audio Clip backed by a Take.
    pub fn set_audio_clip_take_variant(
        &self,
        clip_id: &str,
        variant: AudioTakeVariant,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            apply_audio_clip_take_variant(session, clip_id, variant)
                .map_err(ApplicationError::InvalidCommand)
        })
    }

    /// Activates a recorded Take in its recording Session slot.
    ///
    /// MIDI take decoding remains host-specific; a decoded clip is supplied
    /// when the Take is MIDI-backed. Audio source selection and all canonical
    /// slot/clip updates stay in Core.
    pub fn activate_take(
        &self,
        session_id: &str,
        take_id: &str,
        midi_clip: Option<MidiClip>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let target_take = arrangement
                .takes
                .iter()
                .find(|take| take.session_id == session_id && take.id == take_id)
                .cloned()
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "recording take is not registered: {take_id}"
                    ))
                })?;
            let timeline_clip_id = {
                let slot = arrangement
                    .recording_sessions
                    .iter_mut()
                    .find(|recording| recording.id == session_id)
                    .and_then(|recording| {
                        recording
                            .track_slots
                            .iter_mut()
                            .find(|slot| slot.track_id == target_take.track_id)
                    })
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(format!(
                            "recording session has no track slot for {}",
                            target_take.track_id
                        ))
                    })?;
                slot.active_take_id = take_id.to_owned();
                slot.timeline_clip_id.clone()
            };
            if let Some(clip) = arrangement
                .audio_clips
                .iter_mut()
                .find(|clip| clip.id == timeline_clip_id)
            {
                let source = target_take
                    .preferred_audio_source(clip.take_variant)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "the selected take has no audio asset".into(),
                        )
                    })?;
                apply_audio_source_to_clip(clip, &source);
                clip.recording_take_id = Some(take_id.to_owned());
            } else if target_take.midi_asset_id.is_some() {
                let source = midi_clip.ok_or_else(|| {
                    ApplicationError::InvalidCommand(
                        "the selected MIDI take has no decoded clip".into(),
                    )
                })?;
                let clip = arrangement
                    .midi_clips
                    .iter_mut()
                    .find(|clip| clip.id == timeline_clip_id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take slot has no MIDI clip".into(),
                        )
                    })?;
                clip.asset_id = target_take.midi_asset_id.clone();
                clip.notes = source.notes;
                clip.events = source.events;
                clip.duration_ticks = target_take.duration_ticks;
                clip.recording_take_id = Some(take_id.to_owned());
            } else {
                return Err(ApplicationError::InvalidCommand(
                    "recording take has no timeline source".into(),
                ));
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Places a recorded Take as a new timeline clip.
    ///
    /// The host may provide a decoded MIDI clip because reading the source
    /// asset is an infrastructure concern. Core assigns the new clip identity
    /// and owns the arrangement mutation.
    pub fn place_take_as_separate_clip(
        &self,
        take_id: &str,
        mut midi_clip: Option<MidiClip>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let take = arrangement
                .takes
                .iter()
                .find(|take| take.id == take_id)
                .cloned()
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "recording take is not registered: {take_id}"
                    ))
                })?;
            if let Some(source) = arrangement
                .audio_clips
                .iter()
                .find(|clip| clip.recording_take_id.as_deref() == Some(take_id))
                .cloned()
            {
                let mut clip = source;
                clip.id = next_id("clip:take-place");
                clip.muted = false;
                arrangement.audio_clips.push(clip);
            } else if take.raw_audio.is_some() || take.processed_audio.is_some() {
                let slot_clip_id = arrangement
                    .recording_sessions
                    .iter()
                    .find(|recording| recording.id == take.session_id)
                    .and_then(|recording| {
                        recording
                            .track_slots
                            .iter()
                            .find(|slot| slot.track_id == take.track_id)
                    })
                    .map(|slot| slot.timeline_clip_id.clone())
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take track slot is unavailable".into(),
                        )
                    })?;
                let mut clip = arrangement
                    .audio_clips
                    .iter()
                    .find(|clip| clip.id == slot_clip_id)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take slot has no audio clip".into(),
                        )
                    })?;
                clip.id = next_id("clip:take-place");
                clip.start_tick = take.start_tick;
                let source = take
                    .preferred_audio_source(clip.take_variant)
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand(
                            "recording take has no usable audio asset".into(),
                        )
                    })?;
                apply_audio_source_to_clip(&mut clip, &source);
                clip.recording_take_id = Some(take.id);
                clip.muted = false;
                arrangement.audio_clips.push(clip);
            } else if take.midi_asset_id.is_some() {
                let mut clip = midi_clip.take().ok_or_else(|| {
                    ApplicationError::InvalidCommand(
                        "the selected MIDI take has no decoded clip".into(),
                    )
                })?;
                clip.id = next_id("midi-clip:take-place");
                clip.recording_take_id = Some(take.id);
                arrangement.midi_clips.push(clip);
            } else {
                return Err(ApplicationError::InvalidCommand(format!(
                    "recording take has no timeline clip: {take_id}"
                )));
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Repoints every production reference from one Asset id to another.
    pub fn replace_asset_references(
        &self,
        old_asset_id: &AssetId,
        new_asset_id: AssetId,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let mut arrangement_changed = false;
            for clip in &mut session.arrangement.audio_clips {
                if clip.asset_id == *old_asset_id {
                    clip.asset_id = new_asset_id.clone();
                    arrangement_changed = true;
                }
            }
            let mut play_state_changed = false;
            for pad in &mut session.play_state.sample_instrument.pads {
                if pad.asset_id == *old_asset_id {
                    pad.asset_id = new_asset_id.clone();
                    play_state_changed = true;
                }
            }
            if !arrangement_changed && !play_state_changed {
                return Err(ApplicationError::InvalidCommand(format!(
                    "asset is not referenced by the project: {old_asset_id}"
                )));
            }
            if arrangement_changed {
                session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            }
            Ok(())
        })
    }

    /// Persists a complete state snapshot emitted by a native Plugin Editor.
    pub fn persist_track_plugin_state(
        &self,
        track_id: &str,
        device_id: &str,
        parameter_values: Vec<f32>,
        state_data: Option<String>,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        if parameter_values.iter().any(|value| !value.is_finite()) {
            return Err(ApplicationError::InvalidCommand(
                "track plugin editor returned a non-finite parameter value".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let device = find_track_device_mut(session, track_id, device_id)?;
            device.parameter_values = parameter_values;
            device.state_data = state_data.filter(|value| !value.is_empty());
            device.bypassed = bypassed;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Persists one parameter emitted by a native Plugin Editor.
    pub fn persist_track_plugin_parameter(
        &self,
        track_id: &str,
        device_id: &str,
        parameter_index: usize,
        value: f32,
    ) -> Result<CreativeSession, ApplicationError> {
        if !value.is_finite() {
            return Err(ApplicationError::InvalidCommand(
                "track plugin editor returned a non-finite parameter value".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let device = find_track_device_mut(session, track_id, device_id)?;
            if device.parameter_values.len() <= parameter_index {
                device.parameter_values.resize(parameter_index + 1, 0.0);
            }
            device.parameter_values[parameter_index] = value.clamp(0.0, 1.0);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Marks a Track Plugin as a disabled placeholder after it was found missing.
    pub fn disable_missing_plugin(
        &self,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let device = session
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
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "track device is not registered: {device_id}"
                    ))
                })?;
            if device.disabled_placeholder {
                return Err(ApplicationError::InvalidCommand(format!(
                    "track device is already disabled: {device_id}"
                )));
            }
            device.disabled_placeholder = true;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a Track Plugin while preserving its rack slot identity.
    pub fn replace_track_plugin(
        &self,
        device_id: &str,
        device: RackDevice,
    ) -> Result<CreativeSession, ApplicationError> {
        if device.id != device_id {
            return Err(ApplicationError::InvalidCommand(
                "replacement track device id must match the existing device".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let current = find_any_track_device_mut(session, device_id)?;
            *current = device;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a plugin only if the prepared Core snapshot is still current.
    pub fn replace_track_plugin_at_sequence(
        &self,
        device_id: &str,
        device: RackDevice,
        expected_sequence: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        if device.id != device_id {
            return Err(ApplicationError::InvalidCommand(
                "replacement track device id must match the existing device".into(),
            ));
        }
        self.core
            .commit_at_sequence(self.storage, expected_sequence, |session| {
                let current = find_any_track_device_mut(session, device_id)?;
                *current = device;
                session.arrangement.revision = session.arrangement.revision.saturating_add(1);
                Ok(())
            })
    }

    /// Reorders a Track without changing its owned timeline objects.
    pub fn reorder_track(
        &self,
        track_id: &str,
        target_index: usize,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .reorder_track(track_id, target_index)
                .map_err(Into::into)
        })
    }

    /// Adds an already-validated Audio Clip after checking the host asset index.
    pub fn add_audio_clip(
        &self,
        clip: AudioClip,
        asset_exists: impl Fn(&crate::asset::AssetId) -> bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .add_audio_clip(clip, asset_exists)
                .map_err(Into::into)
        })
    }

    /// Adds an Audio Clip and creates its Audio Track when the caller supplied
    /// a new track id.
    pub fn add_audio_clip_with_track(
        &self,
        clip: AudioClip,
        track_name: Option<String>,
        asset_exists: impl Fn(&crate::asset::AssetId) -> bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            if let Some(track_name) = track_name {
                arrangement
                    .tracks
                    .push(Track::audio(clip.track_id.clone(), track_name));
            }
            arrangement
                .add_audio_clip(clip, asset_exists)
                .map_err(Into::into)
        })
    }

    /// Adds an already-validated MIDI Clip.
    pub fn add_midi_clip(&self, clip: MidiClip) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| arrangement.add_midi_clip(clip).map_err(Into::into))
    }

    /// Adds a MIDI Clip and creates its Instrument Track when the caller
    /// supplied a new track id.
    pub fn add_midi_clip_with_track(
        &self,
        clip: MidiClip,
        track_name: Option<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            if let Some(track_name) = track_name {
                arrangement
                    .tracks
                    .push(Track::instrument(clip.track_id.clone(), track_name));
            }
            arrangement.add_midi_clip(clip).map_err(Into::into)
        })
    }

    /// Replaces the project timebase through the canonical domain operation.
    pub fn update_timebase(
        &self,
        timebase: ProjectTimebase,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.update_timebase(timebase).map_err(Into::into)
        })
    }

    /// Updates the transport loop range through the canonical domain operation.
    pub fn update_loop_range(
        &self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_loop_range(enabled, start_tick, end_tick)
                .map_err(Into::into)
        })
    }

    /// Updates the transport punch range through the canonical domain operation.
    pub fn update_punch_range(
        &self,
        enabled: bool,
        start_tick: TimelineTick,
        end_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_punch_range(enabled, start_tick, end_tick)
                .map_err(Into::into)
        })
    }

    /// Applies a validated Audio Clip patch and commits the result.
    pub fn update_audio_clip(
        &self,
        clip_id: &str,
        patch: AudioClipPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_audio_clip(clip_id, patch)
                .map_err(Into::into)
        })
    }

    /// Applies a validated MIDI Clip patch and commits the result.
    pub fn update_midi_clip(
        &self,
        clip_id: &str,
        patch: MidiClipPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .update_midi_clip(clip_id, patch)
                .map_err(Into::into)
        })
    }

    /// Moves Audio Clips as one atomic arrangement edit.
    pub fn move_audio_clips(
        &self,
        moves: Vec<AudioClipMove>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.move_audio_clips(moves).map_err(Into::into)
        })
    }

    /// Moves MIDI Clips as one atomic arrangement edit.
    pub fn move_midi_clips(
        &self,
        moves: Vec<MidiClipMove>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement.move_midi_clips(moves).map_err(Into::into)
        })
    }

    /// Removes selected Audio and MIDI Clips in one atomic edit.
    pub fn remove_timeline_clips(
        &self,
        audio_clip_ids: Vec<String>,
        midi_clip_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .remove_timeline_clips(&audio_clip_ids, &midi_clip_ids)
                .map_err(Into::into)
        })
    }

    /// Duplicates selected Clips at one timeline anchor with host-provided ids.
    pub fn paste_timeline_clips(
        &self,
        audio_clip_ids: Vec<String>,
        midi_clip_ids: Vec<String>,
        audio_ids: Vec<String>,
        midi_ids: Vec<String>,
        start_tick: TimelineTick,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .paste_timeline_clips(
                    &audio_clip_ids,
                    &midi_clip_ids,
                    &audio_ids,
                    &midi_ids,
                    start_tick,
                )
                .map_err(Into::into)
        })
    }

    /// Trims an Audio Clip after the host validates its source length.
    pub fn trim_audio_clip(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        source_range: FrameRange,
        source_frames: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .trim_audio_clip(clip_id, start_tick, source_range, source_frames)
                .map_err(Into::into)
        })
    }

    /// Splits an Audio Clip at a musical position.
    pub fn split_audio_clip(
        &self,
        clip_id: &str,
        split_tick: TimelineTick,
        right_id: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .split_audio_clip(clip_id, split_tick, right_id)
                .map_err(Into::into)
        })
    }

    /// Duplicates an Audio Clip with a caller-provided canonical id.
    pub fn duplicate_audio_clip(
        &self,
        clip_id: &str,
        duplicate_id: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_audio_clip(clip_id, duplicate_id)
                .map_err(Into::into)
        })
    }

    /// Trims a MIDI Clip and its contained notes/events.
    pub fn trim_midi_clip(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        duration_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .trim_midi_clip(clip_id, start_tick, duration_ticks)
                .map_err(Into::into)
        })
    }

    /// Splits a MIDI Clip at a musical position.
    pub fn split_midi_clip(
        &self,
        clip_id: &str,
        split_tick: TimelineTick,
        right_id: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .split_midi_clip(clip_id, split_tick, right_id)
                .map_err(Into::into)
        })
    }

    /// Duplicates a MIDI Clip with a caller-provided canonical id.
    pub fn duplicate_midi_clip(
        &self,
        clip_id: &str,
        duplicate_id: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_midi_clip(clip_id, duplicate_id)
                .map_err(Into::into)
        })
    }

    /// Adds one MIDI note to an existing MIDI clip.
    pub fn add_midi_note(
        &self,
        clip_id: &str,
        start_tick: TimelineTick,
        pitch: u8,
        duration_ticks: u64,
        velocity: u8,
        channel: u8,
    ) -> Result<CreativeSession, ApplicationError> {
        if pitch > 127 {
            return Err(ApplicationError::InvalidCommand(
                "midi pitch must be between 0 and 127".into(),
            ));
        }
        if velocity > 127 {
            return Err(ApplicationError::InvalidCommand(
                "midi velocity must be between 0 and 127".into(),
            ));
        }
        if !(1..=16).contains(&channel) {
            return Err(ApplicationError::InvalidCommand(
                "midi channel must be between 1 and 16".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            clip.notes.push(MidiNote {
                id: next_id("note"),
                note: pitch,
                start_tick,
                duration_ticks: duration_ticks.max(1),
                velocity,
                channel,
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Applies one atomic set of updates to notes in a MIDI clip.
    pub fn update_midi_notes(
        &self,
        clip_id: &str,
        updates: Vec<MidiNoteUpdate>,
    ) -> Result<CreativeSession, ApplicationError> {
        if updates.is_empty() {
            return Err(ApplicationError::InvalidCommand(
                "at least one midi note update is required".into(),
            ));
        }
        let unique_ids = updates
            .iter()
            .map(|update| update.note_id.as_str())
            .collect::<HashSet<_>>();
        if unique_ids.len() != updates.len() {
            return Err(ApplicationError::InvalidCommand(
                "each midi note may be updated only once per operation".into(),
            ));
        }
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            for update in updates {
                let note = clip
                    .notes
                    .iter_mut()
                    .find(|note| note.id == update.note_id)
                    .ok_or_else(|| {
                        crate::DomainError::InvalidClip(format!(
                            "midi note '{}' is not registered",
                            update.note_id
                        ))
                    })?;
                if let Some(pitch) = update.patch.note {
                    note.note = pitch.min(127);
                }
                if let Some(start_tick) = update.patch.start_tick {
                    note.start_tick = start_tick;
                }
                if let Some(duration_ticks) = update.patch.duration_ticks {
                    note.duration_ticks = duration_ticks.max(1);
                }
                if let Some(velocity) = update.patch.velocity {
                    note.velocity = velocity.min(127);
                }
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes one MIDI note from an existing MIDI clip.
    pub fn remove_midi_note(
        &self,
        clip_id: &str,
        note_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let clip = arrangement
                .midi_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "midi clip '{clip_id}' is not registered"
                    ))
                })?;
            let before = clip.notes.len();
            clip.notes.retain(|note| note.id != note_id);
            if clip.notes.len() == before {
                return Err(crate::DomainError::InvalidClip(format!(
                    "midi note '{note_id}' is not registered"
                ))
                .into());
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Quantizes selected MIDI notes to a positive grid.
    pub fn quantize_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
        grid_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .quantize_midi_notes(clip_id, &note_ids, grid_ticks)
                .map_err(Into::into)
        })
    }

    /// Duplicates selected MIDI notes within one clip.
    pub fn duplicate_midi_notes(
        &self,
        clip_id: &str,
        note_ids: Vec<String>,
        offset_ticks: u64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .duplicate_midi_notes(clip_id, &note_ids, offset_ticks)
                .map_err(Into::into)
        })
    }

    /// Adds a named timeline marker.
    pub fn add_marker(
        &self,
        tick: TimelineTick,
        name: String,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let name = name.trim().chars().take(80).collect::<String>();
            arrangement.markers.push(Marker {
                id: next_id("marker"),
                name: if name.is_empty() {
                    "Marker".into()
                } else {
                    name
                },
                tick: tick.0,
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Updates one timeline marker.
    pub fn update_marker(
        &self,
        marker_id: &str,
        patch: MarkerPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let marker = arrangement
                .markers
                .iter_mut()
                .find(|marker| marker.id == marker_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "marker '{marker_id}' is not registered"
                    ))
                })?;
            if let Some(name) = patch.name {
                let name = name.trim().chars().take(80).collect::<String>();
                if !name.is_empty() {
                    marker.name = name;
                }
            }
            if let Some(tick) = patch.tick {
                marker.tick = tick.0;
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes one timeline marker.
    pub fn remove_marker(&self, marker_id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let before = arrangement.markers.len();
            arrangement.markers.retain(|marker| marker.id != marker_id);
            if arrangement.markers.len() == before {
                return Err(crate::DomainError::InvalidClip(format!(
                    "marker '{marker_id}' is not registered"
                ))
                .into());
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Applies a crossfade between two neighboring Audio Clips.
    pub fn crossfade_audio_clips(
        &self,
        first_id: &str,
        second_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            arrangement
                .crossfade_audio_clips(first_id, second_id)
                .map_err(Into::into)
        })
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
