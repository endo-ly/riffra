//! Canonical CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! [`Arrangement`] and session settings. It deliberately does not own host
//! view state, audio/MIDI file bodies, the Library index, recording files, or
//! background-job state.

use crate::domain::arrangement::*;
use crate::domain::rack::{RackDevice, RackInstance};
use crate::domain::timeline::TIMELINE_PPQ;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreativeSession {
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub arrangement: Arrangement,
    pub settings: SessionSettings,
}

/// Deserializes a canonical session payload.
///
/// # Errors
/// Returns a JSON error when the payload does not match the current schema.
pub fn deserialize_session(payload: &[u8]) -> Result<CreativeSession, serde_json::Error> {
    serde_json::from_slice(payload)
}

impl CreativeSession {
    /// Creates a fresh session with an empty arrangement and neutral playback
    /// settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            arrangement: Arrangement::default(),
            settings: SessionSettings {
                master_db: 0.0,
                loop_enabled: false,
                count_in_beats: 0,
                metronome_enabled: false,
                note: String::new(),
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

        normalize_arrangement(&mut self.arrangement)?;
        Ok(self)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainError;
    use crate::domain::asset::{AssetId, Provenance, mint_asset_id};
    use crate::domain::recording::{
        RecordingPassRecord, RecordingSessionRecord, RecordingSessionTrackSlot,
        RecordingTakeRecord, TakeAudioSource,
    };
    use crate::domain::timeline::{
        FrameDuration, FrameRange, ProjectTimebase, TimelineLoopRange, TimelineTick,
    };

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
    fn new_session_has_empty_arrangement_and_unity_master() {
        let session = CreativeSession::new(0);
        assert!(session.arrangement.tracks.is_empty());
        assert_eq!(session.settings.master_db, 0.0);
        // An unused provenance reference keeps the asset import meaningful here.
        let _ = Provenance::recorded_root();
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
