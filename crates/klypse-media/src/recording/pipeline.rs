use std::{any::Any, fs, path::PathBuf, time::Duration};

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::MediaError;

pub struct PipelineSource {
    element: gst::Element,
    width: u32,
    height: u32,
    input_caps: Option<gst::Caps>,
    guard: Option<Box<dyn Any + Send>>,
}

impl PipelineSource {
    pub fn test_pattern(width: u32, height: u32) -> Result<Self, MediaError> {
        gst::init().map_err(gstreamer_error)?;
        validate_dimensions(width, height)?;
        let element = make_element("videotestsrc", Some("klypse-test-source"))?;
        element.set_property("is-live", true);
        element.set_property_from_str("pattern", "ball");
        let caps = gst::Caps::builder("video/x-raw")
            .field("width", width as i32)
            .field("height", height as i32)
            .build();
        Ok(Self {
            element,
            width,
            height,
            input_caps: Some(caps),
            guard: None,
        })
    }

    pub fn from_element(
        element: gst::Element,
        width: u32,
        height: u32,
    ) -> Result<Self, MediaError> {
        validate_dimensions(width, height)?;
        Ok(Self {
            element,
            width,
            height,
            input_caps: None,
            guard: None,
        })
    }

    pub fn from_element_with_guard<T>(
        element: gst::Element,
        width: u32,
        height: u32,
        guard: T,
    ) -> Result<Self, MediaError>
    where
        T: Send + 'static,
    {
        let mut source = Self::from_element(element, width, height)?;
        source.guard = Some(Box::new(guard));
        Ok(source)
    }

    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        gst::Element,
        Option<gst::Caps>,
        u32,
        u32,
        Option<Box<dyn Any + Send>>,
    ) {
        (
            self.element,
            self.input_caps,
            self.width,
            self.height,
            self.guard,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingArtifact {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
    pub file_size: u64,
}

pub(crate) fn make_element(factory: &str, name: Option<&str>) -> Result<gst::Element, MediaError> {
    let mut builder = gst::ElementFactory::make(factory);
    if let Some(name) = name {
        builder = builder.name(name);
    }
    builder
        .build()
        .map_err(|error| MediaError::Gstreamer(error.to_string()))
}

pub(crate) fn gstreamer_error(error: impl std::fmt::Display) -> MediaError {
    MediaError::Gstreamer(error.to_string())
}

pub(crate) fn finalization_error(error: impl std::fmt::Display) -> MediaError {
    MediaError::Finalization(error.to_string())
}

pub(crate) fn file_size(path: &std::path::Path) -> Result<u64, MediaError> {
    Ok(fs::metadata(path)?.len())
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), MediaError> {
    if width == 0 || height == 0 {
        Err(MediaError::InvalidRecording(
            "recording dimensions must be positive".into(),
        ))
    } else {
        Ok(())
    }
}
