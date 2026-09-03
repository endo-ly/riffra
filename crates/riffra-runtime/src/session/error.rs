use crate::NativeAudioError;
use riffra_control::{ErrorCode, ProtocolError};
use riffra_core::ApplicationError;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error(
        "canonical state changed: expected sequence {expected_sequence}, current sequence {current_sequence}"
    )]
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
    #[error("active project changed: expected {expected_project_id}, current {current_project_id}")]
    ProjectConflict {
        expected_project_id: String,
        current_project_id: String,
    },
    #[error("{0}")]
    RuntimeUnavailable(String),
    #[error("{0}")]
    CommandFailed(String),
}

impl AdapterError {
    /// Converts this feature-operation failure to the shared Control error.
    pub fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::Conflict {
                expected_sequence,
                current_sequence,
            } => ProtocolError::conflict(*expected_sequence, *current_sequence),
            Self::ProjectConflict {
                expected_project_id,
                current_project_id,
            } => ProtocolError::project_conflict(expected_project_id, current_project_id),
            Self::RuntimeUnavailable(message) => {
                ProtocolError::new(ErrorCode::RuntimeUnavailable, message)
            }
            Self::CommandFailed(message) => ProtocolError::new(ErrorCode::CommandFailed, message),
        }
    }

    pub fn command(message: impl Into<String>) -> Self {
        Self::CommandFailed(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::RuntimeUnavailable(message.into())
    }
}

impl From<ApplicationError> for AdapterError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::Conflict {
                expected_sequence,
                current_sequence,
            } => Self::Conflict {
                expected_sequence,
                current_sequence,
            },
            error => Self::CommandFailed(error.to_string()),
        }
    }
}

impl From<String> for AdapterError {
    fn from(error: String) -> Self {
        Self::CommandFailed(error)
    }
}

impl From<&str> for AdapterError {
    fn from(error: &str) -> Self {
        Self::CommandFailed(error.to_owned())
    }
}

impl From<NativeAudioError> for AdapterError {
    fn from(error: NativeAudioError) -> Self {
        Self::RuntimeUnavailable(error.to_string())
    }
}
