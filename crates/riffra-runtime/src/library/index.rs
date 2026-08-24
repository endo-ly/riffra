use riffra_core::CreativeSession;
use std::path::Path;
use std::thread;

/// Refreshes the derived Library read model after a canonical commit.
pub fn queue(data_root: &Path, session: &CreativeSession) {
    let data_root = data_root.to_path_buf();
    let session = session.clone();
    let _ = thread::Builder::new()
        .name("riffra-library-index".into())
        .spawn(move || {
            if let Err(error) = super::sync_session(&data_root, &session) {
                tracing::warn!(error = %error, "library read model refresh failed");
            }
        });
}
