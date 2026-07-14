use std::{env, fs, path::PathBuf};

use directories::{BaseDirs, UserDirs};

use crate::StorageError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub database: PathBuf,
    pub captures: PathBuf,
    pub thumbnails: PathBuf,
    pub temporary: PathBuf,
    pub orphans: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, StorageError> {
        let base = BaseDirs::new().ok_or(StorageError::MissingUserDirectories)?;
        let data_root = base.data_dir().to_path_buf();
        let cache_root = base.cache_dir().to_path_buf();
        let runtime_root = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| cache_root.join("runtime"));
        let pictures_root = UserDirs::new()
            .and_then(|directories| directories.picture_dir().map(PathBuf::from))
            .unwrap_or_else(|| base.home_dir().join("Pictures"));

        Ok(Self::from_roots(
            data_root,
            cache_root,
            runtime_root,
            pictures_root,
        ))
    }

    pub fn from_roots(
        data_root: PathBuf,
        cache_root: PathBuf,
        runtime_root: PathBuf,
        pictures_root: PathBuf,
    ) -> Self {
        Self {
            database: data_root.join("klypse/library.sqlite3"),
            captures: pictures_root.join("Klypse"),
            thumbnails: cache_root.join("klypse/thumbnails"),
            temporary: runtime_root.join("klypse/tmp"),
            orphans: data_root.join("klypse/orphans"),
        }
    }

    pub fn ensure(&self) -> Result<(), StorageError> {
        if let Some(database_directory) = self.database.parent() {
            fs::create_dir_all(database_directory)?;
        }
        for directory in [
            &self.captures,
            &self.thumbnails,
            &self.temporary,
            &self.orphans,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(())
    }
}
