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
    /// A single job that produces several aligned Track stems.
    RenderStems,
    /// Captures multiple mix outputs in a single offline render.
    RenderCapture,
    /// Measures existing WAV files without editing a Project.
    AnalysisMeasure,
    /// Measures saved Render audio on a musical grid.
    AnalysisLevels,
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
        result: Option<Box<RenderResult>>,
    },
    /// One job that produces a set of aligned Track stems.
    RenderStems {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<RenderStemsResult>,
    },
    RenderCapture {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<RenderCaptureResult>,
    },
    AnalysisMeasure {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<AnalysisMeasureResult>,
    },
    AnalysisLevels {
        id: String,
        state: JobState,
        progress: Option<f32>,
        message: String,
        result: Option<AnalysisLevelsResult>,
    },
}

/// RMS and sample peak windows for every selected Render source.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisLevelsResult {
    pub render_id: String,
    pub grid: riffra_core::MusicalDuration,
    pub sample_rate: u32,
    pub start: riffra_core::MusicalPosition,
    pub end: riffra_core::MusicalPosition,
    pub start_tick: u64,
    pub end_tick: u64,
    pub sources: Vec<AnalysisLevelsSource>,
}

/// One Render source and its aligned measurement windows.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisLevelsSource {
    pub source: crate::api::params::LevelSource,
    pub windows: Vec<AnalysisLevelWindow>,
}

/// Measurements of one half-open musical window; silence is represented by null.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisLevelWindow {
    pub start: riffra_core::MusicalPosition,
    pub end: riffra_core::MusicalPosition,
    pub start_tick: u64,
    pub end_tick: u64,
    pub frames: u64,
    pub rms_dbfs: Option<f64>,
    pub peak_dbfs: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub measurements: AudioMeasurements,
    pub warnings: Vec<String>,
    pub mastering: Option<Box<MasteringReport>>,
    pub asset_id: AssetId,
    /// Project that supplied the canonical state, when rendered from a Project.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Timestamp used to identify this managed Render directory.
    #[serde(default)]
    pub created_at_ms: Option<u64>,
    /// Internal WAV path registered as an Asset.
    pub path: String,
    /// External WAV copy requested for this job, when present.
    #[serde(default)]
    pub exported_path: Option<String>,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub clip_count: usize,
    /// Requested range kind: `entireArrangement`, `loopRange`, `timeSelection`, or `section`.
    pub range_kind: String,
    pub range_start_ms: u64,
    pub range_end_ms: u64,
    pub normalized: bool,
    pub track_id: Option<String>,
    /// Score section name for a section render.
    #[serde(default)]
    pub section_name: Option<String>,
    /// One-based Score section occurrence for a section render.
    #[serde(default)]
    pub section_occurrence: Option<u32>,
    pub pacing: RenderPacing,
    pub state: String,
    pub message: String,
}

/// Pacing a render job ran with, derived from the plugins its signal path loads.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderPacing {
    pub render_mode: riffra_core::RenderMode,
    /// Seconds hosted VST3 plugins were processed before the render started: the longest warm-up
    /// of `realtimePlugins`, or 0 without any.
    pub plugin_warm_up_seconds: f64,
    /// Plugins in the signal path that need wall-clock processing, which is why the render is
    /// real-time.
    pub realtime_plugins: Vec<String>,
}

/// Result metadata for an aligned set of rendered Track stems.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderStemsResult {
    /// Directory containing the WAV files and `render.json`.
    pub directory: String,
    /// External directory containing the copied stem WAVs, when requested.
    #[serde(default)]
    pub exported_directory: Option<String>,
    /// Project that supplied the rendered canonical state.
    pub project_id: String,
    /// Timestamp used to identify this managed Render directory.
    #[serde(default)]
    pub created_at_ms: Option<u64>,
    /// Canonical sequence captured when the job started.
    pub source_sequence: u64,
    /// Shared WAV sample rate.
    pub sample_rate: u32,
    /// Shared start Tick.
    pub start_tick: u64,
    /// Shared end Tick before render tail.
    pub end_tick: u64,
    /// Requested range kind: `entireArrangement`, `loopRange`, `timeSelection`, or `section`.
    pub range_kind: String,
    /// Shared frame count, including any render tail.
    pub frames: u64,
    /// Stems in canonical Track order.
    pub stems: Vec<RenderStem>,
    /// Tracks skipped by an all-Tracks request because they had no unmuted Clips.
    pub skipped_track_ids: Vec<String>,
    /// Score section name for a section render.
    #[serde(default)]
    pub section_name: Option<String>,
    /// One-based Score section occurrence for a section render.
    #[serde(default)]
    pub section_occurrence: Option<u32>,
    pub pacing: RenderPacing,
}

