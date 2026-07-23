//! RecordingCapture domain model.
//!
//! A [`RecordingCapture`] represents one recording event. The recording itself
//! is the *process*; its products are [`Asset`](crate::asset::Asset)s
//! (raw/processed audio, MIDI). Separating the capture from its products lets
//! the domain reason about partial recovery without conflating it with the
//! produced material.
//!
//! State transitions are defined here and only here. Terminal states
//! (`Completed`, `Recoverable`, `Failed`) cannot return to `Recording`.

use crate::asset::AssetId;
use crate::errors::DomainError;
use crate::rack::RackDevice;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The status of a [`RecordingCapture`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RecordingCaptureStatus {
    Recording,
    Completing,
    Completed,
    Recoverable,
    Failed,
}

impl RecordingCaptureStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Completing => "completing",
            Self::Completed => "completed",
            Self::Recoverable => "recoverable",
            Self::Failed => "failed",
        }
    }
}

/// Dropout/drop diagnostics captured during recording.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DropoutInformation {
    #[serde(default)]
    pub samples_written: u64,
    #[serde(default)]
    pub dropped_blocks: u64,
    #[serde(default)]
    pub missing_samples: u64,
    #[serde(default)]
    pub dropout_start_sample: Option<u64>,
    #[serde(default)]
    pub dropout_end_sample: Option<u64>,
}

/// One recording event and the assets it produced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingCapture {
    pub capture_id: String,
    pub session_id: String,
    pub status: RecordingCaptureStatus,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input_device: Option<String>,
    pub audio_driver: Option<String>,
    pub input_channel: Option<u32>,
    pub input_channel_name: Option<String>,
    pub buffer_size: Option<u32>,
    /// Session context captured at recording start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub master_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_in_beats: Option<u8>,
    #[serde(default)]
    #[ts(type = "number")]
    pub timeline_start_tick: u64,
    #[serde(default)]
    pub armed_track_ids: Vec<String>,
    #[serde(default)]
    pub loop_recording: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
    #[serde(default)]
    pub rack_snapshot: Vec<RackDevice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub raw_audio_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub processed_audio_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub midi_asset_id: Option<AssetId>,
    #[serde(default)]
    pub dropout_information: DropoutInformation,
}

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
            workspace: None,
            master_db: None,
            count_in_beats: None,
            timeline_start_tick: 0,
            armed_track_ids: Vec::new(),
            loop_recording: false,
            recording_session_id: None,
            source: None,
            rack_snapshot: Vec::new(),
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
        RecordingCapture::start("capture:1", "scratch-1", 1_000)
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
    fn recording_to_completing_to_completed_records_completion_time() {
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
    }

    #[test]
    fn terminal_states_cannot_return_to_recording() {
        let mut capture = fresh();
        capture
            .transition(RecordingCaptureStatus::Failed, 2_000)
            .unwrap();
        let error = capture
            .transition(RecordingCaptureStatus::Recording, 3_000)
            .unwrap_err();
        assert!(matches!(
            error,
            DomainError::InvalidRecordingTransition { .. }
        ));
        assert_eq!(capture.status, RecordingCaptureStatus::Failed);
    }
}
