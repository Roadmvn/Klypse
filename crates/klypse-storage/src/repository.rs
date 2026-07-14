use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use chrono::{DateTime, SecondsFormat, Utc};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use rusqlite::{Connection, Row, params};
use uuid::Uuid;

use crate::{CaptureRecord, DeleteMode, NewCaptureRecord, StorageError};

pub trait CaptureStore: Send + Sync {
    fn insert(&self, record: NewCaptureRecord) -> Result<CaptureRecord, StorageError>;
    fn list_page(&self, offset: usize, limit: usize) -> Result<Vec<CaptureRecord>, StorageError>;
    fn get(&self, id: &Uuid) -> Result<Option<CaptureRecord>, StorageError>;
    fn delete(&self, id: &Uuid, mode: DeleteMode) -> Result<(), StorageError>;
    fn set_thumbnail(&self, id: &Uuid, path: &Path) -> Result<(), StorageError>;
    fn set_annotation(&self, id: &Uuid, annotation: Option<&str>) -> Result<(), StorageError>;

    fn relocate(
        &self,
        _id: &Uuid,
        _path: &Path,
        _width: u32,
        _height: u32,
        _duration: Option<Duration>,
        _file_size: u64,
    ) -> Result<CaptureRecord, StorageError> {
        Err(StorageError::InvalidValue(
            "this capture store does not support file relocation".into(),
        ))
    }
}

pub struct CaptureRepository {
    connection: Mutex<Connection>,
}

impl CaptureRepository {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
        }
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::PoisonedLock)
    }
}

impl CaptureStore for CaptureRepository {
    fn insert(&self, record: NewCaptureRecord) -> Result<CaptureRecord, StorageError> {
        let connection = self.connection()?;
        insert_record(&connection, &record)?;
        Ok(record.into())
    }

    fn list_page(&self, offset: usize, limit: usize) -> Result<Vec<CaptureRecord>, StorageError> {
        if !(1..=200).contains(&limit) {
            return Err(StorageError::InvalidPageLimit(limit));
        }
        let offset = i64::try_from(offset)
            .map_err(|_| StorageError::InvalidValue("page offset is too large".into()))?;
        let limit = i64::try_from(limit)
            .map_err(|_| StorageError::InvalidValue("page limit is too large".into()))?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, kind, path, original_path, thumbnail_path, created_at, width, height,
                    duration_ms, file_size, target, backend, annotation_json
             FROM captures
             ORDER BY created_at DESC, id DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let records = statement
            .query_map(params![limit, offset], capture_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    fn get(&self, id: &Uuid) -> Result<Option<CaptureRecord>, StorageError> {
        let connection = self.connection()?;
        get_record(&connection, id)
    }

    fn delete(&self, id: &Uuid, mode: DeleteMode) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let record = get_record(&connection, id)?.ok_or(StorageError::CaptureNotFound(*id))?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM captures WHERE id = ?1", [id.to_string()])?;
        transaction.commit()?;

        if mode == DeleteMode::GalleryOnly {
            return Ok(());
        }

        let remove_result = record
            .thumbnail_path
            .as_deref()
            .map(remove_if_present)
            .transpose()
            .and_then(|_| remove_if_present(&record.path));
        if let Err(error) = remove_result {
            if let Err(restore_error) = insert_record(&connection, &record.clone().into()) {
                return Err(StorageError::Recovery(format!(
                    "{error}; row restore also failed: {restore_error}"
                )));
            }
            return Err(StorageError::Io(error));
        }
        Ok(())
    }

    fn set_thumbnail(&self, id: &Uuid, path: &Path) -> Result<(), StorageError> {
        let path = path_to_text(path)?;
        let changed = self.connection()?.execute(
            "UPDATE captures SET thumbnail_path = ?1 WHERE id = ?2",
            params![path, id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::CaptureNotFound(*id));
        }
        Ok(())
    }

    fn set_annotation(&self, id: &Uuid, annotation: Option<&str>) -> Result<(), StorageError> {
        let changed = self.connection()?.execute(
            "UPDATE captures SET annotation_json = ?1 WHERE id = ?2",
            params![annotation, id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::CaptureNotFound(*id));
        }
        Ok(())
    }

    fn relocate(
        &self,
        id: &Uuid,
        path: &Path,
        width: u32,
        height: u32,
        duration: Option<Duration>,
        file_size: u64,
    ) -> Result<CaptureRecord, StorageError> {
        let duration_ms = duration
            .map(|value| i64::try_from(value.as_millis()))
            .transpose()
            .map_err(|_| StorageError::InvalidValue("duration is too large".into()))?;
        let file_size = i64::try_from(file_size)
            .map_err(|_| StorageError::InvalidValue("file size is too large".into()))?;
        let changed = self.connection()?.execute(
            "UPDATE captures
             SET path = ?1, width = ?2, height = ?3, duration_ms = ?4,
                 file_size = ?5, thumbnail_path = NULL
             WHERE id = ?6",
            params![
                path_to_text(path)?,
                i64::from(width),
                i64::from(height),
                duration_ms,
                file_size,
                id.to_string(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::CaptureNotFound(*id));
        }
        self.get(id)?.ok_or(StorageError::CaptureNotFound(*id))
    }
}

fn insert_record(connection: &Connection, record: &NewCaptureRecord) -> Result<(), StorageError> {
    let duration_ms = record
        .duration
        .map(|value| i64::try_from(value.as_millis()))
        .transpose()
        .map_err(|_| StorageError::InvalidValue("duration is too large".into()))?;
    let file_size = i64::try_from(record.file_size)
        .map_err(|_| StorageError::InvalidValue("file size is too large".into()))?;
    connection.execute(
        "INSERT INTO captures (
            id, kind, path, original_path, thumbnail_path, created_at, width, height,
            duration_ms, file_size, target, backend, annotation_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            record.id.to_string(),
            kind_to_text(record.kind),
            path_to_text(&record.path)?,
            optional_path_to_text(record.original_path.as_deref())?,
            optional_path_to_text(record.thumbnail_path.as_deref())?,
            record
                .created_at
                .to_rfc3339_opts(SecondsFormat::Nanos, true),
            i64::from(record.width),
            i64::from(record.height),
            duration_ms,
            file_size,
            target_to_text(record.target),
            backend_to_text(record.backend),
            record.annotation_json,
        ],
    )?;
    Ok(())
}

fn get_record(connection: &Connection, id: &Uuid) -> Result<Option<CaptureRecord>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, kind, path, original_path, thumbnail_path, created_at, width, height,
                duration_ms, file_size, target, backend, annotation_json
         FROM captures WHERE id = ?1",
    )?;
    let mut rows = statement.query([id.to_string()])?;
    rows.next()?
        .map(capture_from_row)
        .transpose()
        .map_err(Into::into)
}

