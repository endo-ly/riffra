//! Params of instrument slot, effect, plugin, and missing-dependency commands.

use crate::api::output::{PluginStateSnapshot, PluginStateTree, PluginStateValue};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Loads a VST3 into a Track's instrument slot or effect chain.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginPathParams {
    pub track_id: String,
    pub plugin_path: Option<String>,
    pub plugin_id: Option<String>,
}

/// Inserts an effect into a Track, Bus, or Master chain.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct EffectAddParams {
    pub owner: riffra_core::EffectOwner,
    pub plugin_path: Option<String>,
    pub plugin_id: Option<String>,
}

/// Opens a validated VST3 outside the Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginAuditionParams {
    pub plugin_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EffectReorderParams {
    pub owner: riffra_core::EffectOwner,
    pub device_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceBypassParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub bypassed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct DeviceParameterSetParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub parameter_index: u32,
    pub value: Option<f32>,
    pub display: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub nearest: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceParameterGetParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub parameter_index: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceParameterFormatParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub parameter_index: u32,
    pub value: f32,
}

/// Selects a plugin program by name or index; exactly one is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginPresetSetParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub preset: Option<String>,
    pub preset_index: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginStateSetParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub state: PluginStateSnapshot,
}

/// Selects one value in a plugin's decoded state.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginStateReadParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    /// `ValueTree` path such as `PROJECT/Slot[1]/Level`; omit for the whole state.
    pub path: Option<String>,
    /// Path below the root of the XML held at `path`, such as `Part0/Name` or `Part0/@id`.
    pub xml_path: Option<String>,
}

/// Applies edits to a plugin's live state as one replacement.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginStatePatchParams {
    pub owner: riffra_core::EffectOwner,
    pub device_id: String,
    pub operations: Vec<PluginStatePatchOperation>,
}

/// One edit of a plugin's decoded state.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PluginStatePatchOperation {
    /// Replaces a property value, keeping its JUCE type, or the whole XML document it holds.
    Set {
        path: String,
        value: PluginStateValue,
    },
    /// Replaces a child tree or a property holding an encoded tree.
    SetTree { path: String, tree: PluginStateTree },
    /// Sets elements and attributes of the XML held at `path`.
    SetXml {
        path: String,
        values: Vec<PluginStateXmlValue>,
    },
}

/// One XML element or attribute assignment.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginStateXmlValue {
    /// Path below the root element; a missing last element is appended to its parent.
    pub xml_path: String,
    pub value: String,
}

/// Imports a saved VST3 state into the Project without querying a live plugin.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginStateImportParams {
    pub owner: riffra_core::EffectOwner,
    /// Omit to select the owner's Track instrument; required for effects.
    pub device_id: Option<String>,
    pub state: PluginStateSnapshot,
}

/// Replaces a missing audio Asset with a file on disk.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingRelinkParams {
    pub asset_id: String,
    pub new_path: String,
}

/// Replaces a missing VST3 with another plugin binary.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingPluginReplaceParams {
    pub device_id: String,
    pub new_path: String,
}
