use std::{
    any::Any,
    os::fd::{AsRawFd, OwnedFd},
};

use ashpd::{
    PortalError, WindowIdentifier,
    desktop::{
        PersistMode, Session,
        screencast::{
            CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
            StartCastOptions,
        },
    },
};
use gstreamer as gst;
use klypse_domain::{CaptureTarget, KlypseError};
use klypse_media::PipelineSource;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalScreencastOptions {
    pub monitors: bool,
    pub windows: bool,
    pub multiple: bool,
    pub embedded_cursor: bool,
}

pub const fn portal_screencast_options(target: CaptureTarget) -> PortalScreencastOptions {
    match target {
        CaptureTarget::Screen => PortalScreencastOptions {
            monitors: true,
            windows: false,
            multiple: false,
            embedded_cursor: true,
        },
        CaptureTarget::Window | CaptureTarget::ActiveWindow => PortalScreencastOptions {
            monitors: false,
            windows: true,
            multiple: false,
            embedded_cursor: true,
        },
        CaptureTarget::Area => PortalScreencastOptions {
            monitors: true,
            windows: true,
            multiple: false,
            embedded_cursor: true,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalRecordingClientError {
    Cancelled,
    PermissionDenied(String),
    Unavailable(String),
}

pub struct PortalStreamDescriptor {
    node_id: u32,
    width: u32,
    height: u32,
    remote_fd: OwnedFd,
    session_guard: Option<Box<dyn Any + Send>>,
}

impl PortalStreamDescriptor {
    pub fn new(node_id: u32, width: u32, height: u32, remote_fd: OwnedFd) -> Self {
        Self {
            node_id,
            width,
            height,
            remote_fd,
            session_guard: None,
        }
    }

    fn with_session_guard<T>(mut self, guard: T) -> Self
    where
        T: Send + 'static,
    {
        self.session_guard = Some(Box::new(guard));
        self
    }
}

#[async_trait::async_trait]
pub trait PortalRecordingClient: Send + Sync {
    async fn select_stream(
        &self,
        options: PortalScreencastOptions,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<PortalStreamDescriptor, PortalRecordingClientError>;
}

pub struct AshpdRecordingClient;

#[async_trait::async_trait]
impl PortalRecordingClient for AshpdRecordingClient {
    async fn select_stream(
        &self,
        options: PortalScreencastOptions,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<PortalStreamDescriptor, PortalRecordingClientError> {
        let proxy = Screencast::new().await.map_err(map_ashpd_error)?;
        let session = proxy
            .create_session(Default::default())
            .await
            .map_err(map_ashpd_error)?;
        let sources = match (options.monitors, options.windows) {
            (true, true) => SourceType::Monitor | SourceType::Window,
            (true, false) => SourceType::Monitor.into(),
            (false, true) => SourceType::Window.into(),
            (false, false) => {
                return Err(PortalRecordingClientError::Unavailable(
                    "portal recording requires a monitor or window source".into(),
                ));
            }
        };
        let request = proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(sources)
                    .set_multiple(options.multiple)
                    .set_persist_mode(PersistMode::Application),
            )
            .await
            .map_err(map_ashpd_error)?;
        request.response().map_err(map_ashpd_error)?;
        let request = proxy
            .start(&session, identifier, StartCastOptions::default())
            .await
            .map_err(map_ashpd_error)?;
        let response = request.response().map_err(map_ashpd_error)?;
        let [stream] = response.streams() else {
            return Err(PortalRecordingClientError::Unavailable(
                "the screencast portal must return exactly one stream".into(),
            ));
        };
        let (width, height) = stream
            .size()
            .and_then(|(width, height)| {
                Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
            })
            .filter(|(width, height)| *width > 0 && *height > 0)
            .unwrap_or((1, 1));
        let remote_fd = proxy
            .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
            .await
            .map_err(map_ashpd_error)?;
        Ok(
            PortalStreamDescriptor::new(stream.pipe_wire_node_id(), width, height, remote_fd)
                .with_session_guard(AshpdSessionGuard(session)),
        )
    }
}

struct AshpdSessionGuard(Session<Screencast>);

impl Drop for AshpdSessionGuard {
    fn drop(&mut self) {
        let _ = futures_lite::future::block_on(self.0.close());
    }
}

pub struct PortalRecordingSource {
    node_id: u32,
    width: u32,
    height: u32,
    remote_fd: OwnedFd,
    _session_guard: Option<Box<dyn Any + Send>>,
}

impl PortalRecordingSource {
    pub async fn select(
        target: CaptureTarget,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<Self, KlypseError> {
        Self::select_with_identifier(&AshpdRecordingClient, target, identifier).await
    }

    pub async fn select_with<C>(client: &C, target: CaptureTarget) -> Result<Self, KlypseError>
    where
        C: PortalRecordingClient,
    {
        Self::select_with_identifier(client, target, None).await
    }

    pub async fn select_with_identifier<C>(
        client: &C,
        target: CaptureTarget,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<Self, KlypseError>
    where
        C: PortalRecordingClient,
    {
        let descriptor = client
            .select_stream(portal_screencast_options(target), identifier)
            .await
            .map_err(map_client_error)?;
        Self::from_stream(descriptor)
    }

    pub fn from_stream(descriptor: PortalStreamDescriptor) -> Result<Self, KlypseError> {
        if descriptor.node_id == 0 || descriptor.width == 0 || descriptor.height == 0 {
            return Err(KlypseError::UnavailableCapability(
                "the screencast portal returned an invalid PipeWire stream".into(),
            ));
        }
        Ok(Self {
            node_id: descriptor.node_id,
            width: descriptor.width,
            height: descriptor.height,
            remote_fd: descriptor.remote_fd,
            _session_guard: descriptor.session_guard,
        })
    }

    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn build_element(&self) -> Result<gst::Element, KlypseError> {
        gst::init().map_err(recording_error)?;
        gst::ElementFactory::make("pipewiresrc")
            .name("klypse-portal-recording-source")
            .property("fd", self.remote_fd.as_raw_fd())
            .property("path", self.node_id.to_string())
            .property("do-timestamp", true)
            .build()
            .map_err(|_| {
                KlypseError::UnavailableCapability(
                    "GStreamer pipewiresrc is unavailable for Wayland recording".into(),
                )
            })
    }

    pub fn into_pipeline_source(self) -> Result<PipelineSource, KlypseError> {
        let element = self.build_element()?;
        PipelineSource::from_element_with_guard(element, self.width, self.height, self)
            .map_err(recording_error)
    }
}

fn map_ashpd_error(error: ashpd::Error) -> PortalRecordingClientError {
    match error {
        ashpd::Error::Portal(PortalError::Cancelled(_)) => PortalRecordingClientError::Cancelled,
        ashpd::Error::Portal(PortalError::NotAllowed(message)) => {
            PortalRecordingClientError::PermissionDenied(message)
        }
        other if other.to_string().to_ascii_lowercase().contains("cancel") => {
            PortalRecordingClientError::Cancelled
        }
        other => PortalRecordingClientError::Unavailable(other.to_string()),
    }
}

fn map_client_error(error: PortalRecordingClientError) -> KlypseError {
    match error {
        PortalRecordingClientError::Cancelled => KlypseError::Cancelled,
        PortalRecordingClientError::PermissionDenied(message) => {
            KlypseError::PermissionDenied(message)
        }
        PortalRecordingClientError::Unavailable(message) => {
            KlypseError::UnavailableCapability(message)
        }
    }
}

fn recording_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Media(format!("Wayland recording source failed: {error}"))
}
