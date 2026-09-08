use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use chrono::{DateTime, Utc};
use klypse_domain::{
    CaptureArtifact, CaptureKind, DisplayServer, KlypseError, RecordingBackend, RecordingRequest,
    RecordingTerminalEvent,
};
use klypse_media::{RecordingMachine, RecordingState};
use klypse_storage::{AppPaths, AtomicCaptureFile, CaptureRecord, CaptureStore, NewCaptureRecord};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

pub const RECOVERY_MARKER_NAME: &str = ".klypse-session.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingUiState {
    Idle,
    Selecting,
    Recording,
    Finalizing,
    Failed,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingStage {
    FileCommitted,
    DatabaseInserted,
    ThumbnailGenerated,
}

pub trait RecordingEffects: Send + Sync {
    fn stage(&self, stage: RecordingStage);
    fn gallery_saved(&self, record: &CaptureRecord);
    fn state_changed(&self, state: RecordingUiState);

    fn recording_started(&self, _request: &RecordingRequest) {}

    fn generate_thumbnail(
        &self,
        _record: &CaptureRecord,
        _destination: &Path,
    ) -> Result<bool, KlypseError> {
        Ok(false)
    }
}

#[derive(Clone, Debug)]
struct ActiveRecording {
    id: Uuid,
    request: RecordingRequest,
    started_at: Instant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryMarker {
    pub session_id: Uuid,
    pub kind: CaptureKind,
    pub backend: DisplayServer,
    pub temporary_path: PathBuf,
    pub target: klypse_domain::CaptureTarget,
    pub started_at: DateTime<Utc>,
}

pub struct RecordingController {
    backend: Arc<dyn RecordingBackend>,
    store: Arc<dyn CaptureStore>,
    paths: AppPaths,
    display: DisplayServer,
    effects: Arc<dyn RecordingEffects>,
    machine: RecordingMachine,
    active: Option<ActiveRecording>,
    recovery_required: bool,
}

impl RecordingController {
    pub fn new(
        backend: Arc<dyn RecordingBackend>,
        store: Arc<dyn CaptureStore>,
        paths: AppPaths,
        display: DisplayServer,
        effects: Arc<dyn RecordingEffects>,
    ) -> Self {
        Self {
            backend,
            store,
            paths,
            display,
            effects,
            machine: RecordingMachine::idle(),
            active: None,
            recovery_required: false,
        }
    }

    pub fn state(&self) -> RecordingUiState {
        if self.recovery_required {
            RecordingUiState::RecoveryRequired
        } else {
            ui_state(self.machine.state())
        }
    }

    pub const fn active_session(&self) -> Option<Uuid> {
        match &self.active {
            Some(active) => Some(active.id),
            None => None,
        }
    }

    /// Check the active stream without consuming its terminal event.
    pub fn terminal_event(&self) -> Option<RecordingTerminalEvent> {
        if self.state() != RecordingUiState::Recording {
            return None;
        }
        let active = self.active.as_ref()?;
        self.backend.terminal_event(active.id).or_else(|| {
            active
                .request
                .max_duration
                .filter(|maximum| active.started_at.elapsed() >= *maximum)
                .map(|_| RecordingTerminalEvent::EndOfStream)
        })
    }

    /// Finalize spontaneous EOS or release a failed pipeline exactly once.
    /// Errors preserve the recovery marker, like a user-requested stop.
    pub async fn poll_terminal_event(&mut self) -> Result<Option<CaptureRecord>, KlypseError> {
        if self.terminal_event().is_none() {
            return Ok(None);
        }
        self.stop().await.map(Some)
    }

    pub fn recovery_marker_path(&self) -> PathBuf {
        self.paths.temporary.join(RECOVERY_MARKER_NAME)
    }

    pub fn read_recovery_marker(&self) -> Result<Option<RecoveryMarker>, KlypseError> {
        let path = self.recovery_marker_path();
        if !path.exists() {
            return Ok(None);
        }
        let marker = serde_json::from_reader(File::open(path)?)
            .map_err(|error| KlypseError::Storage(error.to_string()))?;
        Ok(Some(marker))
    }

    pub async fn start(&mut self, request: RecordingRequest) -> Result<Uuid, KlypseError> {
        if self.machine.state() != RecordingState::Idle
            || self.active.is_some()
            || self.recovery_required
        {
            return Err(KlypseError::UnavailableCapability(
                "another recording is already active".into(),
            ));
        }
        if self.recovery_marker_path().exists() {
            return Err(KlypseError::UnavailableCapability(
                "a previous recording requires recovery before starting another".into(),
            ));
        }
        self.machine.begin_selection().map_err(media_error)?;
        self.effects.state_changed(RecordingUiState::Selecting);
        let session_id = match self.backend.start(&request).await {
            Ok(session_id) => session_id,
            Err(KlypseError::Cancelled) => {
                self.machine.cancel_selection().map_err(media_error)?;
                self.effects.state_changed(RecordingUiState::Idle);
                return Err(KlypseError::Cancelled);
            }
            Err(error) => {
                self.fail(&error.to_string());
                return Err(error);
            }
        };
        let marker = RecoveryMarker {
            session_id,
            kind: request.kind,
            backend: self.display,
            temporary_path: self
                .paths
                .temporary
                .join(format!("{session_id}.{}", extension_for(request.kind))),
            target: request.target,
            started_at: Utc::now(),
        };
        if let Err(error) = self.write_recovery_marker(&marker) {
            let _ = self.backend.stop(session_id).await;
            self.fail(&error.to_string());
            return Err(error);
        }
        self.machine.start(session_id).map_err(media_error)?;
        self.active = Some(ActiveRecording {
            id: session_id,
            request,
            started_at: Instant::now(),
        });
        self.effects
            .recording_started(&self.active.as_ref().unwrap().request);
        self.effects.state_changed(RecordingUiState::Recording);
        Ok(session_id)
    }