/// One post-processing output captured from an offline render.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderCapture {
    pub target: crate::api::params::CaptureTarget,
    pub name: String,
    pub bus_kind: Option<riffra_core::MixBusKind>,
    pub asset_id: AssetId,
    pub path: String,
    pub measurements: AudioMeasurements,
    pub warnings: Vec<String>,
}

/// Aligned post-processing outputs from one canonical arrangement snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderCaptureResult {
    pub render_id: String,
    pub project_id: String,
    pub created_at_ms: u64,
    pub source_sequence: u64,
    pub sample_rate: u32,
    pub start_tick: u64,
    pub end_tick: u64,
    pub frames: u64,
    pub captures: Vec<RenderCapture>,
    pub exported_directory: Option<String>,
    pub pacing: RenderPacing,
}

/// Result of copying an existing Render without changing its internal files.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderExportResult {
    pub render_id: String,
    pub source_path: String,
    pub output_path: String,
    pub file_count: usize,
}

/// Render kind represented by one managed Render directory.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RenderKind {
    Timeline,
    Stems,
    Capture,
    Unknown,
}

/// File and Asset consistency for one WAV in the Render store.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderFileStatus {
    pub path: String,
    pub asset_id: Option<AssetId>,
    pub file_exists: bool,
    pub asset_registered: bool,
    pub file_size_bytes: Option<u64>,
}

/// One item in the Render history, including diagnostics for damaged output.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderHistoryEntry {
    pub render_id: String,
    pub kind: RenderKind,
    /// Requested range kind recorded by the render that created this entry.
    pub range_kind: Option<String>,
    /// Score section name for a section render.
    pub section_name: Option<String>,
    /// One-based Score section occurrence for a section render.
    pub section_occurrence: Option<u32>,
    pub project_id: Option<String>,
    pub created_at_ms: Option<u64>,
    pub files: Vec<RenderFileStatus>,
    pub file_count: usize,
    pub file_size_bytes: Option<u64>,
    pub sample_rate: Option<u32>,
    pub duration_ms: Option<u64>,
    pub measurements: Vec<AudioMeasurements>,
    pub diagnostics: Vec<String>,
}

/// Result of removing a managed Render and its Asset registrations.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderDeleteResult {
    pub render_id: String,
    pub deleted_file_count: usize,
}

/// The WAV and source Track represented by one stem result.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderStem {
    /// Measurements of this completed WAV; no individual loudness mastering is applied.
    pub measurements: AudioMeasurements,
    pub warnings: Vec<String>,
    /// Canonical Track ID.
    pub track_id: String,
    /// Track name at render time.
    pub track_name: String,
    /// Asset registered for this WAV.
    pub asset_id: AssetId,
    /// Full path to the WAV file.
    pub path: String,
    /// Track gain in dB already applied to the stem.
    pub gain_db: f64,
    /// Track pan already applied to the stem.
    pub pan: f64,
    /// Group Bus the Track fed at render time; `None` for the Master.
    pub output_bus_id: Option<String>,
}

/// Measured loudness of the completed WAV, with deviations from the requested target.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MasteringReport {
    pub target: riffra_core::LoudnessMastering,
    pub input: LoudnessMeasurement,
    pub deviation: MasteringDeviation,
    pub normalization_type: String,
    pub true_peak_correction_db: f64,
}

/// Differences from configured targets; LRA is absent when no target was requested.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MasteringDeviation {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: Option<f64>,
}

