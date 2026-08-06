//! Native audio facade for the isolated sidecar.
//!
//! The facade owns shared state only. Command acknowledgement, public command
//! construction, process lifecycle, protocol translation, recovery, and
//! Runtime port adaptation live in responsibility-specific sibling modules.

use crate::model::AudioStatus;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod command_bus;
mod commands;
mod error;
mod lifecycle;
mod protocol;
mod recovery;
mod runtime_adapter;
mod sidecar_process;

use command_bus::CommandBus;
pub use commands::NativeSamplePad;
pub(crate) use error::{NativeAudioError, NativeAudioResult};
use recovery::RecoveryState;
use sidecar_process::SidecarProcess;

pub(crate) const SIDECAR_READY_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum StartupState {
    Pending = 0,
    Completed = 1,
    Failed = 2,
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
}

impl AudioSupervisor {
    pub(crate) fn mark_startup_completed(&self, generation: u64) -> bool {
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

    pub(crate) fn mark_startup_failed(&self) {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return;
        };
        self.startup_state
            .store(StartupState::Failed as u8, Ordering::Release);
    }

    pub(crate) fn mark_startup_pending(&self) {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return;
        };
        self.startup_state
            .store(StartupState::Pending as u8, Ordering::Release);
    }

    pub(crate) fn startup_completed(&self) -> bool {
        let Ok(_transition) = self.startup_transition_gate.lock() else {
            return false;
        };
        self.startup_state() == StartupState::Completed
    }

    pub(crate) fn startup_state(&self) -> StartupState {
        StartupState::from_raw(self.startup_state.load(Ordering::Acquire))
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
}
