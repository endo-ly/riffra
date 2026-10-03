//! Params of instrument slot, effect, plugin, and missing-dependency commands.

use crate::api::output::PluginStateSnapshot;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Loads a VST3 into a Track's instrument slot or effect chain.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginPathParams {
    pub track_id: String,
    pub plugin_path: String,
}

/// Opens a validated VST3 outside the Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginAuditionParams {
    pub plugin_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EffectReorderParams {
    pub track_id: String,
    pub device_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceBypassParams {
    pub track_id: String,
    pub device_id: String,
    pub bypassed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceParameterSetParams {
    pub track_id: String,
    pub device_id: String,
    pub parameter_index: u32,
    pub value: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceParameterGetParams {
    pub track_id: String,
    pub device_id: String,
    pub parameter_index: u32,
}

/// Selects a plugin program by name or index; exactly one is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginPresetSetParams {
    pub track_id: String,
    pub device_id: String,
    pub preset: Option<String>,
    pub preset_index: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginStateSetParams {
    pub track_id: String,
    pub device_id: String,
    pub state: PluginStateSnapshot,
}

/// Replaces a missing audio Asset with a file on disk.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingRelinkParams {
    pub asset_id: String,
    pub new_path: String,
}

/// Replaces a missing VST3 with another plugin binary.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingPluginReplaceParams {
    pub device_id: String,
    pub new_path: String,
}
