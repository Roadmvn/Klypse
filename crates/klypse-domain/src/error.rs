#[derive(Debug, thiserror::Error)]
pub enum KlypseError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("unavailable capability: {0}")]
    UnavailableCapability(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("missing dependency: {0}")]
    MissingDependency(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("media error: {0}")]
    Media(String),
}
