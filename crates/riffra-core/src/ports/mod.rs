//! External capabilities required by Core application operations.

use crate::domain::session::CreativeSession;

mod projection;

pub use projection::ProjectionKey;
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
