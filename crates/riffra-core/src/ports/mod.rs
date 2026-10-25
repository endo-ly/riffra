//! External capabilities required by Core application operations.

use crate::domain::session::CreativeSession;

use thiserror::Error;

/// A failure returned by a host-provided Port.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PortError {
    /// Durable state could not be written.
    #[error("storage operation failed: {0}")]
    Storage(String),
}

/// Durable storage for the canonical production session.
pub trait SessionStorage: Send + Sync {
    /// Writes a complete validated session atomically from the host's point of view.
    ///
    /// # Errors
    /// Returns [`PortError::Storage`] when the host cannot persist the state.
    fn save(&self, session: &CreativeSession) -> Result<(), PortError>;
}

/// Absolute ordering assigned by Core when canonical state is
/// committed. `session_revision` is retained for diagnostics and display, but
/// it is not an ordering key because restore/import may legitimately move it
/// backwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionKey {
    /// Canonical commit sequence assigned by Core.
    pub sequence: u64,
    /// Arrangement revision included for runtime diagnostics.
    pub session_revision: u64,
}
