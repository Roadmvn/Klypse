use std::sync::{Arc, Mutex};

use chrono::Utc;
use klypse_app::capture::{CaptureEffects, CaptureOutcome, CaptureService, CaptureStage};
use klypse_domain::{
    AppCommand, CaptureArtifact, CaptureBackend, CaptureKind, CaptureRequest, CaptureSelection,
    CaptureTarget, DisplayServer, KlypseError,
};
use klypse_storage::{
    AppPaths, CaptureRecord, CaptureRepository, CaptureStore, DeleteMode, NewCaptureRecord,
    StorageError, open_database,
};
use uuid::Uuid;

struct FakeBackend {
    result: Mutex<Option<Result<CaptureArtifact, KlypseError>>>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait::async_trait]
impl CaptureBackend for FakeBackend {
    async fn capture(&self, _request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError> {
        self.events.lock().unwrap().push("backend");
        self.result.lock().unwrap().take().unwrap()
    }
}

struct SharedStore(Arc<CaptureRepository>);

impl CaptureStore for SharedStore {
    fn insert(&self, record: NewCaptureRecord) -> Result<CaptureRecord, StorageError> {
        self.0.insert(record)
    }

    fn list_page(&self, offset: usize, limit: usize) -> Result<Vec<CaptureRecord>, StorageError> {
        self.0.list_page(offset, limit)
    }

    fn get(&self, id: &Uuid) -> Result<Option<CaptureRecord>, StorageError> {
        self.0.get(id)
    }

    fn delete(&self, id: &Uuid, mode: DeleteMode) -> Result<(), StorageError> {
        self.0.delete(id, mode)
    }

    fn set_thumbnail(&self, id: &Uuid, path: &std::path::Path) -> Result<(), StorageError> {
        self.0.set_thumbnail(id, path)
    }

    fn set_annotation(&self, id: &Uuid, annotation: Option<&str>) -> Result<(), StorageError> {
        self.0.set_annotation(id, annotation)
    }
}

struct RecordingEffects {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl CaptureEffects for RecordingEffects {
    fn stage(&self, stage: CaptureStage) {
        self.events.lock().unwrap().push(match stage {
            CaptureStage::FileCommitted => "file",
            CaptureStage::DatabaseInserted => "database",
            CaptureStage::ThumbnailGenerated => "thumbnail",
            CaptureStage::GalleryRefreshed => "gallery",
        });
    }

    fn refresh_gallery(&self) -> Result<(), KlypseError> {
        Ok(())
    }

    fn copy_to_clipboard(&self, _path: &std::path::Path) -> Result<(), KlypseError> {
        self.events.lock().unwrap().push("copy");
        Ok(())
    }

    fn notify_saved(&self, _record: &CaptureRecord) {
        self.events.lock().unwrap().push("notify");
    }
}

struct CaptureServiceFixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    repository: Arc<CaptureRepository>,
    service: CaptureService,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl CaptureServiceFixture {
    fn successful() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("backend.png");
        image::RgbaImage::new(40, 30).save(&source).unwrap();
        Self::new(
            directory,
            Ok(CaptureArtifact {
                id: Uuid::new_v4(),
                kind: CaptureKind::Screenshot,
                path: source,
                width: 40,
                height: 30,
                duration: None,
                created_at: Utc::now(),
                backend: DisplayServer::X11,
            }),
        )
    }

    fn cancelled() -> Self {
        Self::new(tempfile::tempdir().unwrap(), Err(KlypseError::Cancelled))
    }

    fn new(directory: tempfile::TempDir, result: Result<CaptureArtifact, KlypseError>) -> Self {
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        let repository = Arc::new(CaptureRepository::new(open_database(&paths).unwrap()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = CaptureService::new(
            Arc::new(FakeBackend {
                result: Mutex::new(Some(result)),
                events: Arc::clone(&events),
            }),
            Arc::new(SharedStore(Arc::clone(&repository))),
            paths.clone(),
            Arc::new(RecordingEffects {
                events: Arc::clone(&events),
            }),
        );
        Self {
            _directory: directory,
            paths,
            repository,
            service,
            events,
        }
    }

    fn execute_area(&self) -> CaptureOutcome {
        self.execute_area_with_copy(false)
    }

    fn execute_area_with_copy(&self, copy_to_clipboard: bool) -> CaptureOutcome {
        futures_lite::future::block_on(self.service.execute(AppCommand::Capture(CaptureRequest {
            target: CaptureTarget::Area,
            copy_to_clipboard,
            selection: CaptureSelection::Automatic,
        })))
    }

    fn events(&self) -> Vec<&'static str> {
        self.events.lock().unwrap().clone()
    }
}

#[test]
fn successful_capture_is_persisted_before_gallery_refresh() {
    let fixture = CaptureServiceFixture::successful();

    let outcome = fixture.execute_area();

    assert!(matches!(outcome, CaptureOutcome::Saved(_)));
    assert_eq!(
        fixture.events(),
        [
            "backend",
            "file",
            "database",
            "thumbnail",
            "gallery",
            "notify"
        ]
    );
    assert_eq!(fixture.repository.list_page(0, 50).unwrap().len(), 1);
}

#[test]
fn cancellation_creates_no_file_or_row() {
    let fixture = CaptureServiceFixture::cancelled();

    assert!(matches!(fixture.execute_area(), CaptureOutcome::Cancelled));
    assert_eq!(fixture.events(), ["backend"]);
    assert!(fixture.repository.list_page(0, 50).unwrap().is_empty());
    assert!(fixture.paths.captures.read_dir().unwrap().next().is_none());
}

#[test]
fn copy_and_notification_run_only_after_a_successful_capture() {
    let fixture = CaptureServiceFixture::successful();

    assert!(matches!(
        fixture.execute_area_with_copy(true),
        CaptureOutcome::Saved(_)
    ));
    assert!(fixture.events().ends_with(&["copy", "notify"]));
}

#[test]
fn unsupported_commands_do_not_call_the_backend() {
    let fixture = CaptureServiceFixture::cancelled();

    let outcome = futures_lite::future::block_on(fixture.service.execute(AppCommand::Open));

    assert!(matches!(outcome, CaptureOutcome::Ignored));
    assert!(fixture.events().is_empty());
}
