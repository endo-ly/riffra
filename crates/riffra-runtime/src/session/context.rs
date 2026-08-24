use crate::{AudioSupervisor, HostEventSink, RuntimeDriver, RuntimeReconciler};
use riffra_core::{AppCore, CreativeSession};
use std::path::Path;

/// Concrete dependencies shared by Session application operations.
pub struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub core: &'a AppCore<AudioSupervisor>,
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<D>,
    pub data_root: &'a Path,
    pub safe_mode: bool,
    pub events: &'a dyn HostEventSink,
}

pub fn current_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<CreativeSession, String> {
    context
        .core
        .snapshot()
        .map(|snapshot| snapshot.session)
        .map_err(|error| error.to_string())
}
