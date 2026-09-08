//! Native audio facade for the isolated sidecar.
//!
//! The facade owns shared state only. Command acknowledgement, public command
//! construction, process lifecycle, protocol translation, recovery, and
//! Runtime port adaptation live in responsibility-specific sibling modules.

use crate::model::AudioStatus;
use crate::{RuntimeBinaries, SharedHostEventSink};
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

mod command_bus;
mod commands;
mod error;
mod lifecycle;
mod probe;
mod protocol;
mod recovery;
mod runtime_adapter;
mod sidecar_process;

use command_bus::CommandBus;
pub use error::{NativeAudioError, NativeAudioResult};
use probe::ProbeCoordinator;
use recovery::RecoveryState;
pub use recovery::{AudioDeviceReopenOutcome, RuntimeRestartHandler};
use sidecar_process::SidecarProcess;

pub(crate) const SIDECAR_READY_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StartupState {
    Pending = 0,
    Completed = 1,
    Failed = 2,
}

#[derive(Clone)]
struct RecordingCompletion {
    directory: String,
    result: NativeAudioResult<()>,
}

impl StartupState {
    fn from_raw(value: u8) -> Self {
        match value {
            value if value == Self::Completed as u8 => Self::Completed,
            value if value == Self::Failed as u8 => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Clone)]
pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    command_bus: Arc<CommandBus>,
    process: Arc<SidecarProcess>,
    recovery: Arc<RecoveryState>,
    startup_state: Arc<AtomicU8>,
    startup_transition_gate: Arc<Mutex<()>>,
    probe_coordinator: Arc<ProbeCoordinator>,
    binaries: Arc<RuntimeBinaries>,
    events: SharedHostEventSink,
    audio_environment_revision: Arc<AtomicU64>,
    projection_duration_ms: Arc<AtomicU64>,
    recording_completion: Arc<(Mutex<Option<RecordingCompletion>>, Condvar)>,
    recording_finalization_pending: Arc<Mutex<Option<String>>>,
}

