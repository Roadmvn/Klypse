use std::{thread, time::Duration};

use klypse_media::{PipelineSource, VideoPipeline, VideoPipelineConfig};

#[test]
fn test_source_produces_discoverable_webm() {
    if !["videotestsrc", "vp8enc", "webmmux"]
        .into_iter()
        .all(gstreamer_element_exists)
    {
        eprintln!("required GStreamer recording plugins are unavailable");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("recording.webm");
    let source = PipelineSource::test_pattern(320, 180).unwrap();
    let pipeline = VideoPipeline::start(source, &output, VideoPipelineConfig::default()).unwrap();

    thread::sleep(Duration::from_millis(500));
    let artifact = pipeline.stop().unwrap();

    assert!(artifact.path.exists());
    assert!(artifact.duration >= Duration::from_millis(400));
    assert_eq!((artifact.width, artifact.height), (320, 180));
    assert!(artifact.file_size > 0);
}

fn gstreamer_element_exists(element: &str) -> bool {
    std::process::Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
