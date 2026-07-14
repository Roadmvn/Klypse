use std::{
    fs,
    io::{BufWriter, Write},
    ops::BitOr,
    path::{Path, PathBuf},
};

use ashpd::{
    PortalError, WindowIdentifier,
    desktop::screenshot::{AvailableTargets as AshpdTarget, ScreenshotOptions, ScreenshotProxy},
};
use chrono::Utc;
use gio::prelude::*;
use gtk::gio;
use image::GenericImageView;
use klypse_domain::{
    CaptureArtifact, CaptureBackend, CaptureKind, CaptureRequest, CaptureTarget, DisplayServer,
    KlypseError,
};
use tempfile::Builder;
use url::Url;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AvailableTargetSet(u8);

impl AvailableTargetSet {
    pub const SCREEN: Self = Self(1 << 0);
    pub const WINDOW: Self = Self(1 << 1);
    pub const AREA: Self = Self(1 << 2);
    pub const ACTIVE_WINDOW: Self = Self(1 << 3);

    pub const fn contains(self, target: Self) -> bool {
        self.0 & target.0 == target.0
    }
}

impl BitOr for AvailableTargetSet {
    type Output = Self;

    fn bitor(self, right: Self) -> Self::Output {
        Self(self.0 | right.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalTarget {
    Screen,
    Window,
    Area,
    ActiveWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalSelection {
    Target(PortalTarget),
    InteractiveWithoutTarget,
}

pub fn map_portal_target(target: CaptureTarget, available: AvailableTargetSet) -> PortalSelection {
    let mapped = match target {
        CaptureTarget::Screen => Some((PortalTarget::Screen, AvailableTargetSet::SCREEN)),
        CaptureTarget::Window => Some((PortalTarget::Window, AvailableTargetSet::WINDOW)),
        CaptureTarget::Area => Some((PortalTarget::Area, AvailableTargetSet::AREA)),
        CaptureTarget::ActiveWindow => Some((
            PortalTarget::ActiveWindow,
            AvailableTargetSet::ACTIVE_WINDOW,
        )),
    };
    match mapped {
        Some((target, flag)) if available.contains(flag) => PortalSelection::Target(target),
        _ => PortalSelection::InteractiveWithoutTarget,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalClientError {
    Cancelled,
    PermissionDenied(String),
    Unavailable(String),
}

#[async_trait::async_trait]
pub trait PortalCaptureClient: Send + Sync {
    async fn version(&self) -> Result<u32, PortalClientError>;
    async fn available_targets(&self) -> Result<AvailableTargetSet, PortalClientError>;
    async fn screenshot(
        &self,
        selection: PortalSelection,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<Url, PortalClientError>;
}

pub struct AshpdPortalClient;

#[async_trait::async_trait]
impl PortalCaptureClient for AshpdPortalClient {
    async fn version(&self) -> Result<u32, PortalClientError> {
        ScreenshotProxy::new()
            .await
            .map(|proxy| proxy.version())
            .map_err(map_ashpd_error)
    }

    async fn available_targets(&self) -> Result<AvailableTargetSet, PortalClientError> {
        let targets = ScreenshotProxy::new()
            .await
            .map_err(map_ashpd_error)?
            .available_targets()
            .await
            .map_err(map_ashpd_error)?;
        let mut available = AvailableTargetSet::default();
        for (target, flag) in [
            (AshpdTarget::Screen, AvailableTargetSet::SCREEN),
            (AshpdTarget::Window, AvailableTargetSet::WINDOW),
            (AshpdTarget::Area, AvailableTargetSet::AREA),
            (AshpdTarget::ActiveWindow, AvailableTargetSet::ACTIVE_WINDOW),
        ] {
            if targets.contains(target) {
                available = available | flag;
            }
        }
        Ok(available)
    }

    async fn screenshot(
        &self,
        selection: PortalSelection,
        identifier: Option<&WindowIdentifier>,
    ) -> Result<Url, PortalClientError> {
        let proxy = ScreenshotProxy::new().await.map_err(map_ashpd_error)?;
        let (interactive, target) = match selection {
            PortalSelection::Target(target) => (false, Some(to_ashpd_target(target))),
            PortalSelection::InteractiveWithoutTarget => (true, None),
        };
        let options = ScreenshotOptions::default()
            .set_interactive(interactive)
            .set_modal(true)
            .set_target(target);
        let request = proxy
            .screenshot(identifier, options)
            .await
            .map_err(map_ashpd_error)?;
        let response = request.response().map_err(map_ashpd_error)?;
        Url::parse(response.uri().as_str())
            .map_err(|_| PortalClientError::Unavailable("invalid portal response URI".into()))
    }
}

pub struct PortalCaptureBackend<C = AshpdPortalClient> {
    client: C,
    output_directory: PathBuf,
}

impl PortalCaptureBackend<AshpdPortalClient> {
    pub fn new(output_directory: impl AsRef<Path>) -> Result<Self, KlypseError> {
        Self::with_client(AshpdPortalClient, output_directory)
    }
}

impl<C> PortalCaptureBackend<C> {
    pub fn with_client(client: C, output_directory: impl AsRef<Path>) -> Result<Self, KlypseError> {
        let output_directory = output_directory.as_ref().to_path_buf();
        fs::create_dir_all(&output_directory)?;
        Ok(Self {
            client,
            output_directory,
        })
    }
}

#[async_trait::async_trait]
impl<C> CaptureBackend for PortalCaptureBackend<C>
where
    C: PortalCaptureClient,
{
    async fn capture(&self, request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError> {
        let version = self.client.version().await.map_err(map_client_error)?;
        let available = if version >= 3 {
            self.client.available_targets().await.unwrap_or_default()
        } else {
            AvailableTargetSet::default()
        };
        let selection = map_portal_target(request.target, available);
        let uri = self
            .client
            .screenshot(selection, None)
            .await
            .map_err(map_client_error)?;
        let source = gio::File::for_uri(uri.as_str());
        let (contents, _) = source.load_contents(gio::Cancellable::NONE).map_err(|_| {
            KlypseError::UnavailableCapability("portal screenshot could not be read".into())
        })?;
        let decoded = image::load_from_memory(&contents)
            .map_err(|error| KlypseError::Media(error.to_string()))?;
        let (width, height) = decoded.dimensions();
        let mut temporary = Builder::new()
            .prefix("klypse-portal-")
            .suffix(".png")
            .tempfile_in(&self.output_directory)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            writer.write_all(&contents)?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        let (_file, path) = temporary
            .keep()
            .map_err(|error| KlypseError::Io(error.error))?;

        Ok(CaptureArtifact {
            id: Uuid::new_v4(),
            kind: CaptureKind::Screenshot,
            path,
            width,
            height,
            duration: None,
            created_at: Utc::now(),
            backend: DisplayServer::Wayland,
        })
    }
}

const fn to_ashpd_target(target: PortalTarget) -> AshpdTarget {
    match target {
        PortalTarget::Screen => AshpdTarget::Screen,
        PortalTarget::Window => AshpdTarget::Window,
        PortalTarget::Area => AshpdTarget::Area,
        PortalTarget::ActiveWindow => AshpdTarget::ActiveWindow,
    }
}

fn map_ashpd_error(error: ashpd::Error) -> PortalClientError {
    match error {
        ashpd::Error::Portal(PortalError::Cancelled(_)) => PortalClientError::Cancelled,
        ashpd::Error::Portal(PortalError::NotAllowed(message)) => {
            PortalClientError::PermissionDenied(message)
        }
        other if other.to_string().to_ascii_lowercase().contains("cancel") => {
            PortalClientError::Cancelled
        }
        other => PortalClientError::Unavailable(other.to_string()),
    }
}

fn map_client_error(error: PortalClientError) -> KlypseError {
    match error {
        PortalClientError::Cancelled => KlypseError::Cancelled,
        PortalClientError::PermissionDenied(message) => KlypseError::PermissionDenied(message),
        PortalClientError::Unavailable(message) => KlypseError::UnavailableCapability(message),
    }
}
