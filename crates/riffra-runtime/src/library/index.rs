use riffra_core::CreativeSession;
use riffra_host::SessionStore;
use std::path::Path;

/// Refreshes the derived Library read model after a canonical commit.
pub fn refresh(data_root: &Path, storage: &SessionStore, session: &CreativeSession) {
    let Ok(project_id) = storage.project_id() else {
        tracing::warn!("library read model refresh skipped: Project ID unavailable");
        return;
    };
    if let Err(error) = super::sync_session(data_root, &project_id, session) {
        tracing::warn!(error = %error, "library read model refresh failed");
    }
}
