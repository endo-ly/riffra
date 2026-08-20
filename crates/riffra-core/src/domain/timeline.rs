//! Shared musical-time and source-frame value types.

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
    pub(super) fn len(self) -> u64 {
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

    pub(super) fn ticks_to_frames(self, ticks: u64, sample_rate: u32) -> u64 {
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
