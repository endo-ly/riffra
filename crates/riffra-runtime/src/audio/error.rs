use std::fmt::{self, Display, Formatter};

pub type NativeAudioResult<T> = Result<T, NativeAudioError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeAudioError {
    Timeout { message: String },
    TransportLost { message: String },
    GenerationChanged { expected: u64, actual: u64 },
    NativeRejected { message: String },
    Protocol { message: String },
    Process { message: String },
    LockPoisoned { resource: &'static str },
    DeadlineExpired,
    ShuttingDown,
}

impl NativeAudioError {
    pub fn transport_lost(message: impl Into<String>) -> Self {
        Self::TransportLost {
            message: message.into(),
        }
    }

    pub fn native_rejected(message: impl Into<String>) -> Self {
        Self::NativeRejected {
            message: message.into(),
        }
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
        }
    }

    pub fn process(message: impl Into<String>) -> Self {
        Self::Process {
            message: message.into(),
        }
    }

    pub fn requires_restart(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::TransportLost { .. })
    }
}

impl Display for NativeAudioError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { message }
            | Self::TransportLost { message }
            | Self::NativeRejected { message }
            | Self::Protocol { message }
            | Self::Process { message } => formatter.write_str(message),
            Self::GenerationChanged { expected, actual } => write!(
                formatter,
                "Native audio sidecar generation changed from {expected} to {actual}."
            ),
            Self::LockPoisoned { resource } => {
                write!(formatter, "{resource} lock was poisoned.")
            }
            Self::DeadlineExpired => formatter
                .write_str("Audio Runtime recovery deadline expired before the next control step."),
            Self::ShuttingDown => formatter.write_str(
                "Native audio sidecar restart was skipped because the app is shutting down.",
            ),
        }
    }
}

impl std::error::Error for NativeAudioError {}

impl From<NativeAudioError> for String {
    fn from(error: NativeAudioError) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transport_failures_require_a_sidecar_restart() {
        assert!(NativeAudioError::transport_lost("pipe closed").requires_restart());
        assert!(
            NativeAudioError::Timeout {
                message: "acknowledgement timed out".into(),
            }
            .requires_restart()
        );
        assert!(!NativeAudioError::native_rejected("device missing").requires_restart());
    }
}
