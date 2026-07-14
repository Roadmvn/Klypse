use std::{fs::File, thread, time::Duration};

use klypse_media::{GifPipeline, GifPipelineConfig, PipelineSource, validate_gif};

#[test]
fn ten_frame_test_source_writes_ten_frame_gif() {
    if !["videotestsrc", "appsink"]
        .into_iter()
        .all(gstreamer_element_exists)
    {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("recording.gif");
    let source = PipelineSource::test_frames(160, 90, 10, 10).unwrap();
    let pipeline = GifPipeline::start(
        source,
        &output,
        GifPipelineConfig::new(10, Duration::from_secs(2)).unwrap(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(1_100));

    let artifact = pipeline.stop().unwrap();
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(File::open(&artifact.path).unwrap())
        .unwrap();
    let mut delays = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        delays.push(frame.delay);
    }

    assert_eq!(delays, vec![10; 10]);
    assert_eq!((artifact.width, artifact.height), (160, 90));
    assert_eq!(artifact.duration, Duration::from_secs(1));
}

#[test]
fn config_rejects_more_than_thirty_seconds_and_invalid_fps() {
    assert!(GifPipelineConfig::new(12, Duration::from_secs(31)).is_err());
    assert!(GifPipelineConfig::new(0, Duration::from_secs(10)).is_err());
    assert!(GifPipelineConfig::new(31, Duration::from_secs(10)).is_err());
}

#[test]
fn truncated_gif_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("truncated.gif");
    std::fs::write(&output, b"GIF89a").unwrap();

    assert!(validate_gif(&output).is_err());
}

#[test]
fn duration_guard_requests_automatic_stop() {
    if !["videotestsrc", "appsink"]
        .into_iter()
        .all(gstreamer_element_exists)
    {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("guarded.gif");
    let pipeline = GifPipeline::start(
        PipelineSource::test_pattern(80, 60).unwrap(),
        &output,
        GifPipelineConfig::new(10, Duration::from_millis(250)).unwrap(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(400));

    assert!(pipeline.automatic_stop_due());
    assert!(pipeline.stop().unwrap().path.exists());
}

fn gstreamer_element_exists(element: &str) -> bool {
    std::process::Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
