//! RecordingCapture domain model.
//!
//! A [`RecordingCapture`] represents one recording event. The recording itself
//! is the *process*; its products are [`Asset`](riffra_core::Asset)s
//! (raw/processed audio, MIDI). Separating the capture from its products lets
//! the domain reason about partial recovery without conflating it with the
//! produced material.
//!
//! State transitions are defined here and only here. Terminal states
//! (`Completed`, `Recoverable`, `Failed`) cannot return to `Recording`.

use crate::api::output::{DropoutInformation, RecordingCapture, RecordingCaptureStatus};
use riffra_core::DomainError;

impl RecordingCapture {
    /// Starts a new capture in the `Recording` status.
    pub fn start(
        capture_id: impl Into<String>,
        session_id: impl Into<String>,
        started_at_ms: u64,
    ) -> Self {
        Self {
            capture_id: capture_id.into(),
            session_id: session_id.into(),
            status: RecordingCaptureStatus::Recording,
            started_at_ms,
            completed_at_ms: None,
            sample_rate: None,
            input_device: None,
            audio_driver: None,
            input_channel: None,
            input_channel_name: None,
            buffer_size: None,
            master_db: None,
            count_in_beats: None,
            timeline_start_tick: 0,
            armed_track_ids: Vec::new(),
            loop_recording: false,
            recording_session_id: None,
            source: None,
            raw_audio_asset_id: None,
            processed_audio_asset_id: None,
            midi_asset_id: None,
            dropout_information: DropoutInformation::default(),
        }
    }

    /// Returns true if `from -> to` is an allowed transition.
    pub fn allows(from: RecordingCaptureStatus, to: RecordingCaptureStatus) -> bool {
        matches!(
            (from, to),
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Completing
            ) | (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Recoverable
            ) | (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Failed
            ) | (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Completed
            ) | (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Recoverable
            ) | (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Failed
            )
        )
    }

    /// Moves this capture to `to`, enforcing the transition matrix.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidRecordingTransition`] for a disallowed
    /// transition, including any attempt to leave a terminal state.
    pub fn transition(
        &mut self,
        to: RecordingCaptureStatus,
        now_ms: u64,
    ) -> Result<(), DomainError> {
        if !Self::allows(self.status, to) {
            return Err(DomainError::InvalidRecordingTransition {
                from: self.status.as_str().into(),
                to: to.as_str().into(),
            });
        }
        self.status = to;
        if to == RecordingCaptureStatus::Completed {
            self.completed_at_ms = Some(now_ms);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> RecordingCapture {
        RecordingCapture::start("capture:1", "session-1", 1_000)
    }

    #[test]
    fn a_capture_walks_its_documented_states() {
        let mut capture = fresh();
        assert_eq!(capture.status, RecordingCaptureStatus::Recording);

        for target in [
            RecordingCaptureStatus::Completing,
            RecordingCaptureStatus::Recoverable,
            RecordingCaptureStatus::Failed,
        ] {
            let mut capture = fresh();
            assert!(
                capture.transition(target, 2_000).is_ok(),
                "{:?}->{target:?} should be allowed",
                capture.status
            );
        }
    }

    #[test]
    fn a_terminal_capture_cannot_reenter_a_previous_state() {
        let mut capture = fresh();
        capture
            .transition(RecordingCaptureStatus::Completing, 2_000)
            .unwrap();
        // A self transition and a jump straight to Completed are undocumented.
        assert!(
            capture
                .transition(RecordingCaptureStatus::Recording, 3_000)
                .is_err()
        );
        assert!(
            capture
                .transition(RecordingCaptureStatus::Completing, 3_000)
                .is_err()
        );
        capture
            .transition(RecordingCaptureStatus::Completed, 4_000)
            .unwrap();

        assert_eq!(capture.status, RecordingCaptureStatus::Completed);
        assert_eq!(capture.completed_at_ms, Some(4_000));
        assert!(
            capture
                .transition(RecordingCaptureStatus::Recording, 5_000)
                .is_err()
        );
        assert!(
            capture
                .transition(RecordingCaptureStatus::Completing, 5_000)
                .is_err()
        );
    }
}
