use gstreamer as gst;
use gstreamer::prelude::*;
use klypse_domain::{CaptureRequest, CaptureSelection, KlypseError};
use klypse_media::PipelineSource;

use super::{Rect, X11CaptureBackend};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct X11RecordingSource {
    rect: Rect,
    show_pointer: bool,
    window_id: Option<u32>,
}

impl X11RecordingSource {
    pub fn for_rect(rect: Rect, show_pointer: bool) -> Result<Self, KlypseError> {
        validate_recording_rect(rect)?;
        Ok(Self {
            rect,
            show_pointer,
            window_id: None,
        })
    }

    pub fn for_target(
        backend: &X11CaptureBackend,
        request: &CaptureRequest,
        show_pointer: bool,
    ) -> Result<Self, KlypseError> {
        let rect = backend.resolve_rect(request)?;
        validate_recording_rect(rect)?;
        backend.validate_root_rect(rect)?;
        let window_id = match request.selection {
            CaptureSelection::X11Window(window) => Some(window),
            _ => None,
        };
        Ok(Self {
            rect,
            show_pointer,
            window_id,
        })
    }

    pub const fn rect(&self) -> Rect {
        self.rect
    }

    pub fn build_element(&self) -> Result<gst::Element, KlypseError> {
        gst::init().map_err(recording_error)?;
        let source = gst::ElementFactory::make("ximagesrc")
            .name("klypse-x11-recording-source")
            .build()
            .map_err(|_| {
                KlypseError::UnavailableCapability(
                    "GStreamer ximagesrc is unavailable for X11 recording".into(),
                )
            })?;
        source.set_property("use-damage", false);
        source.set_property("show-pointer", self.show_pointer);
        let uses_window_id = self.window_id.is_some() && source.find_property("xid").is_some();
        if let Some(window_id) = self.window_id.filter(|_| uses_window_id) {
            source.set_property("xid", u64::from(window_id));
        }
        let (start_x, start_y) = if uses_window_id {
            (0, 0)
        } else {
            (self.rect.x, self.rect.y)
        };
        source.set_property("startx", start_x as u32);
        source.set_property("starty", start_y as u32);
        source.set_property("endx", inclusive_end(start_x, self.rect.width)?);
        source.set_property("endy", inclusive_end(start_y, self.rect.height)?);
        Ok(source)
    }

    pub fn into_pipeline_source(self) -> Result<PipelineSource, KlypseError> {
        PipelineSource::from_element(self.build_element()?, self.rect.width, self.rect.height)
            .map_err(recording_error)
    }
}

fn validate_recording_rect(rect: Rect) -> Result<(), KlypseError> {
    if rect.x < 0 || rect.y < 0 || rect.width == 0 || rect.height == 0 {
        return Err(invalid_rect());
    }
    inclusive_end(rect.x, rect.width)?;
    inclusive_end(rect.y, rect.height)?;
    Ok(())
}

fn inclusive_end(start: i32, length: u32) -> Result<u32, KlypseError> {
    i64::from(start)
        .checked_add(i64::from(length))
        .and_then(|value| value.checked_sub(1))
        .filter(|value| (0..=i64::from(i32::MAX)).contains(value))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(invalid_rect)
}

fn invalid_rect() -> KlypseError {
    KlypseError::UnavailableCapability(
        "X11 recording rectangle is outside supported coordinates".into(),
    )
}

fn recording_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Media(format!("X11 recording source failed: {error}"))
}
