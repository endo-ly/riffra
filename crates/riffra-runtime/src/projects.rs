use crate::api::output::{
    ProjectExport, ProjectRecoveryState, ProjectState, ProjectSummary, RecoveryCandidate,
};
use riffra_core::CreativeSession;
use riffra_host::{LoadedSession, ProjectStore, SessionStore};
use std::path::Path;

pub(crate) fn state(
    project_store: &ProjectStore,
    project_id: &str,
) -> Result<ProjectState, String> {
    Ok(ProjectState {
        active_project_id: project_id.to_owned(),
        projects: project_store
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(ProjectSummary::from)
            .collect(),
    })
}

pub(crate) struct PreparedActivation {
    pub loaded: LoadedSession,
    pub storage: SessionStore,
    pub project_state: ProjectState,
    pub recovery: ProjectRecoveryState,
}

pub(crate) fn prepare(
    project_store: &ProjectStore,
    project_id: &str,
) -> Result<PreparedActivation, String> {
    let loaded = project_store
        .load(project_id)
        .map_err(|error| error.to_string())?;
    let storage = project_store
        .session_store(project_id)
        .map_err(|error| error.to_string())?;
    let project_state = ProjectState {
        active_project_id: project_id.to_owned(),
        projects: project_store
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(ProjectSummary::from)
            .collect(),
    };
    let recovery = recovery(&storage, loaded.recovered_from_generation)?;
    Ok(PreparedActivation {
        loaded,
        storage,
        project_state,
        recovery,
    })
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

impl From<riffra_host::ProjectSummary> for ProjectSummary {
    fn from(summary: riffra_host::ProjectSummary) -> Self {
        Self {
            project_id: summary.project_id,
            name: summary.name,
            updated_at_ms: summary.updated_at_ms,
            error: summary.error,
        }
    }
}

impl From<riffra_host::RecoveryCandidate> for RecoveryCandidate {
    fn from(candidate: riffra_host::RecoveryCandidate) -> Self {
        Self {
            file_name: candidate.file_name,
            updated_at_ms: candidate.updated_at_ms,
            session_id: candidate.session_id,
            project_name: candidate.project_name,
            note: candidate.note,
        }
    }
}
