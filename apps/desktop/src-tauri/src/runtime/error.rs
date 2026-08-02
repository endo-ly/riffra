use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone)]
pub(crate) enum RuntimeError {
    Timeout { message: String },
    TransportLost { message: String },
    GenerationChanged { expected: u64, actual: u64 },
    Superseded { message: String },
    Cancelled { message: String },
    NativeRejected(String),
    ShuttingDown,
    Internal(String),
}

impl Display for RuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { message }
            | Self::TransportLost { message }
            | Self::Superseded { message }
            | Self::Cancelled { message }
            | Self::NativeRejected(message)
            | Self::Internal(message) => formatter.write_str(message),
            Self::GenerationChanged { expected, actual } => write!(
                formatter,
                "audio runtime generation changed (expected {expected}, actual {actual})"
            ),
            Self::ShuttingDown => formatter.write_str("native audio runtime is shutting down"),
        }
    }
}

impl std::error::Error for RuntimeError {}
