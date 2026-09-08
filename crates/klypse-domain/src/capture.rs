use std::{path::PathBuf, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::KlypseError;

pub const GIF_MAX_DURATION: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureKind {
    Screenshot,
    Video,
    Gif,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureTarget {
    Area,
    Screen,
    Window,
    ActiveWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayServer {
    X11,
    Wayland,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSelection {
    Automatic,
    Region(PixelRect),
    X11Window(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRequest {
    pub target: CaptureTarget,
    /// Wait before taking pixels or opening a selector, so menus can be opened.
    pub delay: Duration,
    pub copy_to_clipboard: bool,
    pub selection: CaptureSelection,
}

impl CaptureRequest {
    pub const fn new(target: CaptureTarget) -> Self {
        Self {
            target,
            delay: Duration::ZERO,
            copy_to_clipboard: true,
            selection: CaptureSelection::Automatic,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingRequest {
    pub kind: CaptureKind,
    pub target: CaptureTarget,
    pub selection: CaptureSelection,
    pub max_duration: Option<Duration>,
}

impl RecordingRequest {
    pub fn new(
        kind: CaptureKind,
        target: CaptureTarget,
        max_duration: Option<Duration>,
    ) -> Result<Self, KlypseError> {
        if kind == CaptureKind::Screenshot {
            return Err(KlypseError::InvalidRequest(
                "a recording cannot use the screenshot kind".into(),
            ));
        }

        if max_duration == Some(Duration::ZERO) {
            return Err(KlypseError::InvalidRequest(
                "a recording duration must be greater than zero".into(),
            ));
        }

        if kind == CaptureKind::Gif && max_duration.is_some_and(|value| value > GIF_MAX_DURATION) {
            return Err(KlypseError::InvalidRequest(
                "GIF recordings are limited to 30 seconds".into(),
            ));
        }

        Ok(Self {
            kind,
            target,
            selection: CaptureSelection::Automatic,
            max_duration,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureArtifact {
    pub id: Uuid,
    pub kind: CaptureKind,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration: Option<Duration>,
    pub created_at: DateTime<Utc>,
    pub backend: DisplayServer,
}
