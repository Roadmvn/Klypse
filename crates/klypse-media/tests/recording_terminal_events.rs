use std::{
    thread,
    time::{Duration, Instant},
};

use gstreamer::{self as gst, prelude::*};
use klypse_media::{
    GifPipeline, GifPipelineConfig, PipelineSource, PipelineTerminalEvent, VideoPipeline,
    VideoPipelineConfig,
};

fn failing_source() -> PipelineSource {
    gst::init().unwrap();
    let source = gst::parse::bin_from_description(
        "videotestsrc is-live=true ! video/x-raw,width=80,height=60,framerate=10/1 ! identity error-after=4",
        true,
    ).unwrap();
    PipelineSource::from_element(source.upcast(), 80, 60).unwrap()
}

fn wait_for_terminal(event: impl Fn() -> Option<PipelineTerminalEvent>) -> PipelineTerminalEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = event() {
            return event;
        }
        assert!(
            Instant::now() < deadline,
            "stream did not report a terminal event while active"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn video_source_error_is_visible_before_stop_and_survives_observation() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = VideoPipeline::start(
        failing_source(),
        directory.path().join("failed.webm"),
        VideoPipelineConfig::default(),
    )
    .unwrap();

    assert!(matches!(
        wait_for_terminal(|| pipeline.terminal_event()),
        PipelineTerminalEvent::Failed(_)
    ));
    assert!(matches!(
        pipeline.terminal_event(),
        Some(PipelineTerminalEvent::Failed(_))
    ));
    let started = Instant::now();
    assert!(pipeline.stop().is_err());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "stop waited for an already observed bus event"
    );
}

#[test]
fn gif_source_error_is_visible_before_stop_and_cleanup_does_not_hang() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = GifPipeline::start(
        failing_source(),
        directory.path().join("failed.gif"),
        GifPipelineConfig::default(),
    )
    .unwrap();

    assert!(matches!(
        wait_for_terminal(|| pipeline.terminal_event()),
        PipelineTerminalEvent::Failed(_)
    ));
    let started = Instant::now();
    assert!(pipeline.stop().is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn gif_encoder_write_error_is_visible_without_a_bus_error() {
    let directory = tempfile::tempdir().unwrap();
    // A directory is a valid pipeline destination argument, but the encoder's
    // asynchronous File::create fails after it receives the first frame.
    let pipeline = GifPipeline::start(
        PipelineSource::test_pattern(80, 60).unwrap(),
        directory.path(),
        GifPipelineConfig::default(),
    )
    .unwrap();

    assert!(matches!(
        wait_for_terminal(|| pipeline.terminal_event()),
        PipelineTerminalEvent::Failed(_)
    ));
    assert!(pipeline.stop().is_err());
}

#[test]
fn spontaneous_video_eos_can_be_observed_then_finalized_once() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = VideoPipeline::start(
        PipelineSource::test_frames(80, 60, 5, 10).unwrap(),
        directory.path().join("ended.webm"),
        VideoPipelineConfig::new(10).unwrap(),
    )
    .unwrap();

    assert_eq!(
        wait_for_terminal(|| pipeline.terminal_event()),
        PipelineTerminalEvent::EndOfStream
    );
    assert_eq!(
        pipeline.terminal_event(),
        Some(PipelineTerminalEvent::EndOfStream)
    );
    let artifact = pipeline.stop().unwrap();
    assert!(artifact.file_size > 0);
    assert_eq!((artifact.width, artifact.height), (80, 60));
}

#[test]
fn spontaneous_gif_eos_preserves_all_queued_frames() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = GifPipeline::start(
        PipelineSource::test_frames(80, 60, 5, 10).unwrap(),
        directory.path().join("ended.gif"),
        GifPipelineConfig::new(10, Duration::from_secs(2)).unwrap(),
    )
    .unwrap();

    assert_eq!(
        wait_for_terminal(|| pipeline.terminal_event()),
        PipelineTerminalEvent::EndOfStream
    );
    let artifact = pipeline.stop().unwrap();
    let mut decoder = gif::DecodeOptions::new()
        .read_info(std::fs::File::open(&artifact.path).unwrap())
        .unwrap();
    let mut frames = 0;
    while decoder.read_next_frame().unwrap().is_some() {
        frames += 1;
    }
    assert_eq!(frames, 5);
    assert!(artifact.file_size > 0);
    assert_eq!((artifact.width, artifact.height), (80, 60));
}
