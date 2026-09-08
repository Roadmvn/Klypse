use std::{
    fs::{self, File},
    io,
    path::Path,
    sync::Arc,
};

use klypse_domain::{
    AppCommand, CaptureArtifact, CaptureBackend, CaptureRequest, KlypseError, PixelRect,
};
use klypse_media::Thumbnailer;
use klypse_storage::{AppPaths, AtomicCaptureFile, CaptureRecord, CaptureStore, NewCaptureRecord};

const THUMBNAIL_EDGE: u32 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureStage {
    FileCommitted,
    DatabaseInserted,
    ThumbnailGenerated,
    GalleryRefreshed,
}

#[derive(Debug)]
pub enum CaptureOutcome {
    Saved(CaptureRecord),
    Cancelled,
    Ignored,
    Failed(KlypseError),
}

pub trait CaptureEffects: Send + Sync {
    fn stage(&self, stage: CaptureStage);
    fn refresh_gallery(&self) -> Result<(), KlypseError>;
    fn copy_to_clipboard(&self, path: &Path) -> Result<(), KlypseError>;
    fn notify_saved(&self, record: &CaptureRecord);
}

pub struct CaptureService {
    backend: Arc<dyn CaptureBackend>,
    store: Arc<dyn CaptureStore>,
    paths: AppPaths,
    effects: Arc<dyn CaptureEffects>,
}

impl CaptureService {
    pub fn new(
        backend: Arc<dyn CaptureBackend>,
        store: Arc<dyn CaptureStore>,
        paths: AppPaths,
        effects: Arc<dyn CaptureEffects>,
    ) -> Self {
        Self {
            backend,
            store,
            paths,
            effects,
        }
    }

    pub async fn execute(&self, command: AppCommand) -> CaptureOutcome {
        let AppCommand::Capture(request) = command else {
            return CaptureOutcome::Ignored;
        };
        match self.execute_capture(request).await {
            Ok(record) => CaptureOutcome::Saved(record),
            Err(KlypseError::Cancelled) => CaptureOutcome::Cancelled,
            Err(error) => CaptureOutcome::Failed(error),
        }
    }

    async fn execute_capture(&self, request: CaptureRequest) -> Result<CaptureRecord, KlypseError> {
        let artifact = self.backend.capture(&request).await?;
        self.save_artifact(request, artifact)
    }

    /// Save pixels captured before the selector took focus, including popups.
    pub fn save_artifact(
        &self,
        request: CaptureRequest,
        mut artifact: CaptureArtifact,
    ) -> Result<CaptureRecord, KlypseError> {
        let artifact_id = artifact.id;
        let backend_path = artifact.path.clone();
        let mut destination =
            AtomicCaptureFile::new(&self.paths, artifact.id, "png").map_err(storage_error)?;
        let mut source = File::open(&backend_path)?;
        io::copy(&mut source, &mut destination)?;
        let committed = destination.commit().map_err(storage_error)?;
        let _ = fs::remove_file(&backend_path);
        artifact.path = committed.clone();
        self.effects.stage(CaptureStage::FileCommitted);

        let file_size = fs::metadata(&committed)?.len();
        let new_record = NewCaptureRecord::from_artifact(artifact, request.target, file_size);
        let mut record = match self.store.insert(new_record) {
            Ok(record) => record,
            Err(error) => {
                let _ = AtomicCaptureFile::move_to_orphans(&self.paths, artifact_id, &committed);
                return Err(storage_error(error));
            }
        };
        self.effects.stage(CaptureStage::DatabaseInserted);

        let thumbnail = self.paths.thumbnails.join(format!("{}.png", record.id));
        if Thumbnailer::new(THUMBNAIL_EDGE)
            .generate(&record.path, &thumbnail)
            .is_ok()
            && self.store.set_thumbnail(&record.id, &thumbnail).is_ok()
        {
            record.thumbnail_path = Some(thumbnail);
            self.effects.stage(CaptureStage::ThumbnailGenerated);
        }

        self.effects.refresh_gallery()?;
        self.effects.stage(CaptureStage::GalleryRefreshed);
        if request.copy_to_clipboard {
            self.effects.copy_to_clipboard(&record.path)?;
        }
        self.effects.notify_saved(&record);
        Ok(record)
    }
}

/// Crop the frozen desktop, never a second capture of the now-changed screen.
pub fn crop_snapshot(artifact: &mut CaptureArtifact, rect: PixelRect) -> Result<(), KlypseError> {
    let image =
        image::open(&artifact.path).map_err(|error| KlypseError::Media(error.to_string()))?;
    if rect.x < 0
        || rect.y < 0
        || rect.width == 0
        || rect.height == 0
        || i64::from(rect.x) + i64::from(rect.width) > i64::from(image.width())
        || i64::from(rect.y) + i64::from(rect.height) > i64::from(image.height())
    {
        return Err(KlypseError::InvalidRequest(
            "selection is outside the snapshot".into(),
        ));
    }
    image
        .crop_imm(rect.x as u32, rect.y as u32, rect.width, rect.height)
        .save(&artifact.path)
        .map_err(|error| KlypseError::Media(error.to_string()))?;
    artifact.width = rect.width;
    artifact.height = rect.height;
    Ok(())
}

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