/// Integrated loudness, intersample peak, and loudness range measured by FFmpeg.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessMeasurement {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: f64,
}

/// Measurements taken from the completed WAV, without re-rendering its graph.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeasurements {
    /// Actual PCM sample peak. `None` represents digital silence (negative infinity).
    pub sample_peak_dbfs: Option<f64>,
    pub loudness: LoudnessReadout,
    pub section_loudness_state: MeasurementState,
    /// Per-marker sections; empty when the session has no marker.
    pub sections: Vec<SectionMeasurement>,
    /// Per-Score section occurrences; empty when the Project is not linked to a Score.
    #[serde(default)]
    pub score_sections: Vec<ScoreSectionMeasurement>,
}

/// Measurements of one Score section occurrence inside the rendered WAV.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSectionMeasurement {
    pub name: String,
    /// One-based occurrence among Score regions with the same name.
    pub occurrence: u32,
    #[ts(type = "number")]
    pub start_tick: u64,
    #[ts(type = "number")]
    pub end_tick: u64,
    /// Seconds from the start of the WAV.
    pub start_seconds: f64,
    /// Seconds from the start of the WAV, excluding the render tail.
    pub end_seconds: f64,
    /// Whether the whole Score region was inside the rendered range.
    pub complete: bool,
    /// `None` when section loudness measurement is unavailable.
    pub loudness: Option<SectionLoudness>,
    pub bands: BandLevels,
}

/// FFmpeg loudness measurements, or the reason they could not be obtained.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LoudnessReadout {
    Measured { measurement: LoudnessMeasurement },
    Unavailable { reason: String },
}

/// Measurements of one marker section, including the final render tail.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SectionMeasurement {
    /// Marker name; `None` for the part before the first marker.
    pub name: Option<String>,
    pub start: riffra_core::MusicalPosition,
    pub end: riffra_core::MusicalPosition,
    /// Seconds from the start of the WAV.
    pub start_seconds: f64,
    /// Seconds from the start of the WAV.
    pub end_seconds: f64,
    /// `None` when section loudness measurement is unavailable.
    pub loudness: Option<SectionLoudness>,
    pub bands: BandLevels,
}

/// Gated integrated loudness and frame true peak of a section.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SectionLoudness {
    /// `None` when no complete 400 ms block passes the gates.
    pub integrated_lufs: Option<f64>,
    /// `None` for digital silence.
    pub true_peak: Option<SectionPeak>,
}

/// True peak with the location of its 100 ms frame.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SectionPeak {
    pub db: f64,
    /// Start of the peak's 100 ms frame, in seconds from the start of the WAV.
    pub seconds: f64,
    pub position: riffra_core::MusicalPosition,
}

/// RMS level per band in dBFS; `None` for silence.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BandLevels {
    pub low_db: Option<f64>,
    pub mid_db: Option<f64>,
    pub high_db: Option<f64>,
}

/// Completion of an optional measurement, independent of silent values.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum MeasurementState {
    Measured,
    Unavailable { reason: String },
}

/// True peak location relative to the measured WAV.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RangePeak {
    pub db: f64,
    pub seconds: f64,
}

/// Gated loudness and frame peak without musical coordinates.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RangeLoudness {
    pub integrated_lufs: Option<f64>,
    pub true_peak: Option<RangePeak>,
}

/// Measurements of a half-open interval relative to the WAV start.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSectionMeasurement {
    pub name: Option<String>,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub loudness: Option<RangeLoudness>,
    pub bands: BandLevels,
}

/// Measurements and identification of one existing WAV.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisMeasuredSource {
    pub path: String,
    pub track_id: Option<String>,
    pub track_name: Option<String>,
    pub sample_rate: u32,
    pub duration_seconds: f64,
    pub sample_peak_dbfs: Option<f64>,
    pub loudness: LoudnessReadout,
    pub section_loudness_state: MeasurementState,
    pub sections: Vec<AnalysisSectionMeasurement>,
}

/// Existing-WAV measurements in request order.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisMeasureResult {
    pub source_render_id: Option<String>,
    pub sources: Vec<AnalysisMeasuredSource>,
}
