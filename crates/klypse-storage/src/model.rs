use std::{path::PathBuf, time::Duration};

use chrono::{DateTime, Utc};
use klypse_domain::{CaptureArtifact, CaptureKind, CaptureTarget, DisplayServer};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteMode {
    GalleryOnly,
    GalleryAndFile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRecord {
    pub id: Uuid,
    pub kind: CaptureKind,
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub thumbnail_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub duration: Option<Duration>,
    pub file_size: u64,
    pub target: CaptureTarget,
    pub backend: DisplayServer,
    pub annotation_json: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewCaptureRecord {
    pub id: Uuid,
    pub kind: CaptureKind,
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub thumbnail_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub duration: Option<Duration>,
    pub file_size: u64,
    pub target: CaptureTarget,
    pub backend: DisplayServer,
    pub annotation_json: Option<String>,
}

impl NewCaptureRecord {
    pub fn from_artifact(artifact: CaptureArtifact, target: CaptureTarget, file_size: u64) -> Self {
        Self {
            id: artifact.id,
            kind: artifact.kind,
            path: artifact.path,
            original_path: None,
            thumbnail_path: None,
            created_at: artifact.created_at,
            width: artifact.width,
            height: artifact.height,
            duration: artifact.duration,
            file_size,
            target,
            backend: artifact.backend,
            annotation_json: None,
        }
    }
}

impl From<NewCaptureRecord> for CaptureRecord {
    fn from(value: NewCaptureRecord) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            path: value.path,
            original_path: value.original_path,
            thumbnail_path: value.thumbnail_path,
            created_at: value.created_at,
            width: value.width,
            height: value.height,
            duration: value.duration,
            file_size: value.file_size,
            target: value.target,
            backend: value.backend,
            annotation_json: value.annotation_json,
        }
    }
}

impl From<CaptureRecord> for NewCaptureRecord {
    fn from(value: CaptureRecord) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            path: value.path,
            original_path: value.original_path,
            thumbnail_path: value.thumbnail_path,
            created_at: value.created_at,
            width: value.width,
            height: value.height,
            duration: value.duration,
            file_size: value.file_size,
            target: value.target,
            backend: value.backend,
            annotation_json: value.annotation_json,
        }
    }
}
