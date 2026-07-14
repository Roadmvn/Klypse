use klypse_storage::{AppPaths, StorageError, migrate};

#[test]
fn paths_use_injected_xdg_roots() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_roots(
        root.path().join("data"),
        root.path().join("cache"),
        root.path().join("run"),
        root.path().join("Pictures"),
    );

    assert_eq!(
        paths.database,
        root.path().join("data/klypse/library.sqlite3")
    );
    assert_eq!(
        paths.thumbnails,
        root.path().join("cache/klypse/thumbnails")
    );
    assert_eq!(paths.captures, root.path().join("Pictures/Klypse"));
    assert_eq!(paths.temporary, root.path().join("run/klypse/tmp"));
    assert_eq!(paths.orphans, root.path().join("data/klypse/orphans"));
}

#[test]
fn ensure_creates_every_writable_directory() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_roots(
        root.path().join("data"),
        root.path().join("cache"),
        root.path().join("run"),
        root.path().join("Pictures"),
    );

    paths.ensure().unwrap();

    assert!(paths.captures.is_dir());
    assert!(paths.thumbnails.is_dir());
    assert!(paths.temporary.is_dir());
    assert!(paths.orphans.is_dir());
    assert!(paths.database.parent().unwrap().is_dir());
}

#[test]
fn migration_creates_schema_version_one() {
    let mut connection = rusqlite::Connection::open_in_memory().unwrap();

    migrate(&mut connection).unwrap();

    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='captures'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
}

#[test]
fn migration_rejects_a_newer_schema() {
    let mut connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();

    let error = migrate(&mut connection).unwrap_err();

    assert!(matches!(error, StorageError::UnsupportedSchema(99)));
}
