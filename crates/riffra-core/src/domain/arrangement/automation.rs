use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

pub(super) const MAX_AUTOMATION_LANES: usize = 16_384;

/// A Track mix parameter controlled on the Arrangement timeline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TrackMixParameter {
    Volume,
    Pan,
}

/// A Bus fader parameter controlled on the Arrangement timeline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum BusMixParameter {
    Gain,
    Pan,
}

/// Stable identity of a parameter controlled by a timeline lane.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all_fields = "camelCase")]
pub enum AutomationTarget {
    TrackMix {
        track_id: String,
        parameter: TrackMixParameter,
    },
    BusMix {
        bus_id: String,
        parameter: BusMixParameter,
    },
    MasterGain,
    SendLevel {
        send_id: String,
    },
    DeviceParameter {
        device_id: String,
        parameter_id: String,
    },
}

/// Evaluation between adjacent points in musical time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AutomationInterpolation {
    Linear,
    Step,
}

impl AutomationTarget {
    /// Inclusive value range in dB, pan units, or normalized plugin units.
    pub fn value_range(&self) -> (f64, f64) {
        match self {
            Self::TrackMix {
                parameter: TrackMixParameter::Pan,
                ..
            }
            | Self::BusMix {
                parameter: BusMixParameter::Pan,
                ..
            } => (-1.0, 1.0),
            Self::MasterGain => (-90.0, 0.0),
            Self::DeviceParameter { .. } => (0.0, 1.0),
            _ => (-90.0, 24.0),
        }
    }

    pub(crate) fn is_on_track(&self, track: &super::Track, sends: &[super::Send]) -> bool {
        match self {
            Self::TrackMix { track_id, .. } => *track_id == track.id,
            Self::DeviceParameter { device_id, .. } => {
                track
                    .instrument
                    .as_ref()
                    .is_some_and(|device| device.id == *device_id)
                    || track.effects.iter().any(|device| device.id == *device_id)
            }
            Self::SendLevel { send_id } => sends
                .iter()
                .any(|send| send.id == *send_id && send.track_id == track.id),
            _ => false,
        }
    }
}

impl super::Arrangement {
    /// Resolves a lane's canonical references without requiring an installed plugin.
    pub fn has_automation_target(&self, target: &AutomationTarget) -> bool {
        match target {
            AutomationTarget::TrackMix { track_id, .. } => {
                self.tracks.iter().any(|track| track.id == *track_id)
            }
            AutomationTarget::BusMix { bus_id, .. } => {
                self.mixer.buses.iter().any(|bus| bus.id == *bus_id)
            }
            AutomationTarget::MasterGain => true,
            AutomationTarget::SendLevel { send_id } => {
                self.mixer.sends.iter().any(|send| send.id == *send_id)
            }
            AutomationTarget::DeviceParameter {
                device_id,
                parameter_id,
            } => {
                !parameter_id.trim().is_empty()
                    && (self.tracks.iter().any(|track| {
                        track.instrument.as_ref().is_some_and(|device| {
                            device.id == *device_id
                                && matches!(device.source, crate::TrackInstrumentSource::Vst3(_))
                        })
                    }) || self
                        .tracks
                        .iter()
                        .flat_map(|track| &track.effects)
                        .chain(self.mixer.buses.iter().flat_map(|bus| &bus.effects))
                        .chain(&self.mixer.master_effects)
                        .any(|device| device.id == *device_id))
            }
        }
    }
}

/// A single value on an Automation Lane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationPoint {
    pub id: String,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub value: f64,
}

/// Timeline control data for one canonical parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationLane {
    pub id: String,
    pub target: AutomationTarget,
    pub interpolation: AutomationInterpolation,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    /// Validates and normalizes the points owned by one automation lane.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.points.len() > 16_384 {
            return Err(format!(
                "automation lane '{}' contains too many points",
                self.id
            ));
        }
        self.points.sort_by_key(|point| point.tick);
        let mut point_ids = HashSet::new();
        let mut previous_tick = None;
        for point in &self.points {
            if point.id.trim().is_empty() || !point_ids.insert(point.id.as_str()) {
                return Err(format!(
                    "automation lane '{}' has invalid or duplicate point id '{}'",
                    self.id, point.id
                ));
            }
            if previous_tick == Some(point.tick) {
                return Err(format!(
                    "automation lane '{}' has duplicate tick {} at point '{}'",
                    self.id, point.tick.0, point.id
                ));
            }
            let (minimum, maximum) = self.target.value_range();
            if !point.value.is_finite() || !(minimum..=maximum).contains(&point.value) {
                return Err(format!(
                    "automation lane '{}' point '{}' requires a finite value in {minimum}..={maximum}",
                    self.id, point.id
                ));
            }
            previous_tick = Some(point.tick);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::arrangement::{Arrangement, Track};
    use crate::domain::timeline::TimelineTick;

    #[test]
    fn automation_points_are_canonical_and_follow_track_ownership() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::audio("main".into(), "Main".into()));
        arrangement.automation_lanes.push(AutomationLane {
            id: "automation:main:volume".into(),
            target: crate::AutomationTarget::TrackMix {
                track_id: "main".into(),
                parameter: crate::TrackMixParameter::Volume,
            },
            interpolation: crate::AutomationInterpolation::Linear,
            points: vec![
                AutomationPoint {
                    id: "late".into(),
                    tick: TimelineTick(960),
                    value: 24.0,
                },
                AutomationPoint {
                    id: "early".into(),
                    tick: TimelineTick(0),
                    value: -90.0,
                },
            ],
        });

        Arrangement::validate_and_normalize(&mut arrangement).unwrap();

        assert_eq!(arrangement.automation_lanes[0].points[0].id, "early");
        assert_eq!(arrangement.automation_lanes[0].points[0].value, -90.0);
        assert_eq!(arrangement.automation_lanes[0].points[1].value, 24.0);
        for invalid in [f64::NAN, -90.01, 24.01] {
            let mut candidate = arrangement.clone();
            candidate.automation_lanes[0].points[0].value = invalid;
            assert!(Arrangement::validate_and_normalize(&mut candidate).is_err());
        }
        let mut duplicate = arrangement.clone();
        duplicate.automation_lanes[0].points[1].tick = TimelineTick(0);
        let error = Arrangement::validate_and_normalize(&mut duplicate).unwrap_err();
        assert!(error.contains("duplicate tick 0") && error.contains("late"));
        duplicate = arrangement.clone();
        duplicate.automation_lanes.push(AutomationLane {
            id: "another".into(),
            ..duplicate.automation_lanes[0].clone()
        });
        assert!(Arrangement::validate_and_normalize(&mut duplicate).is_err());
        duplicate = arrangement.clone();
        duplicate.automation_lanes[0].target = AutomationTarget::MasterGain;
        assert!(Arrangement::validate_and_normalize(&mut duplicate).is_err());
        duplicate.automation_lanes[0].points[1].value = 0.0;
        assert!(Arrangement::validate_and_normalize(&mut duplicate).is_ok());
        duplicate.automation_lanes[0].target = AutomationTarget::DeviceParameter {
            device_id: "missing".into(),
            parameter_id: "gain".into(),
        };
        assert!(Arrangement::validate_and_normalize(&mut duplicate).is_err());
        arrangement.remove_track("main").unwrap();
        assert!(arrangement.automation_lanes.is_empty());
    }
}
