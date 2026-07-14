use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::Utc;
use klypse_app::{
    capture::{CaptureEffects, CaptureOutcome, CaptureService, CaptureStage},
    editor::EditorController,
};
use klypse_domain::{
    CaptureArtifact, CaptureBackend, CaptureKind, CaptureRequest, CaptureTarget, DisplayServer,
    KlypseError,
};
use klypse_platform::{CapabilityReport, DisplayProbe};
use klypse_storage::{
    AppPaths, CaptureRecord, CaptureRepository, CaptureStore, DeleteMode, NewCaptureRecord,
    StorageError, open_database,
};
use uuid::Uuid;

#[test]
fn cancellation_and_permission_denial_leave_the_gallery_unchanged() {
    for failure in [BackendFailure::Cancelled, BackendFailure::PermissionDenied] {
        let fixture = Fixture::new();
        let service = fixture.service(Arc::new(FakeBackend::failing(failure)));

        let outcome = futures_lite::future::block_on(service.execute(
            klypse_domain::AppCommand::Capture(CaptureRequest::new(CaptureTarget::Area)),
        ));

        assert!(matches!(
            outcome,
            CaptureOutcome::Cancelled | CaptureOutcome::Failed(KlypseError::PermissionDenied(_))
        ));
        assert!(fixture.repository.list_page(0, 10).unwrap().is_empty());
        assert_eq!(fixture.effects.gallery.load(Ordering::Acquire), 0);
    }
}

#[test]
fn unwritable_capture_destination_does_not_create_a_database_row() {
    let mut fixture = Fixture::new();
    std::fs::remove_dir(&fixture.paths.captures).unwrap();
    std::fs::write(&fixture.paths.captures, b"not a directory").unwrap();
    let artifact = fixture.artifact(true);
    let service = fixture.service(Arc::new(FakeBackend::with_artifact(artifact)));

    let outcome = futures_lite::future::block_on(service.execute(
        klypse_domain::AppCommand::Capture(CaptureRequest::new(CaptureTarget::Screen)),
    ));

    assert!(matches!(outcome, CaptureOutcome::Failed(_)));
    assert!(fixture.repository.list_page(0, 10).unwrap().is_empty());
}

#[test]
fn database_insert_failure_preserves_the_committed_capture_as_an_orphan() {
    let mut fixture = Fixture::new();
    let artifact = fixture.artifact(true);
    let service = CaptureService::new(
        Arc::new(FakeBackend::with_artifact(artifact)),
        Arc::new(FailingStore),
        fixture.paths.clone(),
        fixture.effects.clone(),
    );

    let outcome = futures_lite::future::block_on(service.execute(
        klypse_domain::AppCommand::Capture(CaptureRequest::new(CaptureTarget::Screen)),
    ));

    assert!(matches!(
        outcome,
        CaptureOutcome::Failed(KlypseError::Storage(_))
    ));
    assert_eq!(
        std::fs::read_dir(&fixture.paths.orphans).unwrap().count(),
        1
    );
    assert_eq!(
        std::fs::read_dir(&fixture.paths.captures).unwrap().count(),
        0
    );
}

#[test]
fn thumbnail_failure_keeps_the_saved_capture_and_refreshes_the_gallery() {
    let mut fixture = Fixture::new();
    let artifact = fixture.artifact(false);
    let service = fixture.service(Arc::new(FakeBackend::with_artifact(artifact)));

    let outcome = futures_lite::future::block_on(service.execute(
        klypse_domain::AppCommand::Capture(CaptureRequest::new(CaptureTarget::Screen)),
    ));

    let CaptureOutcome::Saved(record) = outcome else {
        panic!("capture should remain saved when only its thumbnail fails");
    };
    assert!(record.path.exists());
    assert!(record.thumbnail_path.is_none());
    assert_eq!(fixture.repository.list_page(0, 10).unwrap().len(), 1);
    assert_eq!(fixture.effects.gallery.load(Ordering::Acquire), 1);
}

#[test]
fn missing_plugins_and_malformed_annotations_remain_typed_failures() {
    let report = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some("wayland-0".into()),
        None,
        true,
        false,
        false,
    ));
    assert!(!report.video_recording.available);
    assert!(!report.gif_recording.available);

    let mut record = sample_record(std::path::PathBuf::from("missing.png"));
    record.annotation_json = Some("{not-json".into());
    assert!(EditorController::open(&record).is_err());
}

