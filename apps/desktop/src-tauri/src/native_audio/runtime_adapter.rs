use crate::native_audio::AudioSupervisor;
use crate::runtime::error::RuntimeError;
use crate::runtime::ports::{ProjectionDriver, TransportDriver};
use serde_json::Value;
use std::time::Duration;

/// Converts the legacy String-based AudioSupervisor boundary into the typed
/// error contract used by the Runtime ports. The conversion belongs to the
/// native adapter, not to the projection or transport coordinators.
pub(crate) fn map_native_error(message: String) -> RuntimeError {
    if message.contains("did not acknowledge") || message.contains("graph boundary") {
        return RuntimeError::Timeout { message };
    }
    if crate::native_audio::is_transport_loss_error(&message) {
        return RuntimeError::TransportLost { message };
    }
    if message.to_ascii_lowercase().contains("shutting down") {
        return RuntimeError::ShuttingDown;
    }
    RuntimeError::NativeRejected(message)
}

impl ProjectionDriver for AudioSupervisor {
    fn prepare_timeline_snapshot(
        &self,
        snapshot: Value,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        AudioSupervisor::prepare_timeline_snapshot(self, snapshot, timeout)
            .map_err(map_native_error)
    }

    fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError> {
        AudioSupervisor::commit_timeline_snapshot(self, timeout).map_err(map_native_error)
    }

    fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError> {
        AudioSupervisor::discard_timeline_snapshot(self, timeout).map_err(map_native_error)
    }

    fn runtime_generation(&self) -> u64 {
        self.sidecar_generation()
    }

    fn force_shutdown(&self) {
        AudioSupervisor::force_shutdown(self);
    }
}

impl TransportDriver for AudioSupervisor {
    fn play_timeline(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::play_timeline(self)
            .map(|_| ())
            .map_err(map_native_error)
    }

    fn stop_timeline(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::stop_timeline(self)
            .map(|_| ())
            .map_err(map_native_error)
    }

    fn stop_timeline_nonblocking(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::stop_timeline_nonblocking(self).map_err(map_native_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_native_timeout_without_mixing_driver_ports() {
        let message = "Native audio did not acknowledge the command within 15 seconds.";

        let error = map_native_error(message.into());

        assert!(matches!(error, RuntimeError::Timeout { .. }));
    }
}
