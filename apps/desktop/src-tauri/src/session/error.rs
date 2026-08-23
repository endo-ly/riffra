use crate::native_audio::NativeAudioError;
use riffra_core::ApplicationError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterError {
    #[error(
        "canonical state changed: expected sequence {expected_sequence}, current sequence {current_sequence}"
    )]
    Conflict {
        expected_sequence: u64,
        current_sequence: u64,
    },
    #[error("{0}")]
    RuntimeUnavailable(String),
    #[error("{0}")]
    CommandFailed(String),
}

impl AdapterError {
    pub(crate) fn command(message: impl Into<String>) -> Self {
        Self::CommandFailed(message.into())
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
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
