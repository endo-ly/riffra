use super::RuntimeError;
use serde_json::Value;
use std::time::Duration;

/// Port used by the projection coordinator. It contains no Transport
/// operations, so a projection cannot accidentally acquire Transport state.
pub trait ProjectionDriver: Send + Sync + 'static {
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
    fn force_shutdown(&self) {}
}

/// Port used by the Transport executor. It contains no projection operations.
pub trait TransportDriver: Send + Sync + 'static {
    fn play_timeline(&self) -> Result<(), RuntimeError>;
    fn stop_timeline(&self) -> Result<(), RuntimeError>;
}

/// Combined runtime bound for a driver used by both projection and transport.
/// The implementation remains split at the coordinator/executor boundary.
pub trait RuntimeDriver: ProjectionDriver + TransportDriver {}

impl<T> RuntimeDriver for T where T: ProjectionDriver + TransportDriver {}
