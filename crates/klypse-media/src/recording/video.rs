use std::{path::Path, time::Duration};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_pbutils as gst_pbutils;

use crate::{
    MediaError,
    recording::pipeline::{
        PipelineSource, RecordingArtifact, file_size, finalization_error, gstreamer_error,
        make_element,
    },
};

const FINALIZATION_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoPipelineConfig {
    pub fps: u32,
}

impl VideoPipelineConfig {
    pub fn new(fps: u32) -> Result<Self, MediaError> {
        if !(1..=120).contains(&fps) {
            return Err(MediaError::InvalidRecording(
                "video frame rate must be within 1..=120".into(),
            ));
        }
        Ok(Self { fps })
    }
}

impl Default for VideoPipelineConfig {
    fn default() -> Self {
        Self { fps: 30 }
    }
}

pub struct VideoPipeline {
    pipeline: gst::Pipeline,
    path: std::path::PathBuf,
    width: u32,
    height: u32,
}

impl VideoPipeline {
    pub fn start(
        source: PipelineSource,
        destination: impl AsRef<Path>,
        config: VideoPipelineConfig,
    ) -> Result<Self, MediaError> {
        gst::init().map_err(gstreamer_error)?;
        VideoPipelineConfig::new(config.fps)?;
        let destination = destination.as_ref().to_path_buf();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let (source, source_caps, width, height) = source.into_parts();
        let pipeline = gst::Pipeline::new();
        let input_caps = source_caps
            .map(|caps| {
                let filter = make_element("capsfilter", Some("klypse-input-caps"))?;
                filter.set_property("caps", caps);
                Ok::<_, MediaError>(filter)
            })
            .transpose()?;
        let convert = make_element("videoconvert", Some("klypse-video-convert"))?;
        let rate = make_element("videorate", Some("klypse-video-rate"))?;
        let rate_caps = make_element("capsfilter", Some("klypse-rate-caps"))?;
        rate_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("framerate", gst::Fraction::new(config.fps as i32, 1))
                .build(),
        );
        let queue = make_element("queue", Some("klypse-video-queue"))?;
        let encoder = make_element("vp8enc", Some("klypse-vp8-encoder"))?;
        encoder.set_property_from_str("deadline", "1");
        encoder.set_property_from_str("cpu-used", "8");
        let muxer = make_element("webmmux", Some("klypse-webm-muxer"))?;
        let sink = make_element("filesink", Some("klypse-file-sink"))?;
        sink.set_property("location", destination.to_string_lossy().as_ref());

        let mut elements = vec![source.clone()];
        if let Some(filter) = &input_caps {
            elements.push(filter.clone());
        }
        elements.extend([
            convert.clone(),
            rate.clone(),
            rate_caps.clone(),
            queue.clone(),
            encoder.clone(),
            muxer.clone(),
            sink.clone(),
        ]);
        pipeline
            .add_many(elements.iter())
            .map_err(gstreamer_error)?;
        gst::Element::link_many(elements.iter()).map_err(gstreamer_error)?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(gstreamer_error)?;
        Ok(Self {
            pipeline,
            path: destination,
            width,
            height,
        })
    }

    pub fn stop(self) -> Result<RecordingArtifact, MediaError> {
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| finalization_error("recording pipeline did not expose a message bus"))?;
        if !self.pipeline.send_event(gst::event::Eos::new()) {
            let _ = self.pipeline.set_state(gst::State::Null);
            return Err(finalization_error("recording pipeline rejected EOS"));
        }
        let message = bus.timed_pop_filtered(
            FINALIZATION_TIMEOUT,
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        let result = match message.as_ref().map(|message| message.view()) {
            Some(gst::MessageView::Eos(_)) => Ok(()),
            Some(gst::MessageView::Error(error)) => Err(finalization_error(format!(
                "{} ({:?})",
                error.error(),
                error.debug()
            ))),
            _ => Err(finalization_error(
                "timed out while finalizing the WebM container",
            )),
        };
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(gstreamer_error)?;
        result?;

        let canonical = self.path.canonicalize()?;
        let uri = gst::glib::filename_to_uri(&canonical, None)
            .map_err(|error| finalization_error(error.to_string()))?;
        let discoverer =
            gst_pbutils::Discoverer::new(FINALIZATION_TIMEOUT).map_err(finalization_error)?;
        let info = discoverer.discover_uri(&uri).map_err(finalization_error)?;
        if info.video_streams().is_empty() {
            return Err(finalization_error(
                "the finalized WebM does not contain a video stream",
            ));
        }
        let duration = info
            .duration()
            .map(|value| Duration::from_nanos(value.nseconds()))
            .ok_or_else(|| finalization_error("the finalized WebM has no duration"))?;
        Ok(RecordingArtifact {
            file_size: file_size(&self.path)?,
            path: self.path.clone(),
            width: self.width,
            height: self.height,
            duration,
        })
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