    pub async fn stop(&mut self) -> Result<CaptureRecord, KlypseError> {
        let active = self
            .active
            .clone()
            .ok_or_else(|| KlypseError::UnavailableCapability("no recording is active".into()))?;
        self.machine.begin_finalization().map_err(media_error)?;
        self.effects.state_changed(RecordingUiState::Finalizing);
        let artifact = match self.backend.stop(active.id).await {
            Ok(artifact) => artifact,
            Err(error) => {
                self.fail(&error.to_string());
                return Err(error);
            }
        };
        let paths = self.paths.clone();
        let store = Arc::clone(&self.store);
        let effects = Arc::clone(&self.effects);
        let marker = self.recovery_marker_path();
        let result = gtk::gio::spawn_blocking(move || {
            persist_recording(
                artifact,
                active.request,
                &paths,
                &*store,
                &*effects,
                &marker,
            )
        })
        .await
        .map_err(|_| media_error("recording save worker panicked"))
        .and_then(|result| result);
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                self.fail(&error.to_string());
                return Err(error);
            }
        };
        // Notifications and gallery callbacks belong to the calling UI thread.
        self.effects.gallery_saved(&record);
        self.machine.finish().map_err(media_error)?;
        self.active = None;
        self.recovery_required = false;
        self.effects.state_changed(RecordingUiState::Idle);
        Ok(record)
    }

    pub fn acknowledge_failure(&mut self) -> Result<(), KlypseError> {
        self.machine.acknowledge_failure().map_err(media_error)?;
        self.active = None;
        self.recovery_required = self.recovery_marker_path().exists();
        self.effects.state_changed(if self.recovery_required {
            RecordingUiState::RecoveryRequired
        } else {
            RecordingUiState::Idle
        });
        Ok(())
    }

    pub fn complete_recovery(&mut self) -> Result<bool, KlypseError> {
        if !self.recovery_required {
            return Ok(false);
        }
        if self.recovery_marker_path().exists() {
            return Ok(false);
        }
        self.recovery_required = false;
        self.effects.state_changed(RecordingUiState::Idle);
        Ok(true)
    }

    fn write_recovery_marker(&self, marker: &RecoveryMarker) -> Result<(), KlypseError> {
        self.paths.ensure().map_err(storage_error)?;
        let mut temporary = NamedTempFile::new_in(&self.paths.temporary)?;
        serde_json::to_writer(&mut temporary, marker)
            .map_err(|error| KlypseError::Storage(error.to_string()))?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist_noclobber(self.recovery_marker_path())
            .map_err(|error| KlypseError::Io(error.error))?;
        File::open(&self.paths.temporary)?.sync_all()?;
        Ok(())
    }

    fn fail(&mut self, reason: &str) {
        let _ = self.machine.fail(reason);
        self.effects.state_changed(RecordingUiState::Failed);
    }
}

fn persist_recording(
    mut artifact: CaptureArtifact,
    request: RecordingRequest,
    paths: &AppPaths,
    store: &dyn CaptureStore,
    effects: &dyn RecordingEffects,
    marker: &Path,
) -> Result<CaptureRecord, KlypseError> {
    let artifact_id = artifact.id;
    let mut output = AtomicCaptureFile::new(paths, artifact.id, extension_for(artifact.kind))
        .map_err(storage_error)?;
    let mut source = File::open(&artifact.path)?;
    io::copy(&mut source, &mut output)?;
    let committed = output.commit().map_err(storage_error)?;
    let _ = fs::remove_file(&artifact.path);
    effects.stage(RecordingStage::FileCommitted);

    let file_size = fs::metadata(&committed)?.len();
    artifact.path = committed.clone();
    let new_record = NewCaptureRecord::from_artifact(artifact, request.target, file_size);
    let mut record = match store.insert(new_record) {
        Ok(record) => record,
        Err(error) => {
            let _ = AtomicCaptureFile::move_to_orphans(paths, artifact_id, &committed);
            return Err(storage_error(error));
        }
    };
    effects.stage(RecordingStage::DatabaseInserted);

    let thumbnail = paths.thumbnails.join(format!("{}.png", record.id));
    if effects
        .generate_thumbnail(&record, &thumbnail)
        .unwrap_or(false)
        && store.set_thumbnail(&record.id, &thumbnail).is_ok()
    {
        record.thumbnail_path = Some(thumbnail);
        effects.stage(RecordingStage::ThumbnailGenerated);
    }
    match fs::remove_file(marker) {
        Ok(()) => File::open(&paths.temporary)?.sync_all()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(record)
}

const fn extension_for(kind: CaptureKind) -> &'static str {
    match kind {
        CaptureKind::Video => "webm",
        CaptureKind::Gif => "gif",
        CaptureKind::Screenshot => "png",
    }
}

const fn ui_state(state: RecordingState) -> RecordingUiState {
    match state {
        RecordingState::Idle => RecordingUiState::Idle,
        RecordingState::Selecting => RecordingUiState::Selecting,
        RecordingState::Recording => RecordingUiState::Recording,
        RecordingState::Finalizing => RecordingUiState::Finalizing,
        RecordingState::Failed => RecordingUiState::Failed,
    }
}

fn media_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Media(error.to_string())
}

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
