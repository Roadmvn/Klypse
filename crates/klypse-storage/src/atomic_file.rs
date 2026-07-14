use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::{AppPaths, StorageError};

pub struct AtomicCaptureFile {
    temporary: NamedTempFile,
    captures: PathBuf,
    extension: String,
    file_stem: String,
}

impl AtomicCaptureFile {
    pub fn new(paths: &AppPaths, id: Uuid, extension: &str) -> Result<Self, StorageError> {
        Self::new_named(paths, id, extension, &id.to_string())
    }

    pub fn new_named(
        paths: &AppPaths,
        _id: Uuid,
        extension: &str,
        file_stem: &str,
    ) -> Result<Self, StorageError> {
        paths.ensure()?;
        if extension.is_empty()
            || !extension
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        {
            return Err(StorageError::InvalidValue(format!(
                "invalid capture extension {extension:?}"
            )));
        }
        if file_stem.is_empty()
            || !file_stem.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(StorageError::InvalidValue(format!(
                "invalid capture file stem {file_stem:?}"
            )));
        }
        Ok(Self {
            temporary: NamedTempFile::new_in(&paths.temporary)?,
            captures: paths.captures.clone(),
            extension: extension.to_ascii_lowercase(),
            file_stem: file_stem.into(),
        })
    }

    pub fn commit(mut self) -> Result<PathBuf, StorageError> {
        self.temporary.as_file_mut().flush()?;
        self.temporary.as_file().sync_all()?;

        let final_path = self
            .captures
            .join(format!("{}.{}", self.file_stem, self.extension));
        let mut source = File::open(self.temporary.path())?;
        let mut sibling = NamedTempFile::new_in(&self.captures)?;
        io::copy(&mut source, sibling.as_file_mut())?;
        sibling.as_file_mut().flush()?;
        sibling.as_file().sync_all()?;
        sibling
            .persist(&final_path)
            .map_err(|error| StorageError::Io(error.error))?;
        File::open(&self.captures)?.sync_all()?;
        Ok(final_path)
    }

    pub fn move_to_orphans(
        paths: &AppPaths,
        id: Uuid,
        committed_path: &Path,
    ) -> Result<PathBuf, StorageError> {
        paths.ensure()?;
        let extension = committed_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("bin");
        let orphan = paths.orphans.join(format!("{id}.{extension}"));
        std::fs::rename(committed_path, &orphan)?;
        File::open(&paths.orphans)?.sync_all()?;
        Ok(orphan)
    }
}

impl Write for AtomicCaptureFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.temporary.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.temporary.flush()
    }
}

impl Seek for AtomicCaptureFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.temporary.seek(position)
    }
}
