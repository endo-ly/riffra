use riffra_core::CreativeSession;
use std::path::Path;

/// Refreshes the derived Library read model after a canonical commit.
pub fn refresh(data_root: &Path, session: &CreativeSession) {
    if let Err(error) = super::sync_session(data_root, session) {
        tracing::warn!(error = %error, "library read model refresh failed");
    }
}
