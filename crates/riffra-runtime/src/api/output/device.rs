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
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities {
    pub parameters: bool,
    pub state: bool,
    pub presets: bool,
    pub editor: bool,
}

/// One persisted or runtime-reported plugin parameter.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceParameterInfo {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub value: f32,
    pub default_value: f32,
    pub automatable: bool,
}

/// One program exposed by a plugin host.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginPresetInfo {
    pub index: u32,
    pub name: String,
}

/// Opaque plugin state transported between the Host and the CLI.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginStateSnapshot {
    pub schema_version: u32,
    pub plugin_path: String,
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
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
