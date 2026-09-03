use crate::domain::asset::AssetId;
use crate::domain::recording::AudioTakeVariant;
use crate::domain::timeline::{FrameDuration, FrameRange, TimelineTick};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

    /// Validates and normalizes the rules owned by one audio clip.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.track_id.trim().is_empty()
            || self.asset_id.as_str().trim().is_empty()
        {
            return Err("Audio clips require ids, names, tracks and asset ids.".into());
        }
        if !self.gain_db.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid gain.", self.id));
        }
        if !self.pan.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid pan.", self.id));
        }
        self.normalize_fields();
        Ok(())
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainError;
    use crate::domain::arrangement::{Arrangement, MidiClip, Track};
    use crate::domain::asset::{AssetId, mint_asset_id};
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
}