fn capture_from_row(row: &Row<'_>) -> rusqlite::Result<CaptureRecord> {
    let id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let created_at: String = row.get(5)?;
    let width: i64 = row.get(6)?;
    let height: i64 = row.get(7)?;
    let duration_ms: Option<i64> = row.get(8)?;
    let file_size: i64 = row.get(9)?;
    let target: String = row.get(10)?;
    let backend: String = row.get(11)?;

    Ok(CaptureRecord {
        id: parse_value(0, &id, Uuid::parse_str)?,
        kind: parse_kind(1, &kind)?,
        path: PathBuf::from(row.get::<_, String>(2)?),
        original_path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        thumbnail_path: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
        created_at: parse_value(5, &created_at, |value| {
            DateTime::parse_from_rfc3339(value).map(|date| date.with_timezone(&Utc))
        })?,
        width: unsigned_value(6, width)?,
        height: unsigned_value(7, height)?,
        duration: duration_ms
            .map(|value| unsigned_duration(8, value))
            .transpose()?,
        file_size: unsigned_value(9, file_size)?,
        target: parse_target(10, &target)?,
        backend: parse_backend(11, &backend)?,
        annotation_json: row.get(12)?,
    })
}

fn parse_value<T, E>(
    column: usize,
    value: &str,
    parser: impl FnOnce(&str) -> Result<T, E>,
) -> rusqlite::Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    parser(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn parse_kind(column: usize, value: &str) -> rusqlite::Result<CaptureKind> {
    match value {
        "screenshot" => Ok(CaptureKind::Screenshot),
        "video" => Ok(CaptureKind::Video),
        "gif" => Ok(CaptureKind::Gif),
        _ => Err(invalid_text(column, value)),
    }
}

fn parse_target(column: usize, value: &str) -> rusqlite::Result<CaptureTarget> {
    match value {
        "area" => Ok(CaptureTarget::Area),
        "screen" => Ok(CaptureTarget::Screen),
        "window" => Ok(CaptureTarget::Window),
        "active-window" => Ok(CaptureTarget::ActiveWindow),
        _ => Err(invalid_text(column, value)),
    }
}

fn parse_backend(column: usize, value: &str) -> rusqlite::Result<DisplayServer> {
    match value {
        "x11" => Ok(DisplayServer::X11),
        "wayland" => Ok(DisplayServer::Wayland),
        _ => Err(invalid_text(column, value)),
    }
}

fn invalid_text(column: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        std::io::Error::new(std::io::ErrorKind::InvalidData, value.to_owned()).into(),
    )
}

fn unsigned_value<T>(column: usize, value: i64) -> rusqlite::Result<T>
where
    T: TryFrom<i64>,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    T::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn unsigned_duration(column: usize, value: i64) -> rusqlite::Result<Duration> {
    unsigned_value::<u64>(column, value).map(Duration::from_millis)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn path_to_text(path: &Path) -> Result<&str, StorageError> {
    path.to_str()
        .ok_or_else(|| StorageError::InvalidValue("path is not valid UTF-8".into()))
}

fn optional_path_to_text(path: Option<&Path>) -> Result<Option<&str>, StorageError> {
    path.map(path_to_text).transpose()
}

const fn kind_to_text(value: CaptureKind) -> &'static str {
    match value {
        CaptureKind::Screenshot => "screenshot",
        CaptureKind::Video => "video",
        CaptureKind::Gif => "gif",
    }
}

const fn target_to_text(value: CaptureTarget) -> &'static str {
    match value {
        CaptureTarget::Area => "area",
        CaptureTarget::Screen => "screen",
        CaptureTarget::Window => "window",
        CaptureTarget::ActiveWindow => "active-window",
    }
}

const fn backend_to_text(value: DisplayServer) -> &'static str {
    match value {
        DisplayServer::X11 => "x11",
        DisplayServer::Wayland => "wayland",
    }
}
