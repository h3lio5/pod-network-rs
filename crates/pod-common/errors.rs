#[derive(thiserror::Error, Debug)]
pub enum PodError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Insufficient votes")]
    InsufficientVotes,
    #[error("Configuration error: {0}")]
    ConfigError(String),
}
