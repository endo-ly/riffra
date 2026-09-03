//! Canonical CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! [`Arrangement`] and session settings. It deliberately does not own host
//! view state, audio/MIDI file bodies, the Library index, recording files, or
//! background-job state.

use crate::domain::arrangement::*;
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

        Arrangement::validate_and_normalize(&mut self.arrangement)?;
        Ok(self)
    }
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
        FrameDuration, FrameRange, ProjectTimebase, TIMELINE_PPQ, TimelineLoopRange, TimelineTick,
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

    #[test]
    fn timeline_regions_allow_overlap_and_nesting_but_require_valid_ranges() {
        let mut arrangement = Arrangement::default();
        arrangement
            .add_region(TimelineRegion {
                id: "region:outer".into(),
                name: "A".into(),
                start_tick: TimelineTick(0),
                end_tick: TimelineTick(3_840),
            })
            .unwrap();
        arrangement
            .add_region(TimelineRegion {
                id: "region:inner".into(),
                name: "A".into(),
                start_tick: TimelineTick(960),
                end_tick: TimelineTick(1_920),
            })
            .unwrap();
        assert_eq!(arrangement.regions.len(), 2);
        assert!(
            arrangement
                .add_region(TimelineRegion {
                    id: "region:empty".into(),
                    name: " ".into(),
                    start_tick: TimelineTick(0),
                    end_tick: TimelineTick(960),
                })
                .is_err()
        );
        assert!(
            arrangement
                .add_region(TimelineRegion {
                    id: "region:inverted".into(),
                    name: "B".into(),
                    start_tick: TimelineTick(1_920),
                    end_tick: TimelineTick(960),
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
            regions: Vec::new(),
            harmony_events: Vec::new(),
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

        Arrangement::validate_and_normalize(&mut arrangement).unwrap();

        assert_eq!(arrangement.automation_lanes[0].points[0].id, "early");
        assert_eq!(arrangement.automation_lanes[0].points[0].value, -90.0);
        assert_eq!(arrangement.automation_lanes[0].points[1].value, 24.0);
        arrangement.remove_track("main").unwrap();
        assert!(arrangement.automation_lanes.is_empty());
    }

    #[test]
    fn harmony_events_are_sorted_without_rejecting_overlap_or_gaps() {
        use crate::domain::music::{HarmonyChord, HarmonyEvent};

        let chord = HarmonyChord::resolve("C").unwrap();
        let mut arrangement = Arrangement {
            harmony_events: vec![
                HarmonyEvent {
                    id: "harmony:late".into(),
                    start_tick: TimelineTick(1_920),
                    end_tick: TimelineTick(3_840),
                    chord: chord.clone(),
                },
                HarmonyEvent {
                    id: "harmony:overlap".into(),
                    start_tick: TimelineTick(960),
                    end_tick: TimelineTick(2_880),
                    chord: chord.clone(),
                },
                HarmonyEvent {
                    id: "harmony:gap".into(),
                    start_tick: TimelineTick(4_800),
                    end_tick: TimelineTick(5_760),
                    chord,
                },
            ],
            ..Arrangement::default()
        };

        Arrangement::validate_and_normalize(&mut arrangement).unwrap();

        assert_eq!(
            arrangement
                .harmony_events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["harmony:overlap", "harmony:late", "harmony:gap"]
        );
    }

    #[test]
    fn harmony_event_ids_must_be_unique_during_normalization() {
        use crate::domain::music::{HarmonyChord, HarmonyEvent};

        let event = HarmonyEvent {
            id: "harmony:duplicate".into(),
            start_tick: TimelineTick(0),
            end_tick: TimelineTick(960),
            chord: HarmonyChord::resolve("C").unwrap(),
        };
        let mut arrangement = Arrangement {
            harmony_events: vec![event.clone(), event],
            ..Arrangement::default()
        };

        assert!(Arrangement::validate_and_normalize(&mut arrangement).is_err());
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
