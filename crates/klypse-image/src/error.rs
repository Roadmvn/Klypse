use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("invalid geometry: {0}")]
    InvalidGeometry(&'static str),
    #[error("invalid annotation document: {0}")]
    InvalidDocument(String),
    #[error("invalid edit history: {0}")]
    InvalidHistory(&'static str),
    #[error("image processing failed: {0}")]
    Processing(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
