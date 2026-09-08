use std::{collections::HashMap, sync::Mutex, time::Duration};

use chrono::{DateTime, Utc};
use klypse_domain::{
    CaptureArtifact, CaptureKind, DisplayServer, KlypseError, RecordingBackend, RecordingRequest,
    RecordingTerminalEvent,
};
use klypse_media::{
    GifPipeline, GifPipelineConfig, PipelineTerminalEvent, RecordingArtifact, VideoPipeline,
    VideoPipelineConfig,
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
        let display = self.display;
        gtk::gio::spawn_blocking(move || {
            let artifact = match active.pipeline {
                ActivePipeline::Video(pipeline) => pipeline.stop().map_err(media_error)?,
                ActivePipeline::Gif(pipeline) => pipeline.stop().map_err(media_error)?,
            };
            Ok(to_capture_artifact(
                session_id,
                active.kind,
                active.created_at,
                display,
                artifact,
            ))
        })
        .await
        .map_err(|_| media_error("recording finalization worker panicked"))?
    }

    fn terminal_event(&self, session_id: Uuid) -> Option<RecordingTerminalEvent> {
        let active = match self.active.lock() {
            Ok(active) => active,
            Err(_) => return Some(RecordingTerminalEvent::Failed(lock_error().to_string())),
        };
        let active = active.get(&session_id)?;
        let event = match &active.pipeline {
            ActivePipeline::Video(pipeline) => pipeline.terminal_event(),
            ActivePipeline::Gif(pipeline) => pipeline.terminal_event(),
        }?;
        Some(match event {
            PipelineTerminalEvent::EndOfStream => RecordingTerminalEvent::EndOfStream,
            PipelineTerminalEvent::Failed(error) => RecordingTerminalEvent::Failed(error),
        })
    }
}

fn request_to_capture(request: &RecordingRequest) -> klypse_domain::CaptureRequest {
    klypse_domain::CaptureRequest {
        target: request.target,
        delay: std::time::Duration::ZERO,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    // Source guards can close portal sessions and block while releasing a
    // stream. Include that cleanup in the work moved off the graphical thread.
    struct SlowSourceRelease;

    impl Drop for SlowSourceRelease {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    #[test]
    fn stop_keeps_main_loop_responsive_while_releasing_the_stream() {
        use gstreamer::prelude::*;
        use klypse_media::PipelineSource;

        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("pictures"),
        );
        paths.ensure().unwrap();
        gstreamer::init().unwrap();
        let source = gstreamer::parse::bin_from_description(
            "videotestsrc is-live=true ! video/x-raw,width=80,height=60 ! identity",
            true,
        )
        .unwrap();
        let source =
            PipelineSource::from_element_with_guard(source.upcast(), 80, 60, SlowSourceRelease)
                .unwrap();
        let id = Uuid::new_v4();
        let pipeline = VideoPipeline::start(
            source,
            paths.temporary.join(format!("{id}.webm")),
            VideoPipelineConfig::default(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        let backend = DesktopRecordingBackend {
            paths,
            display: DisplayServer::X11,
            gif_fps: 12,
            gif_maximum_duration: Duration::from_secs(30),
            active: Mutex::new(HashMap::from([(
                id,
                ActiveRecording {
                    pipeline: ActivePipeline::Video(pipeline),
                    kind: CaptureKind::Video,
                    created_at: Utc::now(),
                },
            )])),
        };
        let context = gtk::glib::MainContext::new();
        let ticks = Arc::new(AtomicUsize::new(0));
        let timer = gtk::glib::timeout_source_new(
            Duration::from_millis(1),
            None,
            gtk::glib::Priority::DEFAULT,
            {
                let ticks = Arc::clone(&ticks);
                move || {
                    ticks.fetch_add(1, Ordering::Relaxed);
                    gtk::glib::ControlFlow::Continue
                }
            },
        );
        timer.attach(Some(&context));
        let artifact = context.block_on(backend.stop(id)).unwrap();
        timer.destroy();
        assert!(artifact.path.exists());
        assert!(
            ticks.load(Ordering::Relaxed) >= 5,
            "pipeline finalization blocked the graphical loop"
        );
        assert!(backend.active.lock().unwrap().is_empty());
    }
}
