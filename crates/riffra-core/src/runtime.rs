use std::path::PathBuf;

/// Platform-independent request for rendering a prepared Timeline snapshot.
pub struct OfflineRenderRequest {
    /// Prepared Timeline graph with resolved source locations.
    pub snapshot: serde_json::Value,
    /// Final WAV destination.
    pub destination: PathBuf,
    /// First Timeline tick included in the output.
    pub start_tick: u64,
    /// Exclusive Timeline tick at the end of the output.
    pub end_tick: u64,
    /// Output sample rate.
    pub sample_rate: u32,
    /// Processing block size used by the offline graph.
    pub block_size: u32,
    /// Master gain applied before optional normalization.
    pub master_gain_db: f64,
    /// Whether the completed output is peak-normalized.
    pub normalize: bool,
}

/// Port for rendering a Timeline without a real-time audio device.
///
/// Process adapters implement the concrete command surface. The production
/// domain does not need to know how a worker is spawned.
pub trait AudioRuntime: Send + Sync {
    /// Renders a prepared Timeline without requiring a real-time audio device.
    ///
    /// # Errors
    /// Returns a host-provided description when the render cannot be completed.
    fn render_timeline_offline(&self, request: OfflineRenderRequest) -> Result<(), String>;
}
