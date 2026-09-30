//! Params of commands that require a live Host runtime.

use riffra_core::{AudioTakeVariant, MusicalPosition};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Audio preference input shared by shell adapters and the Host workflow.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioDriverConfig {
    /// Native driver name.
    pub driver: String,
    /// Optional input device name.
    pub input_device: Option<String>,
    /// Input channel index.
    pub input_channel: u32,
    /// Optional output device name.
    pub output_device: Option<String>,
    /// Requested sample rate.
    pub sample_rate: Option<u32>,
    /// Requested buffer size.
    pub buffer_size: Option<u32>,
}

/// The timeline span of an offline render.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RenderRange {
    #[default]
    EntireArrangement,
    LoopRange,
    TimeSelection {
        start: MusicalPosition,
        end: MusicalPosition,
    },
}

/// Range, normalization, and Track selection of an offline render.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RenderOptions {
    #[serde(default)]
    pub range: RenderRange,
    #[serde(default)]
    pub normalize: bool,
    pub track_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SeekParams {
    pub tick: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MasterGainParams {
    pub gain_db: f64,
}

/// Previews a Track mix without persisting it; at least one value is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct TrackMixParams {
    pub track_id: String,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MuteParams {
    pub muted: bool,
}

/// Sends raw MIDI bytes to a Track's instrument.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiSendParams {
    pub track_id: String,
    pub bytes: Vec<u8>,
}

/// Routes live MIDI input to a Track; `null` clears the target.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct LiveMidiTargetParams {
    pub track_id: Option<String>,
}

/// Scans a VST3 folder; defaults to the platform plugin folder.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PluginScanParams {
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioChannelsProbeParams {
    pub driver: String,
    pub input_device: String,
    pub output_device: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioDiagnosticsParams {
    /// Includes projection and timeline details outside the stable contract.
    #[serde(default)]
    pub debug: bool,
}

/// Previews an Asset through the live output.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct AssetPreviewParams {
    pub asset_id: String,
    #[serde(default)]
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub looped: bool,
    #[serde(default = "unity_gain")]
    pub gain: f32,
}

fn unity_gain() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RenderStartParams {
    pub options: Option<RenderOptions>,
}

/// Analyzes an Asset or a file; exactly one source is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct AnalysisParams {
    pub asset_id: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TakeIdParams {
    pub take_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TakeActivateParams {
    pub session_id: String,
    pub take_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TakeVariantParams {
    pub clip_id: String,
    pub variant: AudioTakeVariant,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TakeComparisonParams {
    pub variant: AudioTakeVariant,
}
