//! External capabilities required by Core application operations.

use crate::session::CreativeSession;
use thiserror::Error;

/// A failure returned by a host-provided Port.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PortError {
    /// Durable state could not be written.
    #[error("storage operation failed: {0}")]
    Storage(String),
    /// A runtime projection could not be accepted.
    #[error("runtime projection failed: {0}")]
    Runtime(String),
}

/// Durable storage for the canonical production session.
pub trait SessionStorage: Send + Sync {
    /// Writes a complete validated session atomically from the host's point of view.
    ///
    /// # Errors
    /// Returns [`PortError::Storage`] when the host cannot persist the state.
    fn save(&self, session: &CreativeSession) -> Result<(), PortError>;
}

/// Runtime projection requested after a canonical commit.
pub trait RuntimeProjection: Send + Sync {
    /// Applies a canonical snapshot to the host runtime.
    ///
    /// # Errors
    /// Returns [`PortError::Runtime`] when the projection cannot be accepted.
    fn project(&self, request: RuntimeProjectionRequest) -> Result<(), PortError>;
}

/// Immutable request passed across the Core/runtime boundary.
#[derive(Clone, Debug)]
pub struct RuntimeProjectionRequest {
    session: CreativeSession,
    sequence: u64,
}

impl RuntimeProjectionRequest {
    /// Creates a projection request for a canonical snapshot.
    pub fn new(session: CreativeSession, sequence: u64) -> Self {
        Self { session, sequence }
    }

    /// Returns the session being projected.
    pub fn session(&self) -> &CreativeSession {
        &self.session
    }

    /// Returns the monotonically increasing Core projection sequence.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}
