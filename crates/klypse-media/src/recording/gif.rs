use std::{
    any::Any,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video::VideoInfo;

use crate::{
    MediaError,
    recording::pipeline::{
        PipelineSource, RecordingArtifact, file_size, finalization_error, gstreamer_error,
        make_element,
    },
};

const FINALIZATION_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(10);
const MAX_EDGE: u32 = 1280;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GifPipelineConfig {
    pub fps: u32,
    pub maximum_duration: Duration,
}

impl GifPipelineConfig {
    pub fn new(fps: u32, maximum_duration: Duration) -> Result<Self, MediaError> {
        if !(1..=30).contains(&fps) {
            return Err(MediaError::InvalidRecording(
                "GIF frame rate must be within 1..=30".into(),
            ));
        }
        if maximum_duration.is_zero() || maximum_duration > Duration::from_secs(30) {
            return Err(MediaError::InvalidRecording(
                "GIF maximum duration must be within zero and thirty seconds".into(),
            ));
        }
        Ok(Self {
            fps,
            maximum_duration,
        })
    }
}

impl Default for GifPipelineConfig {
    fn default() -> Self {
        Self {
            fps: 12,
            maximum_duration: Duration::from_secs(30),
        }
    }
}

struct WorkerResult {
    frame_count: u64,
}

pub struct GifPipeline {
    pipeline: gst::Pipeline,
    path: std::path::PathBuf,
    width: u32,
    height: u32,
    delay_hundredths: u16,
    worker: Option<JoinHandle<Result<WorkerResult, MediaError>>>,
    worker_stop: Arc<AtomicBool>,
    automatic_cancel: Option<mpsc::Sender<()>>,
    automatic_thread: Option<JoinHandle<()>>,
    automatic_stop_due: Arc<AtomicBool>,
    _source_guard: Option<Box<dyn Any + Send>>,
}

impl GifPipeline {
    pub fn start(
        source: PipelineSource,
        destination: impl AsRef<Path>,
        config: GifPipelineConfig,
    ) -> Result<Self, MediaError> {
        gst::init().map_err(gstreamer_error)?;
        GifPipelineConfig::new(config.fps, config.maximum_duration)?;
        let destination = destination.as_ref().to_path_buf();
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let (source, source_caps, source_width, source_height, source_guard) = source.into_parts();
        let (width, height) = scaled_dimensions(source_width, source_height, MAX_EDGE);
        let pipeline = gst::Pipeline::new();
        let input_caps = source_caps
            .map(|caps| {
                let filter = make_element("capsfilter", Some("klypse-gif-input-caps"))?;
                filter.set_property("caps", caps);
                Ok::<_, MediaError>(filter)
            })
            .transpose()?;
        let convert = make_element("videoconvert", Some("klypse-gif-convert"))?;
        let scale = make_element("videoscale", Some("klypse-gif-scale"))?;
        let rate = make_element("videorate", Some("klypse-gif-rate"))?;
        let output_caps = make_element("capsfilter", Some("klypse-gif-output-caps"))?;
        output_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .field("width", width as i32)
                .field("height", height as i32)
                .field("framerate", gst::Fraction::new(config.fps as i32, 1))
                .build(),
        );
        let (sink, appsink) = make_gif_appsink()?;

        let mut elements = vec![source];
        if let Some(filter) = input_caps {
            elements.push(filter);
        }
        elements.extend([convert, scale, rate, output_caps, sink]);
        pipeline
            .add_many(elements.iter())
            .map_err(gstreamer_error)?;
        gst::Element::link_many(elements.iter()).map_err(gstreamer_error)?;

        let delay_hundredths = ((100.0 / f64::from(config.fps)).round() as u16).max(1);
        let worker_stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::spawn({
            let destination = destination.clone();
            let worker_stop = Arc::clone(&worker_stop);
            move || {
                encode_samples(
                    appsink,
                    &destination,
                    width,
                    height,
                    delay_hundredths,
                    worker_stop,
                )
            }
        });

        pipeline
            .set_state(gst::State::Playing)
            .map_err(gstreamer_error)?;
        let (automatic_cancel, automatic_receiver) = mpsc::channel();
        let automatic_stop_due = Arc::new(AtomicBool::new(false));
        let automatic_thread = std::thread::spawn({
            let pipeline = pipeline.clone();
            let automatic_stop_due = Arc::clone(&automatic_stop_due);
            move || {
                if matches!(
                    automatic_receiver.recv_timeout(config.maximum_duration),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    automatic_stop_due.store(true, Ordering::Release);
                    let _ = pipeline.send_event(gst::event::Eos::new());
                }
            }
        });

        Ok(Self {
            pipeline,
            path: destination,
            width,
            height,
            delay_hundredths,
            worker: Some(worker),
            worker_stop,
            automatic_cancel: Some(automatic_cancel),
            automatic_thread: Some(automatic_thread),
            automatic_stop_due,
            _source_guard: source_guard,
        })
    }

    pub fn automatic_stop_due(&self) -> bool {
        self.automatic_stop_due.load(Ordering::Acquire)
    }

    pub fn stop(mut self) -> Result<RecordingArtifact, MediaError> {
        if let Some(cancel) = self.automatic_cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(thread) = self.automatic_thread.take() {
            let _ = thread.join();
        }
        // The duration guard can fire while a live pipeline is still completing
        // its asynchronous transition to Playing. Re-sending EOS here is safe
        // when the guard's event was accepted, and guarantees finalization when
        // that earlier event was rejected during the transition.
        let _ = self.pipeline.send_event(gst::event::Eos::new());
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| finalization_error("GIF pipeline did not expose a message bus"))?;
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
            _ => Err(finalization_error("timed out while finalizing the GIF")),
        };
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(gstreamer_error)?;
        result?;
        let worker = self
            .worker
            .take()
            .ok_or_else(|| finalization_error("GIF encoder worker is unavailable"))?
            .join()
            .map_err(|_| finalization_error("GIF encoder worker panicked"))??;
        validate_gif(&self.path)?;
        let duration = Duration::from_millis(
            worker
                .frame_count
                .saturating_mul(u64::from(self.delay_hundredths))
                .saturating_mul(10),
        );
        Ok(RecordingArtifact {
            file_size: file_size(&self.path)?,
            path: self.path.clone(),
            width: self.width,
            height: self.height,
            duration,
        })
    }
}

