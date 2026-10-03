//! Library, instrument library, and analysis results.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAsset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub tag: Option<String>,
    pub note: Option<String>,
    pub created_at_ms: Option<u64>,
    pub updated_at_ms: Option<u64>,
    pub stability: String,
}

/// The origin of an instrument exposed by the library.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstrumentOrigin {
    BuiltIn,
    User,
}

/// A user-visible instrument with persisted library preferences.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentLibraryItem {
    pub id: String,
    pub preset_id: Option<String>,
    pub origin: InstrumentOrigin,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    /// The library category the instrument's definition files it under.
    pub default_category: String,
    /// The default category, or the user's choice for a User Instrument.
    pub category: String,
    pub default_tags: Vec<String>,
    pub user_tags: Vec<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub collection_ids: Vec<i64>,
    pub recommended_range: Option<InstrumentRecommendedRange>,
    pub preview: Option<InstrumentPreviewDefinition>,
}

/// A named user collection of instruments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentCollection {
    pub id: i64,
    pub name: String,
}

/// The practical MIDI range recommended for one instrument.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentRecommendedRange {
    pub min_midi: u8,
    pub max_midi: u8,
}

/// The deterministic MIDI pattern used for an instrument preview.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewDefinition {
    pub tempo_bpm: f64,
    pub ticks_per_beat: u16,
    pub time_signature: InstrumentPreviewTimeSignature,
    pub length_ticks: u64,
    pub notes: Vec<InstrumentPreviewNote>,
}

/// The meter used by an instrument preview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewTimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

/// One MIDI note in an instrument preview.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentPreviewNote {
    pub tick: u64,
    pub duration_ticks: u64,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: u64,
    pub duration_ms: u64,
    pub peak_db: f64,
    pub true_peak_db: f64,
    pub rms_db: f64,
    pub clipping_samples: u64,
    pub dynamic_range_db: f64,
    pub zero_crossings: u64,
    pub phase_correlation: Option<f64>,
    pub spectrum_peak_hz: Option<f64>,
    pub waveform: Vec<f64>,
}

/// Location of an exported user instrument package.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentExport {
    pub instrument_id: String,
    pub path: String,
}
