use super::AudioSupervisor;
use super::error::NativeAudioError;
use crate::runtime::{ProjectionDriver, RuntimeError, TransportDriver};
use serde_json::Value;
use std::time::Duration;

impl From<NativeAudioError> for RuntimeError {
    fn from(error: NativeAudioError) -> Self {
        match error {
            NativeAudioError::Timeout { message } => Self::Timeout { message },
            NativeAudioError::TransportLost { message } => Self::TransportLost { message },
            NativeAudioError::GenerationChanged { expected, actual } => {
                Self::GenerationChanged { expected, actual }
            }
            error @ NativeAudioError::DeadlineExpired => Self::Timeout {
                message: error.to_string(),
            },
            NativeAudioError::ShuttingDown => Self::ShuttingDown,
            error => Self::NativeRejected(error.to_string()),
        }
    }
}

impl ProjectionDriver for AudioSupervisor {
    fn prepare_timeline_snapshot(
        &self,
        snapshot: Value,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        AudioSupervisor::prepare_timeline_snapshot(self, snapshot, timeout)
            .map_err(RuntimeError::from)
    }

    fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError> {
        AudioSupervisor::commit_timeline_snapshot(self, timeout).map_err(RuntimeError::from)
    }

    fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), RuntimeError> {
        AudioSupervisor::discard_timeline_snapshot(self, timeout).map_err(RuntimeError::from)
    }

    fn runtime_generation(&self) -> u64 {
        self.sidecar_generation()
    }

    fn release_runtime_mute_if_allowed(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::release_runtime_mute_if_allowed(self).map_err(RuntimeError::from)
    }

    fn force_shutdown(&self) {
        AudioSupervisor::force_shutdown(self);
    }
}

impl TransportDriver for AudioSupervisor {
    fn play_timeline(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::play_timeline(self)
            .map(|_| ())
            .map_err(RuntimeError::from)
    }

    fn stop_timeline(&self) -> Result<(), RuntimeError> {
        AudioSupervisor::stop_timeline(self)
            .map(|_| ())
            .map_err(RuntimeError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_native_timeout_without_mixing_driver_ports() {
        let error = RuntimeError::from(NativeAudioError::Timeout {
            message: "Native audio command acknowledgement timed out".into(),
        });

        assert!(matches!(error, RuntimeError::Timeout { .. }));
    }
}
