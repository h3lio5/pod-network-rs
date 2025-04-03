#[derive(thiserror::Error, Debug)]
pub enum PodError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Insufficient votes")]
    InsufficientVotes,
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Protocol violation: {0}")]
    ProtocolViolation(String, String),
    #[error("Invalid configuration")]
    InvalidConfig,
    #[error("Unknown transaction with ID: {0}")]
    UnknownTransaction(String),
}
