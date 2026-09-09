use serde_json::Value;
use std::fmt::{self, Display, Formatter};

pub type NativeAudioResult<T> = Result<T, NativeAudioError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeErrorDescriptor {
    pub kind: String,
    pub message: String,
    pub operation: Option<String>,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeAudioError {
    Timeout {
        message: String,
    },
    TransportLost {
        message: String,
    },
    GenerationChanged {
        expected: u64,
        actual: u64,
    },
    NativeRejected {
        message: String,
    },
    Protocol {
        message: String,
    },
    Process {
        message: String,
    },
    Structured {
        kind: String,
        message: String,
        operation: String,
        details: Option<Value>,
    },
    LockPoisoned {
        resource: &'static str,
    },
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

    pub fn structured(
        kind: impl Into<String>,
        message: impl Into<String>,
        operation: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self::Structured {
            kind: kind.into(),
            message: message.into(),
            operation: operation.into(),
            details,
        }
    }

    pub fn descriptor(&self) -> NativeErrorDescriptor {
        match self {
            Self::Timeout { message } => descriptor("timeout", message),
            Self::TransportLost { message } => descriptor("transportLost", message),
            Self::GenerationChanged { expected, actual } => NativeErrorDescriptor {
                kind: "generationChanged".into(),
                message: format!(
                    "Native audio sidecar generation changed from {expected} to {actual}."
                ),
                operation: None,
                details: Some(serde_json::json!({ "expected": expected, "actual": actual })),
            },
            Self::NativeRejected { message } => descriptor("nativeRejected", message),
            Self::Protocol { message } => descriptor("protocolViolation", message),
            Self::Process { message } => descriptor("process", message),
            Self::Structured {
                kind,
                message,
                operation,
                details,
            } => NativeErrorDescriptor {
                kind: kind.clone(),
                message: message.clone(),
                operation: (!operation.is_empty()).then(|| operation.clone()),
                details: details.clone(),
            },
            Self::LockPoisoned { resource } => {
                descriptor("internal", &format!("{resource} lock was poisoned."))
            }
            Self::DeadlineExpired => descriptor(
                "deadlineExpired",
                "Audio Runtime recovery deadline expired before the next control step.",
            ),
            Self::ShuttingDown => descriptor(
                "shuttingDown",
                "Native audio sidecar restart was skipped because the app is shutting down.",
            ),
        }
    }

    pub fn requires_restart(&self) -> bool {
        matches!(self, Self::TransportLost { .. } | Self::Process { .. })
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
            Self::Structured { message, .. } => formatter.write_str(message),
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

fn descriptor(kind: &str, message: &str) -> NativeErrorDescriptor {
    NativeErrorDescriptor {
        kind: kind.into(),
        message: message.into(),
        operation: None,
        details: None,
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
    fn only_transport_and_process_failures_require_a_sidecar_restart() {
        assert!(NativeAudioError::transport_lost("pipe closed").requires_restart());
        assert!(NativeAudioError::process("process exited").requires_restart());
        assert!(
            !NativeAudioError::Timeout {
                message: "acknowledgement timed out".into(),
            }
            .requires_restart()
        );
        assert!(!NativeAudioError::native_rejected("device missing").requires_restart());
    }
}
