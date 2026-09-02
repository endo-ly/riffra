use crate::model::{
    ProjectActivationResult, ProjectRecoveryState, ProjectState, ProjectSummary, RecoveryCandidate,
};
use riffra_core::{CanonicalState, CreativeSession};
use riffra_host::{ProjectStore, SessionStore};
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

pub(crate) fn state(project_store: &ProjectStore) -> Result<ProjectState, String> {
    Ok(ProjectState {
        active_project_id: project_store
            .active_project_id()
            .map_err(|error| error.to_string())?,
        projects: project_store
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(ProjectSummary::from)
            .collect(),
    })
}

pub(crate) fn activate<A>(
    project_store: &ProjectStore,
    project_id: &str,
    activate_core: impl FnOnce(CreativeSession) -> Result<(), A>,
) -> Result<riffra_host::LoadedSession, String>
where
    A: std::fmt::Display,
{
    let loaded = project_store
        .load(project_id)
        .map_err(|error| error.to_string())?;
    let previous_project_id = project_store
        .set_active(project_id)
        .map_err(|error| error.to_string())?;
    if let Err(error) = activate_core(loaded.session.clone()) {
        let rollback = project_store.set_active(&previous_project_id);
        return Err(match rollback {
            Ok(_) => error.to_string(),
            Err(rollback_error) => format!(
                "Project activation failed: {error}; active Project rollback failed: {rollback_error}"
            ),
        });
    }
    Ok(loaded)
}

pub(crate) fn recovery(
    storage: &SessionStore,
    recovered_from_generation: bool,
) -> Result<ProjectRecoveryState, String> {
    Ok(ProjectRecoveryState {
        recovered_from_generation,
        recovery_candidates: if recovered_from_generation {
            storage
                .recovery_candidates()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(RecoveryCandidate::from)
                .collect()
        } else {
            Vec::new()
        },
    })
}

pub(crate) fn activation_result(
    project_state: ProjectState,
    canonical: CanonicalState,
    storage: &SessionStore,
    recovered_from_generation: bool,
) -> Result<ProjectActivationResult, String> {
    Ok(ProjectActivationResult {
        project_state,
        canonical,
        recovery: recovery(storage, recovered_from_generation)?,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub path: String,
    pub session_id: String,
    pub exported_at_ms: u64,
    pub asset_count: usize,
}

pub fn export(
    data_root: &Path,
    session: &CreativeSession,
    exported_at_ms: u64,
    output: &Path,
) -> Result<ProjectExport, String> {
    riffra_host::export_project(data_root, session, exported_at_ms, output).map(|result| {
        ProjectExport {
            path: result.path,
            session_id: result.session_id,
            exported_at_ms: result.exported_at_ms,
            asset_count: result.asset_count,
        }
    })
}

pub fn import(data_root: &Path, path: &Path) -> Result<CreativeSession, String> {
    riffra_host::import_project(data_root, path)
}
