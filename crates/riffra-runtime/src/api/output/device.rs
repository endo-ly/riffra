//! Track device, plugin, and dependency results.

use riffra_core::AssetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lightweight metadata and capabilities for one Track Device.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInspection {
    pub id: String,
    pub name: String,
    pub source: String,
    pub bypassed: bool,
    pub capabilities: DeviceCapabilities,
    pub parameter_count: usize,
    pub state_persisted: bool,
}

/// Host-visible capabilities of one Track Device.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities {
    pub parameters: bool,
    pub state: bool,
    pub presets: bool,
    pub editor: bool,
    pub sidechain: DeviceSidechainCapability,
}

/// Auxiliary input support of one Track Device.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSidechainCapability {
    pub available: bool,
    pub mono: bool,
    pub stereo: bool,
    pub channels: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// One normalized discrete value and its plugin-provided display text.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterChoice {
    pub value: f32,
    pub display_value: String,
}

/// One runtime-reported plugin parameter.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterInfo {
    pub parameter_id: Option<String>,
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub value: f32,
    pub default_value: f32,
    pub automatable: bool,
    pub display_value: String,
    pub label: String,
    pub discrete: bool,
    pub step_count: u32,
    pub choices: Vec<DeviceParameterChoice>,
}

/// Result of setting one plugin parameter on a live Host.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterMutationResult {
    pub mutation: super::ArrangementMutationResult,
    /// The parameter that was set, read back after the change.
    pub parameter: DeviceParameterInfo,
    /// Other parameters whose name, display value, or value changed.
    pub changed_parameters: Vec<DeviceParameterInfo>,
}

/// One program exposed by a plugin host.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginPresetInfo {
    pub index: u32,
    pub name: String,
}

/// Opaque plugin state transported between the Host and the CLI.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginStateSnapshot {
    pub schema_version: u32,
    /// Stable VST3 class identifier reported by the scanner.
    pub plugin_identifier: String,
    /// Path of the plugin when the snapshot was taken. Informational only;
    /// validation uses [`Self::plugin_identifier`].
    pub plugin_path: String,
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
}

/// A value stored in a JUCE `ValueTree` property.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(untagged)]
pub enum PluginStateValue {
    Bool(bool),
    Integer(i32),
    Double(f64),
    Text(String),
    Encoded(PluginStateEncodedValue),
}

/// A `ValueTree` value without a plain JSON form.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginStateEncodedValue {
    Void,
    Int64 {
        value: i64,
    },
    Binary {
        #[serde(with = "standard_base64")]
        #[ts(type = "string")]
        #[schemars(with = "String")]
        base64: Vec<u8>,
    },
    /// A JUCE `var` of a type that is carried over without interpretation.
    Opaque {
        marker: u8,
        #[serde(with = "standard_base64")]
        #[ts(type = "string")]
        #[schemars(with = "String")]
        base64: Vec<u8>,
    },
}

mod standard_base64 {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        STANDARD
            .decode(String::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

/// One named `ValueTree` property. Property order is part of the encoded state.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginStateProperty {
    pub name: String,
    pub value: PluginStateValue,
}

/// A JUCE `ValueTree` decoded from plugin state.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginStateTree {
    #[serde(rename = "type")]
    pub tree_type: String,
    pub properties: Vec<PluginStateProperty>,
    pub children: Vec<PluginStateTree>,
}

/// How a plugin's component state is packaged.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PluginStateContainer {
    /// The plugin content is stored directly in the VST3 component.
    Component,
    /// The plugin content is stored in a VST2-compatible fxb bank chunk.
    Fxb,
}

/// A size-bounded description of one state value.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginStateContent {
    Value {
        value: PluginStateValue,
    },
    Xml {
        root: String,
        #[ts(type = "number")]
        bytes: usize,
    },
    ValueTree {
        tree: PluginStateTreeSummary,
    },
    Data {
        #[ts(type = "number")]
        bytes: usize,
    },
}

/// A property within a [`PluginStateTreeSummary`].
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PluginStatePropertySummary {
    pub name: String,
    pub content: PluginStateContent,
}

/// A `ValueTree` whose large values are reduced to their kind and size.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PluginStateTreeSummary {
    #[serde(rename = "type")]
    pub tree_type: String,
    pub properties: Vec<PluginStatePropertySummary>,
    pub children: Vec<PluginStateTreeSummary>,
}

/// The decoded structure of a plugin's state.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PluginStateInspection {
    pub container: PluginStateContainer,
    pub content: PluginStateContent,
}

/// One value read from a plugin's state.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginStateReadResult {
    Value { value: PluginStateValue },
    Xml { text: String },
    Tree { tree: PluginStateTree },
}

/// Plugin container format. The runtime currently hosts VST3 only; the enum
/// keeps the TS literal narrow and leaves room for future formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum PluginFormat {
    Vst3,
}

/// Outcome of a plugin scan/validate pass, used as the Library stability label.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum PluginScanState {
    Discovered,
    Validated,
    Failed,
    Quarantined,
}

/// The role a VST3 plug-in can serve in Riffra's signal graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum PluginRole {
    /// Generates audio from MIDI events on an instrument track.
    Instrument,
    /// Processes audio in a track's effect chain.
    Effect,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    /// Identifier reported by the plugin scanner after validation.
    pub identifier: Option<String>,
    pub format: PluginFormat,
    /// The plug-in role reported by the VST3 scanner, when available.
    pub role: Option<PluginRole>,
    pub path: String,
    pub bundle: bool,
    pub modified_at_ms: Option<u64>,
    pub scan_state: PluginScanState,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub root: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub plugins: Vec<PluginEntry>,
    pub issues: Vec<ScanIssue>,
}

impl PluginScanState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Validated => "validated",
            Self::Failed => "failed",
            Self::Quarantined => "quarantined",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MissingDependency {
    /// `file` for a missing audio asset, `plugin` for a missing VST3 binary.
    pub kind: String,
    pub id: String,
    pub name: String,
    /// Resolved content location (for files) or plugin path (for plugins), for
    /// display only. Relink is driven by `asset_id`, not this path.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset_id: Option<AssetId>,
    /// Where the missing dependency is referenced from, so the UI can point the
    /// user at the exact clip, instrument, or effect slot.
    pub used_by: Vec<String>,
}
