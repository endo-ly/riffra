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
