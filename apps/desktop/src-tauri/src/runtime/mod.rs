//! Runtime coordination internals.
//!
//! `ports` contains the two narrow native ports. `projection_coordinator` owns
//! the latest-wins Prepare/Commit/Discard worker, while `transport_executor`
//! applies Core transport decisions to the native runtime.
//! The public application facade is [`RuntimeReconciler`].

pub(crate) mod error;
pub(crate) mod ports;
pub(crate) mod projection_coordinator;
pub(crate) mod reconciler;
pub(crate) mod transport_executor;

pub(crate) use reconciler::RuntimeReconciler;

use std::time::Duration;

pub(crate) const TIMELINE_PREPARE_TIMEOUT: Duration = Duration::from_secs(30);
