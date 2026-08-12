//! Canonical Rack domain model.
//!
//! A Track's live signal chain: device order, plugin state, parameters,
//! bypass, and utility settings needed for audio processing right now.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Functional role of a slot in the rack signal chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Plugin,
    Utility,
    Output,
}

/// One slot in a rack: an input, plugin, utility, or output stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    pub bypassed: bool,
    pub gain_db: f64,
    #[serde(default)]
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
    #[serde(default)]
    pub disabled_placeholder: bool,
}

/// A named, ranged macro control mapped to a rack parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackMacro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parameter_index: Option<u32>,
}

/// The live rack currently in use on a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackInstance {
    pub devices: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}
