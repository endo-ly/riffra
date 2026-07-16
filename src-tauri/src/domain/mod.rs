//! Riffra domain model.
//!
//! The domain owns the canonical production-state types and the rules that keep
//! them consistent. Persistence ([`crate::storage`], [`crate::library`]) and the
//! audio runtime ([`crate::native_audio`]) translate between these types and
//! their storage/wire representations; they never redefine production rules.
//!
//! Layering:
//! ```text
//! asset  ->  rack  ->  session  ->  recording
//! ```
//! `asset` is the foundation; `rack`, `session`, and `recording` build on it.

pub mod asset;
pub mod rack;
pub mod recording;
pub mod session;

use serde::{Deserialize, Serialize};

/// A domain rule violation reported to callers as a structured error.
///
/// The display form is lower-case per repository convention.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DomainError {
    InvalidAssetId(String),
    InvalidProvenance(String),
    InvalidClip(String),
    UnknownTrack(String),
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
