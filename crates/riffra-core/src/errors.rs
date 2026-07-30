use serde::{Deserialize, Serialize};

/// A production rule violation reported to callers as a structured error.
///
/// The display form is lower-case per repository convention.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DomainError {
    /// An Asset identifier does not use the canonical UUIDv7 form.
    InvalidAssetId(String),
    /// Provenance does not describe a valid source relationship.
    InvalidProvenance(String),
    /// A Timeline Clip violates a production invariant.
    InvalidClip(String),
    /// A production operation references a Track that is not registered.
    UnknownTrack(String),
    /// Recording capture attempted a disallowed lifecycle transition.
    InvalidRecordingTransition { from: String, to: String },
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAssetId(message) => {
                write!(formatter, "invalid asset id: {message}")
            }
            Self::InvalidProvenance(message) => {
                write!(formatter, "invalid provenance: {message}")
            }
            Self::InvalidClip(message) => write!(formatter, "invalid clip: {message}"),
            Self::UnknownTrack(track_id) => {
                write!(formatter, "unknown track '{track_id}'")
            }
            Self::InvalidRecordingTransition { from, to } => {
                write!(
                    formatter,
                    "recording capture cannot transition from {from} to {to}"
                )
            }
        }
    }
}

impl std::error::Error for DomainError {}
