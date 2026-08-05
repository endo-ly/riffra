//! Native audio facade for the isolated sidecar.
//!
//! The facade owns shared state only. Command acknowledgement, public command
//! construction, process lifecycle, protocol translation, recovery, and
//! Runtime port adaptation live in responsibility-specific sibling modules.

use crate::model::AudioStatus;
use std::sync::atomic::{AtomicBool, Ordering};
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
use recovery::RecoveryState;
use sidecar_process::SidecarProcess;

pub(crate) const SIDECAR_READY_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    command_bus: Arc<CommandBus>,
    process: Arc<SidecarProcess>,
    recovery: Arc<RecoveryState>,
    startup_complete: Arc<AtomicBool>,
}

impl AudioSupervisor {
    pub(crate) fn mark_startup_complete(&self) {
        self.startup_complete.store(true, Ordering::Release);
    }

    pub(crate) fn startup_complete(&self) -> bool {
        self.startup_complete.load(Ordering::Acquire)
    }
}
