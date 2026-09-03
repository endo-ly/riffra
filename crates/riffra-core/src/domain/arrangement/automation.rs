use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

/// A Track mix parameter controlled on the Arrangement timeline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AutomationParameter {
    Volume,
    Pan,
}

/// A single value on an Automation Lane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPoint {
    pub id: String,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub value: f64,
}

/// Timeline control data for one Track parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationLane {
    pub id: String,
    pub track_id: String,
    pub parameter: AutomationParameter,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    /// Validates and normalizes the points owned by one automation lane.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.points.len() > 16_384 {
            return Err(format!(
                "Automation Lane '{}' contains too many points.",
                self.id
            ));
        }
        self.points.sort_by_key(|point| point.tick);
        let mut point_ids = HashSet::new();
        let mut previous_tick = None;
        for point in &mut self.points {
            if point.id.trim().is_empty()
                || !point_ids.insert(point.id.as_str())
                || previous_tick == Some(point.tick)
                || !point.value.is_finite()
            {
                return Err(format!(
                    "Automation Lane '{}' contains an invalid point.",
                    self.id
                ));
            }
            point.value = match self.parameter {
                AutomationParameter::Volume => point.value.clamp(-90.0, 24.0),
                AutomationParameter::Pan => point.value.clamp(-1.0, 1.0),
            };
            previous_tick = Some(point.tick);
        }
        Ok(())
    }
}
