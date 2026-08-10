use crate::session::CreativeSession;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Platform-independent state shared by Riffra application hosts.
pub struct AppCore<A> {
    data_root: PathBuf,
    session: Arc<Mutex<CreativeSession>>,
    audio: A,
    recovered_from_generation: bool,
    safe_mode: bool,
}

impl<A> AppCore<A> {
    /// Creates application state from an already-loaded canonical session.
    pub fn new(
        data_root: PathBuf,
        session: CreativeSession,
        audio: A,
        recovered_from_generation: bool,
        safe_mode: bool,
    ) -> Self {
        Self {
            data_root,
            session: Arc::new(Mutex::new(session)),
            audio,
            recovered_from_generation,
            safe_mode,
        }
    }

    /// Returns the root used for durable application data.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Returns the lock protecting the canonical production session.
    pub fn session(&self) -> &Mutex<CreativeSession> {
        self.session.as_ref()
    }

    /// Returns the shared canonical session handle for recovery workers that
    /// outlive a single application command.
    pub fn shared_session(&self) -> Arc<Mutex<CreativeSession>> {
        Arc::clone(&self.session)
    }

    /// Returns the host-provided live audio service.
    pub fn audio(&self) -> &A {
        &self.audio
    }

    /// Reports whether startup restored a recovery generation.
    pub fn recovered_from_generation(&self) -> bool {
        self.recovered_from_generation
    }

    /// Reports whether external devices and plugins are isolated.
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::CreativeSession;

    struct OfflineRuntime;

    #[test]
    fn owns_canonical_state_without_platform_services() {
        let core = AppCore::new(
            PathBuf::from("data"),
            CreativeSession::new(1),
            OfflineRuntime,
            true,
            true,
        );

        assert_eq!(core.data_root(), Path::new("data"));
        assert!(core.recovered_from_generation());
        assert!(core.safe_mode());
        assert_eq!(core.session().lock().unwrap().session_id, "scratch-1");
    }
}