#[derive(Clone, Copy)]
enum BackendFailure {
    Cancelled,
    PermissionDenied,
}

struct FakeBackend {
    artifact: Mutex<Option<CaptureArtifact>>,
    failure: Option<BackendFailure>,
}

impl FakeBackend {
    fn failing(failure: BackendFailure) -> Self {
        Self {
            artifact: Mutex::new(None),
            failure: Some(failure),
        }
    }

    fn with_artifact(artifact: CaptureArtifact) -> Self {
        Self {
            artifact: Mutex::new(Some(artifact)),
            failure: None,
        }
    }
}

#[async_trait::async_trait]
impl CaptureBackend for FakeBackend {
    async fn capture(&self, _request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError> {
        match self.failure {
            Some(BackendFailure::Cancelled) => Err(KlypseError::Cancelled),
            Some(BackendFailure::PermissionDenied) => {
                Err(KlypseError::PermissionDenied("test denial".into()))
            }
            None => self
                .artifact
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| KlypseError::InvalidRequest("artifact already consumed".into())),
        }
    }
}

#[derive(Default)]
struct Effects {
    gallery: AtomicUsize,
}

impl CaptureEffects for Effects {
    fn stage(&self, _stage: CaptureStage) {}

    fn refresh_gallery(&self) -> Result<(), KlypseError> {
        self.gallery.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn copy_to_clipboard(&self, _path: &Path) -> Result<(), KlypseError> {
        Ok(())
    }

    fn notify_saved(&self, _record: &CaptureRecord) {}
}

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    repository: Arc<CaptureRepository>,
    effects: Arc<Effects>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        paths.ensure().unwrap();
        Self {
            repository: Arc::new(CaptureRepository::new(open_database(&paths).unwrap())),
            effects: Arc::new(Effects::default()),
            _directory: directory,
            paths,
        }
    }

    fn artifact(&mut self, valid_png: bool) -> CaptureArtifact {
        let id = Uuid::new_v4();
        let path = self.paths.temporary.join(format!("{id}.png"));
        if valid_png {
            image::RgbaImage::new(16, 9).save(&path).unwrap();
        } else {
            std::fs::write(&path, b"not a PNG").unwrap();
        }
        CaptureArtifact {
            id,
            kind: CaptureKind::Screenshot,
            path,
            width: 16,
            height: 9,
            duration: None,
            created_at: Utc::now(),
            backend: DisplayServer::X11,
        }
    }

    fn service(&self, backend: Arc<dyn CaptureBackend>) -> CaptureService {
        CaptureService::new(
            backend,
            self.repository.clone(),
            self.paths.clone(),
            self.effects.clone(),
        )
    }
}

struct FailingStore;

impl CaptureStore for FailingStore {
    fn insert(&self, _record: NewCaptureRecord) -> Result<CaptureRecord, StorageError> {
        Err(StorageError::InvalidValue("injected insert failure".into()))
    }

    fn list_page(&self, _offset: usize, _limit: usize) -> Result<Vec<CaptureRecord>, StorageError> {
        Ok(Vec::new())
    }

    fn get(&self, _id: &Uuid) -> Result<Option<CaptureRecord>, StorageError> {
        Ok(None)
    }

    fn delete(&self, _id: &Uuid, _mode: DeleteMode) -> Result<(), StorageError> {
        Ok(())
    }

    fn set_thumbnail(&self, _id: &Uuid, _path: &Path) -> Result<(), StorageError> {
        Ok(())
    }

    fn set_annotation(&self, _id: &Uuid, _annotation: Option<&str>) -> Result<(), StorageError> {
        Ok(())
    }
}

fn sample_record(path: std::path::PathBuf) -> CaptureRecord {
    CaptureRecord {
        id: Uuid::new_v4(),
        kind: CaptureKind::Screenshot,
        path,
        original_path: None,
        thumbnail_path: None,
        created_at: Utc::now(),
        width: 16,
        height: 9,
        duration: None,
        file_size: 0,
        target: CaptureTarget::Area,
        backend: DisplayServer::X11,
        annotation_json: None,
    }
}