impl AudioSupervisor {
    /// Returns the latest native audio status snapshot.
    pub fn status(&self) -> NativeAudioResult<AudioStatus> {
        let mut status = self
            .status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio status",
            })?;
        self.overlay_diagnostics(&mut status);
        Ok(status)
    }

    /// Returns the current native process generation.
    pub fn runtime_generation(&self) -> u64 {
        self.process.current_generation()
    }

    pub fn mark_startup_completed(&self, generation: u64) -> bool {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return false;
        };
        if self.process.current_generation() != generation || self.process.is_terminated(generation)
        {
            return false;
        }
        self.startup_state
            .store(StartupState::Completed as u8, Ordering::Release);
        true
    }

    pub fn mark_startup_failed(&self) {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return;
        };
        self.startup_state
            .store(StartupState::Failed as u8, Ordering::Release);
    }

    pub fn mark_startup_pending(&self) {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return;
        };
        self.startup_state
            .store(StartupState::Pending as u8, Ordering::Release);
    }

    pub fn startup_completed(&self) -> bool {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return false;
        };
        self.startup_state() == StartupState::Completed
    }

    pub fn startup_finished(&self) -> bool {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return false;
        };
        self.startup_state() != StartupState::Pending
    }

    pub fn startup_state(&self) -> StartupState {
        StartupState::from_raw(self.startup_state.load(Ordering::Acquire))
    }

    pub fn audio_environment_revision(&self) -> u64 {
        self.audio_environment_revision.load(Ordering::Acquire)
    }

    pub fn advance_audio_environment(&self) -> u64 {
        let revision = self
            .audio_environment_revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        if let Ok(mut status) = self.status.lock() {
            self.overlay_diagnostics(&mut status);
        }
        revision
    }

    pub(crate) fn record_projection_duration(&self, duration_ms: u64) {
        self.projection_duration_ms
            .store(duration_ms, Ordering::Release);
        if let Ok(mut status) = self.status.lock() {
            self.overlay_diagnostics(&mut status);
        }
    }

    pub(crate) fn overlay_diagnostics(&self, status: &mut AudioStatus) {
        status.diagnostics.audio_environment_revision = self.audio_environment_revision();
        status.diagnostics.projection_duration_ms =
            self.projection_duration_ms.load(Ordering::Acquire);
        if self.recording_finalization_pending() {
            status.recording.active = false;
            status.recording.processing = true;
        }
    }

    pub(super) fn record_recording_completion(
        &self,
        value: &serde_json::Value,
    ) -> NativeAudioResult<()> {
        let directory = value
            .get("directory")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| NativeAudioError::protocol("Recording completion has no directory."))?
            .to_owned();
        let succeeded = value
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let message = value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let result = if succeeded {
            Ok(())
        } else {
            Err(NativeAudioError::structured(
                "recordingProcessing",
                message
                    .clone()
                    .unwrap_or_else(|| "Native recording processing failed.".into()),
                "recording.stop",
                None,
            ))
        };
        let (completion_lock, completion_ready) = &*self.recording_completion;
        let mut completion =
            completion_lock
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Recording completion",
                })?;
        *completion = Some(RecordingCompletion { directory, result });
        completion_ready.notify_all();
        Ok(())
    }

    pub(super) fn fail_recording_completion(&self, error: NativeAudioError) {
        if let Ok(mut completion) = self.recording_completion.0.lock() {
            if completion.is_some() {
                return;
            }
            *completion = Some(RecordingCompletion {
                directory: String::new(),
                result: Err(error),
            });
            self.recording_completion.1.notify_all();
        }
    }

    pub(crate) fn wait_for_recording_completion(&self, directory: &Path) -> NativeAudioResult<()> {
        let expected_directory = directory.to_string_lossy();
        let (completion_lock, completion_ready) = &*self.recording_completion;
        let mut completion =
            completion_lock
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Recording completion",
                })?;
        loop {
            if let Some(recording) = completion.as_ref()
                && (recording.directory.is_empty() || recording.directory == expected_directory)
            {
                let result = recording.result.clone();
                *completion = None;
                return result;
            }
            if self.process.shutting_down.load(Ordering::Acquire) {
                return Err(NativeAudioError::ShuttingDown);
            }
            completion =
                completion_ready
                    .wait(completion)
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Recording completion",
                    })?;
        }
    }

    pub(crate) fn begin_recording_finalization(&self, directory: &Path) -> NativeAudioResult<()> {
        let mut pending = self.recording_finalization_pending.lock().map_err(|_| {
            NativeAudioError::LockPoisoned {
                resource: "Recording finalization",
            }
        })?;
        if pending.is_some() {
            return Err(NativeAudioError::native_rejected(
                "The previous recording is still being finalized.",
            ));
        }
        *pending = Some(directory.to_string_lossy().into_owned());
        Ok(())
    }

    pub(crate) fn finish_recording_finalization(&self) {
        if let Ok(mut pending) = self.recording_finalization_pending.lock() {
            *pending = None;
        }
        if let Ok(mut status) = self.status.lock() {
            self.overlay_diagnostics(&mut status);
        }
    }

    pub(crate) fn recording_finalization_pending(&self) -> bool {
        self.recording_finalization_pending
            .lock()
            .map(|pending| pending.is_some())
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_startup_state_values_are_pending() {
        assert_eq!(StartupState::from_raw(u8::MAX), StartupState::Pending);
    }

    #[test]
    fn startup_completion_requires_the_current_generation() {
        let supervisor = AudioSupervisor::offline("test");
        supervisor
            .startup_state
            .store(StartupState::Pending as u8, Ordering::Release);

        assert!(!supervisor.mark_startup_completed(1));
        assert_eq!(supervisor.startup_state(), StartupState::Pending);
        assert!(supervisor.mark_startup_completed(0));
        assert!(supervisor.startup_completed());
    }

    #[test]
    fn startup_completion_is_rejected_after_generation_termination() {
        let supervisor = AudioSupervisor::offline("test");
        supervisor
            .startup_state
            .store(StartupState::Pending as u8, Ordering::Release);
        let generation = supervisor.process.next_generation();
        supervisor.process.mark_terminated(generation);

        assert!(!supervisor.mark_startup_completed(generation));
        assert_eq!(supervisor.startup_state(), StartupState::Pending);
    }

    #[test]
    fn startup_completion_wins_when_recorded_before_generation_termination() {
        let supervisor = AudioSupervisor::offline("test");
        supervisor
            .startup_state
            .store(StartupState::Pending as u8, Ordering::Release);
        let generation = supervisor.process.next_generation();

        assert!(supervisor.mark_startup_completed(generation));
        supervisor.process.mark_terminated(generation);

        assert!(supervisor.startup_completed());
    }

    #[test]
    fn recording_finalization_is_visible_until_host_promotion_finishes() {
        let supervisor = AudioSupervisor::offline("test");
        let directory = std::path::Path::new("recordings/take-1");

        supervisor
            .begin_recording_finalization(directory)
            .expect("finalization should begin");
        assert!(supervisor.status().unwrap().recording.processing);

        supervisor.finish_recording_finalization();
        assert!(!supervisor.status().unwrap().recording.processing);
    }
}
