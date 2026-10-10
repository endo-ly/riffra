//! Background job results.

use super::ScanReport;
use riffra_core::AssetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lifecycle state of a background job. Terminal states (`Cancelled`,
/// `Completed`, `Failed`) cannot return to `Running`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

/// Background job kind. Acts as the `kind` discriminator of
/// [`BackgroundJobStatus`] and fixes the type of the result payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Scan,
    Render,
}

/// Typed view of a background job, produced from the job registry at the IPC
/// boundary. `kind` is the discriminator and fixes the shape of `result`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackgroundJobStatus {
    Scan {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<ScanReport>,
    },
    Render {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<RenderResult>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub mastering: Option<Box<MasteringReport>>,
    pub asset_id: AssetId,
    pub path: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub clip_count: usize,
    pub range_start_ms: u64,
    pub range_end_ms: u64,
    pub normalized: bool,
    pub track_id: Option<String>,
    pub state: String,
    pub message: String,
}

/// Measured loudness of the completed WAV, with deviations from the requested target.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MasteringReport {
    pub target: riffra_core::LoudnessMastering,
    pub input: LoudnessMeasurement,
    pub output: LoudnessMeasurement,
    pub deviation: LoudnessMeasurement,
    pub normalization_type: String,
    pub true_peak_correction_db: f64,
}

/// Integrated loudness, intersample peak, and loudness range measured by FFmpeg.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessMeasurement {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: f64,
}
