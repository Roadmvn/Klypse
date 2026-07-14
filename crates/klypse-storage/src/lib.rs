mod atomic_file;
mod migration;
mod model;
mod paths;
mod repository;

pub use atomic_file::AtomicCaptureFile;
pub use migration::{SCHEMA_VERSION, migrate, open_database};
pub use model::{CaptureRecord, DeleteMode, NewCaptureRecord};
pub use paths::AppPaths;
pub use repository::{CaptureRepository, CaptureStore};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("unable to discover the user data directories")]
    MissingUserDirectories,
    #[error("database schema version {0} is newer than this Klypse build")]
    UnsupportedSchema(i64),
    #[error("page size {0} is outside the supported range 1..=200")]
    InvalidPageLimit(usize),
    #[error("capture {0} was not found")]
    CaptureNotFound(uuid::Uuid),
    #[error("capture {id} has a corrupt annotation: {reason}")]
    CorruptAnnotation { id: uuid::Uuid, reason: String },
    #[error("invalid stored value: {0}")]
    InvalidValue(String),
    #[error("a storage lock was poisoned")]
    PoisonedLock,
    #[error("failed to restore a capture after a file deletion error: {0}")]
    Recovery(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub const CRATE_NAME: &str = "klypse-storage";
