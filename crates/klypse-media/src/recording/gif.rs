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
    recording::terminal::{PipelineTerminalEvent, TerminalMonitor},
};

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
    width: u32,
    height: u32,
}

pub struct GifPipeline {
    pipeline: gst::Pipeline,
    terminal: TerminalMonitor,
    path: std::path::PathBuf,
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
        let (source, source_caps, _, _, source_guard) = source.into_parts();
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
                .field("width", gst::IntRange::<i32>::new(1, MAX_EDGE as i32))
                .field("height", gst::IntRange::<i32>::new(1, MAX_EDGE as i32))
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
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
        let terminal = TerminalMonitor::install(
            &pipeline
                .bus()
                .ok_or_else(|| finalization_error("GIF pipeline did not expose a message bus"))?,
        );
        pipeline
            .set_state(gst::State::Playing)
            .map_err(gstreamer_error)?;
        let worker = std::thread::spawn({
            let destination = destination.clone();
            let worker_stop = Arc::clone(&worker_stop);
            let terminal = terminal.clone();
            move || {
                let result = encode_samples(appsink, &destination, delay_hundredths, worker_stop);
                if let Err(error) = &result {
                    terminal.publish(PipelineTerminalEvent::Failed(error.to_string()));
                }
                result
            }
        });

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
            terminal,
            path: destination,
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

    pub fn terminal_event(&self) -> Option<PipelineTerminalEvent> {
        self.terminal.event()
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
        if self.terminal.event().is_none() {
            let _ = self.pipeline.send_event(gst::event::Eos::new());
        }
        self.terminal.wait(Duration::from_secs(10))?;
        // Keep the sink alive until the encoder drains the last queued frames.
        // Null would discard them before the worker can write them to the GIF.
        let worker = self
            .worker
            .take()
            .ok_or_else(|| finalization_error("GIF encoder worker is unavailable"))?
            .join()
            .map_err(|_| finalization_error("GIF encoder worker panicked"))??;
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(gstreamer_error)?;
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
            width: worker.width,
            height: worker.height,
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
    delay_hundredths: u16,
    stop: Arc<AtomicBool>,
) -> Result<WorkerResult, MediaError> {
    let first_sample = loop {
        if stop.load(Ordering::Acquire) {
            return Err(finalization_error(
                "GIF recording stopped before receiving its first frame",
            ));
        }
        if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
            break sample;
        }
        if appsink.is_eos() {
            return Err(finalization_error(
                "GIF recording reached EOS before receiving its first frame",
            ));
        }
    };
    let first_caps = first_sample
        .caps()
        .ok_or_else(|| MediaError::Gstreamer("GIF frame has no caps".into()))?;
    let first_info = VideoInfo::from_caps(first_caps).map_err(gstreamer_error)?;
    let width = first_info.width();
    let height = first_info.height();
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return Err(MediaError::Gstreamer(
            "GIF negotiated invalid output dimensions".into(),
        ));
    }
    let negotiated_caps = first_caps.to_owned();
    appsink.set_caps(Some(&negotiated_caps));

    let file = File::create(destination)?;
    let mut writer = BufWriter::new(file);
    let mut frame_count = 0_u64;
    {
        let mut encoder = gif::Encoder::new(&mut writer, width as u16, height as u16, &[])
            .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        encode_sample(&mut encoder, &first_sample, width, height, delay_hundredths)?;
        frame_count += 1;
        while !stop.load(Ordering::Acquire) {
            let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
                if appsink.is_eos() {
                    break;
                }
                continue;
            };
            encode_sample(&mut encoder, &sample, width, height, delay_hundredths)?;
            frame_count += 1;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(WorkerResult {
        frame_count,
        width,
        height,
    })
}

fn encode_sample<W: Write>(
    encoder: &mut gif::Encoder<W>,
    sample: &gst::Sample,
    width: u32,
    height: u32,
    delay_hundredths: u16,
) -> Result<(), MediaError> {
    let caps = sample
        .caps()
        .ok_or_else(|| MediaError::Gstreamer("GIF frame has no caps".into()))?;
    let info = VideoInfo::from_caps(caps).map_err(gstreamer_error)?;
    if info.width() != width || info.height() != height {
        return Err(MediaError::Gstreamer(
            "GIF frame dimensions changed during recording".into(),
        ));
    }
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
        .map_err(|error| MediaError::Gstreamer(error.to_string()))
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
