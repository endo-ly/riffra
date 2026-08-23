//! Desktop-facing re-exports of the shared live Runtime.

pub(crate) mod error {
    pub(crate) use riffra_runtime::RuntimeError;
}

pub(crate) mod ports {
    pub(crate) use riffra_runtime::{ProjectionDriver, RuntimeDriver, TransportDriver};
}

pub(crate) mod projection_coordinator {
    pub(crate) use riffra_runtime::{ProjectionStatusHook, RuntimeRecovery};
}

pub(crate) use riffra_runtime::RuntimeReconciler;

pub(crate) const TIMELINE_PREPARE_TIMEOUT: std::time::Duration =
    riffra_runtime::TIMELINE_PREPARE_TIMEOUT;
