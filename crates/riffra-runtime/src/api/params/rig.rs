//! Params of rig commands. Rigs belong to the Host, not to a Project.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Identifies one rig.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RigIdParams {
    pub rig_id: String,
}

/// Creates a rig on one audio input channel.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RigCreateParams {
    pub name: String,
    pub channel_index: u32,
}

/// Changes a rig; absent fields keep their current values.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RigUpdateParams {
    pub rig_id: String,
    pub name: Option<String>,
    pub channel_index: Option<u32>,
    pub gain_db: Option<f64>,
}

/// Adds a VST3 effect to a rig.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RigEffectAddParams {
    pub rig_id: String,
    pub plugin_path: Option<String>,
    pub plugin_id: Option<String>,
}

/// Identifies one effect of a rig.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RigDeviceParams {
    pub rig_id: String,
    pub device_id: String,
}

/// Reorders every effect of one rig.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RigEffectReorderParams {
    pub rig_id: String,
    pub device_ids: Vec<String>,
}

/// Changes one rig effect's bypass state.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RigDeviceBypassParams {
    pub rig_id: String,
    pub device_id: String,
    pub bypassed: bool,
}
