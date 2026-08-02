//! Runtime coordination internals.
//!
//! `model` contains ordering/value types shared by the coordinators. `ports`
//! contains the two narrow native ports. `projection_coordinator` owns the
//! latest-wins Prepare/Commit/Discard worker, while `transport_controller` and
//! `transport_executor` own transport intent and native transport execution.
//! The public application facade is [`RuntimeReconciler`].

pub(crate) mod error;
pub(crate) mod model;
pub(crate) mod ports;
pub(crate) mod projection_coordinator;
pub(crate) mod reconciler;
pub(crate) mod transport_controller;
pub(crate) mod transport_executor;

pub(crate) use reconciler::RuntimeReconciler;
