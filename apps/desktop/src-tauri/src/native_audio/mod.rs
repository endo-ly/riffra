//! Native audio facade for the isolated sidecar.
//!
//! The facade owns shared state only. Command acknowledgement, public command
//! construction, process lifecycle, protocol translation, recovery, and
//! Runtime port adaptation live in responsibility-specific sibling modules.

use crate::model::AudioStatus;
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
pub struct AudioSupervisor {
    status: Arc<Mutex<AudioStatus>>,
    command_bus: Arc<CommandBus>,
    process: Arc<SidecarProcess>,
    recovery: Arc<RecoveryState>,
}
