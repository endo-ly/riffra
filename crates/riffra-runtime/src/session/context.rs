use crate::{AudioSupervisor, HostEventSink, RuntimeDriver, RuntimeReconciler};
use riffra_core::{AppCore, CreativeSession};
use riffra_host::{ProjectStore, SessionStore};
use std::path::Path;
use std::sync::Mutex;

pub(crate) struct ProjectCommitContext<'a> {
    pub(crate) project_store: &'a ProjectStore,
    pub(crate) command_gate: &'a Mutex<()>,
    pub(crate) expected_project_id: String,
}

/// Concrete dependencies shared by Session application operations.
pub struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub core: &'a AppCore<AudioSupervisor>,
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<D>,
    pub storage: SessionStore,
    pub data_root: &'a Path,
    pub safe_mode: bool,
    pub events: &'a dyn HostEventSink,
    pub(crate) project_commit: Option<ProjectCommitContext<'a>>,
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
