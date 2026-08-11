use crate::runtime::error::RuntimeError;
use serde_json::Value;
use std::time::Duration;

/// Port used by the projection coordinator. It contains no Transport
/// operations, so a projection cannot accidentally acquire Transport state.
pub(crate) trait ProjectionDriver: Send + Sync + 'static {
    fn prepare_timeline_snapshot(
        &self,
        snapshot: Value,
        timeout: Duration,
    ) -> Result<(), RuntimeError>;
    fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError>;
    fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError>;
    fn runtime_generation(&self) -> u64;
    fn release_runtime_mute_if_allowed(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn set_processing_mode_passive(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn force_shutdown(&self) {}
}

/// Port used by the Transport executor. It contains no projection operations.
pub(crate) trait TransportDriver: Send + Sync + 'static {
    fn play_timeline(&self) -> Result<(), RuntimeError>;
    fn stop_timeline(&self) -> Result<(), RuntimeError>;

    /// Sends the stop intent without waiting for a native acknowledgement.
    fn stop_timeline_nonblocking(&self) -> Result<(), RuntimeError>;
}

/// Compatibility bound for callers that own one AudioSupervisor for both
/// ports. The implementation remains split at the coordinator/executor
/// boundary.
pub(crate) trait RuntimeDriver: ProjectionDriver + TransportDriver {}

impl<T> RuntimeDriver for T where T: ProjectionDriver + TransportDriver {}
