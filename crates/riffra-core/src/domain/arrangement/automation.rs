use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
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