impl Drop for GifPipeline {
    fn drop(&mut self) {
        if let Some(cancel) = self.automatic_cancel.take() {
            let _ = cancel.send(());
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        self.worker_stop.store(true, Ordering::Release);
        if let Some(thread) = self.automatic_thread.take() {
            let _ = thread.join();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn make_gif_appsink() -> Result<(gst::Element, gst_app::AppSink), MediaError> {
    let sink = gst::ElementFactory::make("appsink")
        .name("klypse-gif-sink")
        .property("sync", true)
        .property("max-buffers", 2_u32)
        .property("drop", false)
        .property("wait-on-eos", false)
        .build()
        .map_err(gstreamer_error)?;
    let appsink = sink
        .clone()
        .downcast::<gst_app::AppSink>()
        .map_err(|_| MediaError::Gstreamer("GIF sink has an invalid type".into()))?;

    Ok((sink, appsink))
}

pub fn validate_gif(path: impl AsRef<Path>) -> Result<(), MediaError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(File::open(path)?)
        .map_err(|error| finalization_error(error.to_string()))?;
    if decoder
        .read_next_frame()
        .map_err(|error| finalization_error(error.to_string()))?
        .is_none()
    {
        return Err(finalization_error("the finalized GIF contains no frames"));
    }
    Ok(())
}

fn encode_samples(
    appsink: gst_app::AppSink,
    destination: &Path,
    width: u32,
    height: u32,
    delay_hundredths: u16,
    stop: Arc<AtomicBool>,
) -> Result<WorkerResult, MediaError> {
    let file = File::create(destination)?;
    let mut writer = BufWriter::new(file);
    let mut frame_count = 0_u64;
    {
        let mut encoder = gif::Encoder::new(&mut writer, width as u16, height as u16, &[])
            .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        while !stop.load(Ordering::Acquire) {
            let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
                if appsink.is_eos() {
                    break;
                }
                continue;
            };
            let caps = sample
                .caps()
                .ok_or_else(|| MediaError::Gstreamer("GIF frame has no caps".into()))?;
            let info = VideoInfo::from_caps(caps).map_err(gstreamer_error)?;
            let buffer = sample
                .buffer()
                .ok_or_else(|| MediaError::Gstreamer("GIF frame has no buffer".into()))?;
            let map = buffer.map_readable().map_err(gstreamer_error)?;
            let stride = usize::try_from(info.stride()[0])
                .map_err(|_| MediaError::Gstreamer("GIF frame has a negative stride".into()))?;
            let row_bytes = width as usize * 4;
            let mut rgba = Vec::with_capacity(row_bytes * height as usize);
            for row in 0..height as usize {
                let start = row * stride;
                let end = start + row_bytes;
                rgba.extend_from_slice(map.as_slice().get(start..end).ok_or_else(|| {
                    MediaError::Gstreamer("GIF frame buffer is shorter than its caps".into())
                })?);
            }
            let mut frame =
                gif::Frame::from_rgba_speed(width as u16, height as u16, rgba.as_mut_slice(), 10);
            frame.delay = delay_hundredths;
            encoder
                .write_frame(&frame)
                .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
            frame_count += 1;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(WorkerResult { frame_count })
}

fn scaled_dimensions(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    if width <= maximum && height <= maximum {
        return (width, height);
    }
    if width >= height {
        (
            maximum,
            ((u64::from(height) * u64::from(maximum)) / u64::from(width)).max(1) as u32,
        )
    } else {
        (
            ((u64::from(width) * u64::from(maximum)) / u64::from(height)).max(1) as u32,
            maximum,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_sink_does_not_wait_for_consumers_after_eos() {
        gst::init().unwrap();

        let (_, appsink) = make_gif_appsink().unwrap();

        assert!(!appsink.property::<bool>("wait-on-eos"));
    }
}
