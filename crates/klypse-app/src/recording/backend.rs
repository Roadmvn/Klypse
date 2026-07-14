use std::{collections::HashMap, sync::Mutex, time::Duration};

use chrono::{DateTime, Utc};
use klypse_domain::{
    CaptureArtifact, CaptureKind, DisplayServer, KlypseError, RecordingBackend, RecordingRequest,
};
use klypse_media::{
    GifPipeline, GifPipelineConfig, RecordingArtifact, VideoPipeline, VideoPipelineConfig,
};
use klypse_platform::{
    CapabilityReport, PortalRecordingSource, X11CaptureBackend, X11RecordingSource,
};
use klypse_storage::AppPaths;
use uuid::Uuid;

enum ActivePipeline {
    Video(VideoPipeline),
    Gif(GifPipeline),
}

struct ActiveRecording {
    pipeline: ActivePipeline,
    kind: CaptureKind,
    created_at: DateTime<Utc>,
}

pub struct DesktopRecordingBackend {
    paths: AppPaths,
    display: DisplayServer,
    gif_fps: u32,
    gif_maximum_duration: Duration,
    active: Mutex<HashMap<Uuid, ActiveRecording>>,
}

impl DesktopRecordingBackend {
    pub fn new(
        paths: AppPaths,
        gif_fps: u32,
        gif_maximum_duration: Duration,
    ) -> Result<Self, KlypseError> {
        paths.ensure().map_err(storage_error)?;
        let report = CapabilityReport::detect();
        Ok(Self {
            paths,
            display: report.display,
            gif_fps,
            gif_maximum_duration,
            active: Mutex::new(HashMap::new()),
        })
    }

    pub const fn display(&self) -> DisplayServer {
        self.display
    }

    async fn source_for(
        &self,
        request: &RecordingRequest,
    ) -> Result<klypse_media::PipelineSource, KlypseError> {
        match self.display {
            DisplayServer::X11 => {
                let backend = X11CaptureBackend::connect(&self.paths.temporary)?;
                X11RecordingSource::for_target(&backend, &request_to_capture(request), true)?
                    .into_pipeline_source()
            }
            DisplayServer::Wayland => PortalRecordingSource::select(request.target, None)
                .await?
                .into_pipeline_source(),
        }
    }
}

#[async_trait::async_trait]
impl RecordingBackend for DesktopRecordingBackend {
    async fn start(&self, request: &RecordingRequest) -> Result<Uuid, KlypseError> {
        let report = CapabilityReport::detect();
        let capability = match request.kind {
            CaptureKind::Video => &report.video_recording,
            CaptureKind::Gif => &report.gif_recording,
            CaptureKind::Screenshot => {
                return Err(KlypseError::InvalidRequest(
                    "screenshots cannot start a recording pipeline".into(),
                ));
            }
        };
        if !capability.available {
            return Err(KlypseError::UnavailableCapability(
                capability.detail.clone(),
            ));
        }
        if !self.active.lock().map_err(|_| lock_error())?.is_empty() {
            return Err(KlypseError::UnavailableCapability(
                "another recording pipeline is active".into(),
            ));
        }
        let source = self.source_for(request).await?;
        let id = Uuid::new_v4();
        let destination = self.paths.temporary.join(format!(
            "{id}.{}",
            match request.kind {
                CaptureKind::Video => "webm",
                CaptureKind::Gif => "gif",
                CaptureKind::Screenshot => unreachable!(),
            }
        ));
        let pipeline = match request.kind {
            CaptureKind::Video => ActivePipeline::Video(
                VideoPipeline::start(source, &destination, VideoPipelineConfig::default())
                    .map_err(media_error)?,
            ),
            CaptureKind::Gif => ActivePipeline::Gif(
                GifPipeline::start(
                    source,
                    &destination,
                    GifPipelineConfig::new(
                        self.gif_fps,
                        request
                            .max_duration
                            .unwrap_or(self.gif_maximum_duration)
                            .min(self.gif_maximum_duration),
                    )
                    .map_err(media_error)?,
                )
                .map_err(media_error)?,
            ),
            CaptureKind::Screenshot => unreachable!(),
        };
        self.active.lock().map_err(|_| lock_error())?.insert(
            id,
            ActiveRecording {
                pipeline,
                kind: request.kind,
                created_at: Utc::now(),
            },
        );
        Ok(id)
    }

    async fn stop(&self, session_id: Uuid) -> Result<CaptureArtifact, KlypseError> {
        let active = self
            .active
            .lock()
            .map_err(|_| lock_error())?
            .remove(&session_id)
            .ok_or_else(|| {
                KlypseError::UnavailableCapability("recording session not found".into())
            })?;
        let artifact = match active.pipeline {
            ActivePipeline::Video(pipeline) => pipeline.stop().map_err(media_error)?,
            ActivePipeline::Gif(pipeline) => pipeline.stop().map_err(media_error)?,
        };
        Ok(to_capture_artifact(
            session_id,
            active.kind,
            active.created_at,
            self.display,
            artifact,
        ))
    }
}

fn request_to_capture(request: &RecordingRequest) -> klypse_domain::CaptureRequest {
    klypse_domain::CaptureRequest {
        target: request.target,
        copy_to_clipboard: false,
        selection: request.selection,
    }
}

fn to_capture_artifact(
    id: Uuid,
    kind: CaptureKind,
    created_at: DateTime<Utc>,
    backend: DisplayServer,
    artifact: RecordingArtifact,
) -> CaptureArtifact {
    CaptureArtifact {
        id,
        kind,
        path: artifact.path,
        width: artifact.width,
        height: artifact.height,
        duration: Some(artifact.duration),
        created_at,
        backend,
    }
}

fn lock_error() -> KlypseError {
    KlypseError::UnavailableCapability("recording state lock was poisoned".into())
}

fn media_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Media(error.to_string())
}

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
