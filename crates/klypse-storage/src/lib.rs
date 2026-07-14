mod migration;
mod paths;

pub use migration::{SCHEMA_VERSION, migrate, open_database};
pub use paths::AppPaths;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("unable to discover the user data directories")]
    MissingUserDirectories,
    #[error("database schema version {0} is newer than this Klypse build")]
    UnsupportedSchema(i64),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub const CRATE_NAME: &str = "klypse-storage";
