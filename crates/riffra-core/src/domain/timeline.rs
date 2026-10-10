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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FrameDuration {
    pub frames: u64,
    pub sample_rate: u32,
}

/// Musical clock shared by the ruler, snapping, MIDI, and transport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectTimebase {
    pub ppq: u32,
    pub tempo_changes: Vec<TempoChange>,
    pub time_signature_changes: Vec<TimeSignatureChange>,
}

/// A tempo effective from an absolute timeline tick.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TempoChange {
    pub tick: u64,
    pub bpm: f64,
}

/// A time signature effective from an absolute timeline tick.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TimeSignatureChange {
    pub tick: u64,
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for ProjectTimebase {
    fn default() -> Self {
        Self {
            ppq: TIMELINE_PPQ,
            tempo_changes: vec![TempoChange {
                tick: 0,
                bpm: 120.0,
            }],
            time_signature_changes: vec![TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
        }
    }
}

impl ProjectTimebase {
    /// Validates the fixed PPQ and complete, ordered musical clock.
    ///
    /// # Errors
    /// Returns an error for missing tick-zero changes, duplicate positions,
    /// unsupported signatures, or non-positive/non-finite tempo values.
    pub fn validate(&self) -> Result<(), String> {
        if self.ppq != TIMELINE_PPQ
            || self.tempo_changes.first().is_none_or(|p| p.tick != 0)
            || self
                .time_signature_changes
                .first()
                .is_none_or(|p| p.tick != 0)
            || self
                .tempo_changes
                .windows(2)
                .any(|p| p[0].tick >= p[1].tick)
            || self
                .time_signature_changes
                .windows(2)
                .any(|p| p[0].tick >= p[1].tick)
            || self
                .tempo_changes
                .iter()
                .any(|p| !p.bpm.is_finite() || p.bpm <= 0.0)
            || self
                .time_signature_changes
                .iter()
                .any(|p| p.numerator == 0 || !matches!(p.denominator, 1 | 2 | 4 | 8 | 16 | 32))
        {
            return Err("invalid project timebase".into());
        }
        Ok(())
    }

    /// Integrates elapsed seconds through all tempo intervals before `tick`.
    pub fn ticks_to_seconds(&self, tick: u64) -> f64 {
        let mut seconds = 0.0;
        for (i, point) in self.tempo_changes.iter().enumerate() {
            if point.tick >= tick {
                break;
            }
            let end = self
                .tempo_changes
                .get(i + 1)
                .map_or(tick, |p| p.tick.min(tick));
            seconds += (end - point.tick) as f64 * 60.0 / (point.bpm * f64::from(self.ppq));
        }
        seconds
    }

    /// Inverts the same tempo intervals and rounds only the final tick.
    pub fn seconds_to_ticks(&self, seconds: f64) -> TimelineTick {
        let mut remaining = seconds.max(0.0);
        for (i, point) in self.tempo_changes.iter().enumerate() {
            let ticks_per_second = point.bpm * f64::from(self.ppq) / 60.0;
            if let Some(next) = self.tempo_changes.get(i + 1) {
                let duration = (next.tick - point.tick) as f64 / ticks_per_second;
                if remaining > duration {
                    remaining -= duration;
                    continue;
                }
            }
            return TimelineTick((point.tick as f64 + remaining * ticks_per_second).round() as u64);
        }
        TimelineTick(0)
    }

    /// Converts a real-time millisecond offset to the nearest timeline tick.
    pub fn milliseconds_to_ticks(&self, milliseconds: f64) -> TimelineTick {
        self.seconds_to_ticks(milliseconds / 1000.0)
    }

    /// Converts source frames at `sample_rate` to the nearest Timeline tick.
    pub fn frames_to_ticks(&self, frames: u64, sample_rate: u32) -> TimelineTick {
        self.seconds_to_ticks(frames as f64 / f64::from(sample_rate))
    }

    /// Converts a frame duration at its timeline position into a tick duration.
    pub fn duration_to_ticks(&self, start: TimelineTick, duration: FrameDuration) -> u64 {
        self.seconds_to_ticks(
            self.ticks_to_seconds(start.0)
                + duration.frames as f64 / f64::from(duration.sample_rate),
        )
        .0
        .saturating_sub(start.0)
    }

    /// Converts absolute ticks to sample frames, rounding once after integration.
    pub fn ticks_to_frames(&self, ticks: u64, sample_rate: u32) -> u64 {
        (self.ticks_to_seconds(ticks) * f64::from(sample_rate)).round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_tempo_segments_and_inverts_the_same_clock() {
        let map = ProjectTimebase {
            tempo_changes: vec![
                TempoChange {
                    tick: 0,
                    bpm: 120.0,
                },
                TempoChange {
                    tick: 1920,
                    bpm: 90.0,
                },
            ],
            time_signature_changes: vec![
                TimeSignatureChange {
                    tick: 0,
                    numerator: 4,
                    denominator: 4,
                },
                TimeSignatureChange {
                    tick: 1920,
                    numerator: 3,
                    denominator: 4,
                },
            ],
            ..ProjectTimebase::default()
        };

        map.validate().unwrap();

        assert_eq!(map.ticks_to_frames(1920, 48000), 48000);
        assert_eq!(map.ticks_to_frames(3360, 48000), 96000);
        assert_eq!(map.frames_to_ticks(96000, 48000), TimelineTick(3360));
        assert_eq!(
            map.tick_to_musical_position(TimelineTick(1920)).to_string(),
            "2:1"
        );
        assert_eq!(
            map.musical_position_to_tick("3:1".parse().unwrap())
                .unwrap(),
            TimelineTick(4800)
        );
        assert!(
            map.musical_position_to_tick("1:4".parse().unwrap())
                .is_err()
        );
    }
}

/// Persisted loop selection. Disabled ranges retain their endpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TimelineLoopRange {
    pub enabled: bool,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub end_tick: TimelineTick,
}

/// Optional non-destructive punch recording range on the project timeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TimelinePunchRange {
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    #[ts(type = "number")]
    pub end_tick: TimelineTick,
}
