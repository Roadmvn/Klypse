use std::{
    fs,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Utc;
use klypse_app::recording::{
    RecordingController, RecordingEffects, RecordingStage, RecordingUiState,
};
use klypse_domain::{
    CaptureArtifact, CaptureKind, CaptureTarget, DisplayServer, KlypseError, RecordingBackend,
    RecordingRequest,
};
use klypse_storage::{AppPaths, CaptureRecord, CaptureRepository, CaptureStore, open_database};
use uuid::Uuid;

struct FakeBackend {
    directory: std::path::PathBuf,
    events: Arc<Mutex<Vec<&'static str>>>,
    sessions: Mutex<Vec<Uuid>>,
    fail_stop: bool,
}

#[async_trait::async_trait]
impl RecordingBackend for FakeBackend {
    async fn start(&self, _request: &RecordingRequest) -> Result<Uuid, KlypseError> {
        let id = Uuid::new_v4();
        self.sessions.lock().unwrap().push(id);
        Ok(id)
    }

    async fn stop(&self, session_id: Uuid) -> Result<CaptureArtifact, KlypseError> {
        self.events.lock().unwrap().push("finalize");
        if self.fail_stop {
            return Err(KlypseError::Media("encoder failed".into()));
        }
        let path = self.directory.join(format!("{session_id}.webm"));
        fs::write(&path, b"finalized-webm").unwrap();
        Ok(CaptureArtifact {
            id: session_id,
            kind: CaptureKind::Video,
            path,
            width: 320,
            height: 180,
            duration: Some(Duration::from_secs(1)),
            created_at: Utc::now(),
            backend: DisplayServer::X11,
        })
    }
}

struct Effects {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingEffects for Effects {
    fn stage(&self, stage: RecordingStage) {
        self.events.lock().unwrap().push(match stage {
            RecordingStage::FileCommitted => "move",
            RecordingStage::DatabaseInserted => "database",
            RecordingStage::ThumbnailGenerated => "thumbnail",
        });
    }

    fn gallery_saved(&self, _record: &CaptureRecord) {
        self.events.lock().unwrap().push("gallery");
    }

    fn state_changed(&self, state: RecordingUiState) {
        if state == RecordingUiState::Idle {
            self.events.lock().unwrap().push("idle");
        }
    }

    fn generate_thumbnail(
        &self,
        _record: &CaptureRecord,
        destination: &std::path::Path,
    ) -> Result<bool, KlypseError> {
        image::RgbaImage::new(16, 9)
            .save(destination)
            .map_err(|error| KlypseError::Media(error.to_string()))?;
        Ok(true)
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    repository: Arc<CaptureRepository>,
    controller: RecordingController,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Fixture {
    fn new(fail_stop: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        paths.ensure().unwrap();
        let repository = Arc::new(CaptureRepository::new(open_database(&paths).unwrap()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(FakeBackend {
            directory: paths.temporary.clone(),
            events: Arc::clone(&events),
            sessions: Mutex::new(Vec::new()),
            fail_stop,
        });
        let controller = RecordingController::new(
            backend,
            repository.clone(),
            paths.clone(),
            DisplayServer::X11,
            Arc::new(Effects {
                events: Arc::clone(&events),
            }),
        );
        Self {
            _directory: directory,
            paths,
            repository,
            controller,
            events,
        }
    }

    fn video_request() -> RecordingRequest {
        RecordingRequest::new(CaptureKind::Video, CaptureTarget::Screen, None).unwrap()
    }

    fn marker(&self) -> std::path::PathBuf {
        self.paths.temporary.join(".klypse-session.json")
    }
}

#[test]
fn second_recording_is_rejected_while_one_is_active() {
    let mut fixture = Fixture::new(false);
    futures_lite::future::block_on(fixture.controller.start(Fixture::video_request())).unwrap();

    let error = futures_lite::future::block_on(fixture.controller.start(Fixture::video_request()))
        .unwrap_err();

    assert!(matches!(error, KlypseError::UnavailableCapability(_)));
    assert!(fixture.marker().exists());
}

#[test]
fn successful_stop_persists_before_returning_to_idle() {
    let mut fixture = Fixture::new(false);
    futures_lite::future::block_on(fixture.controller.start(Fixture::video_request())).unwrap();
    fixture.events.lock().unwrap().clear();

    let record = futures_lite::future::block_on(fixture.controller.stop()).unwrap();

    assert!(record.path.exists());
    assert_eq!(
        fixture.events.lock().unwrap().as_slice(),
        [
            "finalize",
            "move",
            "database",
            "thumbnail",
            "gallery",
            "idle"
        ]
    );
    assert!(!fixture.marker().exists());
    assert_eq!(fixture.repository.list_page(0, 10).unwrap().len(), 1);
}

#[test]
fn failed_finalization_requires_recovery_before_another_recording() {
    let mut fixture = Fixture::new(true);
    futures_lite::future::block_on(fixture.controller.start(Fixture::video_request())).unwrap();

    assert!(futures_lite::future::block_on(fixture.controller.stop()).is_err());
    assert!(fixture.marker().exists());
    assert_eq!(fixture.controller.state(), RecordingUiState::Failed);

    fixture.controller.acknowledge_failure().unwrap();
    assert_eq!(
        fixture.controller.state(),
        RecordingUiState::RecoveryRequired
    );
    assert!(fixture.marker().exists());

    let error = futures_lite::future::block_on(fixture.controller.start(Fixture::video_request()))
        .unwrap_err();
    assert!(matches!(error, KlypseError::UnavailableCapability(_)));

    fs::remove_file(fixture.marker()).unwrap();
    fixture.controller.complete_recovery().unwrap();
    assert_eq!(fixture.controller.state(), RecordingUiState::Idle);
    futures_lite::future::block_on(fixture.controller.start(Fixture::video_request())).unwrap();
}
