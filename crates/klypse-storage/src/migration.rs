use rusqlite::{Connection, TransactionBehavior};

use crate::{AppPaths, StorageError};

pub const SCHEMA_VERSION: i64 = 1;

pub fn open_database(paths: &AppPaths) -> Result<Connection, StorageError> {
    paths.ensure()?;
    let mut connection = Connection::open(&paths.database)?;
    // The capture runtime, the gallery and the recovery scanner each hold their
    // own connection to this file. Without these, a writer that loses the race
    // fails immediately and the freshly taken capture is parked in orphans/
    // without the user ever being told.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&mut connection)?;
    Ok(connection)
}

pub fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema(version));
    }

    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE captures (
                id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('screenshot','video','gif')),
                path TEXT NOT NULL UNIQUE,
                original_path TEXT,
                thumbnail_path TEXT,
                created_at TEXT NOT NULL,
                width INTEGER NOT NULL CHECK(width >= 0),
                height INTEGER NOT NULL CHECK(height >= 0),
                duration_ms INTEGER,
                file_size INTEGER NOT NULL CHECK(file_size >= 0),
                target TEXT NOT NULL,
                backend TEXT NOT NULL CHECK(backend IN ('x11','wayland')),
                annotation_json TEXT
            );
            CREATE INDEX captures_created_at_idx
                ON captures(created_at DESC, id DESC);
            PRAGMA user_version = 1;",
        )?;
    }

    transaction.commit()?;
    Ok(())
}
