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
    fn allows_every_documented_transition() {
        for (from, to) in [
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Completing,
            ),
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Recoverable,
            ),
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Failed,
            ),
            (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Completed,
            ),
            (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Recoverable,
            ),
            (
                RecordingCaptureStatus::Completing,
                RecordingCaptureStatus::Failed,
            ),
        ] {
            assert!(
                RecordingCapture::allows(from, to),
                "{from:?}->{to:?} should be allowed"
            );
        }
    }

    #[test]
    fn rejects_undocumented_transitions() {
        let disallowed = [
            (
                RecordingCaptureStatus::Completed,
                RecordingCaptureStatus::Recording,
            ),
            (
                RecordingCaptureStatus::Recoverable,
                RecordingCaptureStatus::Recording,
            ),
            (
                RecordingCaptureStatus::Failed,
                RecordingCaptureStatus::Recording,
            ),
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Completed,
            ),
            (
                RecordingCaptureStatus::Completed,
                RecordingCaptureStatus::Completing,
            ),
            (
                RecordingCaptureStatus::Recording,
                RecordingCaptureStatus::Recording,
            ),
        ];
        for (from, to) in disallowed {
            assert!(
                !RecordingCapture::allows(from, to),
                "{from:?}->{to:?} should be rejected"
            );
        }
    }

    #[test]
    fn completion_records_time_and_terminal_states_cannot_reenter() {
        let mut capture = fresh();
        capture
            .transition(RecordingCaptureStatus::Completing, 2_000)
            .unwrap();
        assert_eq!(capture.status, RecordingCaptureStatus::Completing);
        assert_eq!(capture.completed_at_ms, None);
        capture
            .transition(RecordingCaptureStatus::Completed, 3_000)
            .unwrap();
        assert_eq!(capture.status, RecordingCaptureStatus::Completed);
        assert_eq!(capture.completed_at_ms, Some(3_000));

        let mut failed_capture = fresh();
        failed_capture
            .transition(RecordingCaptureStatus::Failed, 2_000)
            .unwrap();
        let error = failed_capture
            .transition(RecordingCaptureStatus::Recording, 3_000)
            .unwrap_err();
        assert!(matches!(
            error,
            DomainError::InvalidRecordingTransition { .. }
        ));
        assert_eq!(failed_capture.status, RecordingCaptureStatus::Failed);
    }
}
