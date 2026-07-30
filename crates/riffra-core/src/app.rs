use crate::{AudioRuntime, session::CreativeSession};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Platform-independent state shared by Riffra application hosts.
pub struct AppCore<A: AudioRuntime> {
    data_root: PathBuf,
    session: Mutex<CreativeSession>,
    audio: A,
    recovered_from_generation: bool,
    safe_mode: bool,
}

impl<A: AudioRuntime> AppCore<A> {
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
            session: Mutex::new(session),
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
        &self.session
    }

    /// Returns the host-provided audio runtime.
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
    use crate::{OfflineRenderRequest, session::CreativeSession};

    struct OfflineRuntime;

    impl AudioRuntime for OfflineRuntime {
        fn render_timeline_offline(&self, _request: OfflineRenderRequest) -> Result<(), String> {
            Err("offline runtime has no render worker".into())
        }
    }

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
