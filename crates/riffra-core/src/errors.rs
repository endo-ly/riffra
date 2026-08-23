use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A production rule violation reported to callers as a structured error.
///
/// The display form is lower-case per repository convention.
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
pub enum DomainError {
    /// An Asset identifier does not use the canonical UUIDv7 form.
    #[error("invalid asset id: {0}")]
    InvalidAssetId(String),
    /// Provenance does not describe a valid source relationship.
    #[error("invalid provenance: {0}")]
    InvalidProvenance(String),
    /// A Timeline Clip violates a production invariant.
    #[error("invalid clip: {0}")]
    InvalidClip(String),
    /// A production operation references a Track that is not registered.
    #[error("unknown track '{0}'")]
    UnknownTrack(String),
    /// Recording capture attempted a disallowed lifecycle transition.
    #[error("recording capture cannot transition from {from} to {to}")]
    InvalidRecordingTransition { from: String, to: String },
}

/// Failure returned by an application operation at the Core boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApplicationError {
    /// A long-running or preconditioned command was based on an old revision.
    #[error(
        "canonical state changed: expected sequence {expected_sequence}, current sequence {current_sequence}"
    )]
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
    /// The requested production edit violates a domain rule.
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    /// The canonical state could not be validated before the commit.
    #[error("invalid session: {0}")]
    InvalidSession(String),
    /// A persistence Port rejected the candidate state.
    #[error("storage operation failed: {0}")]
    Storage(String),
    /// A state lock was poisoned and the operation was not attempted.
    #[error("canonical state lock is poisoned")]
    StateLock,
    /// The runtime rejected a projection after the canonical commit completed.
    #[error("runtime projection failed: {0}")]
    Runtime(String),
    /// No history entry is available for the requested direction.
    #[error("history is empty")]
    HistoryEmpty,
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        Self::InvalidCommand(error.to_string())
    }
}
