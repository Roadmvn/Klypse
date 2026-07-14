use std::{thread, time::Duration};

use gstreamer::prelude::*;
use klypse_media::{VideoPipeline, VideoPipelineConfig};
use klypse_platform::{Rect, X11RecordingSource};

#[test]
fn area_source_uses_inclusive_ximagesrc_bounds() {
    if !gstreamer_element_exists("ximagesrc") {
        return;
    }
    gstreamer::init().unwrap();
    let source = X11RecordingSource::for_rect(
        Rect {
            x: 10,
            y: 20,
            width: 100,
            height: 50,
        },
        true,
    )
    .unwrap();
    let element = source.build_element().unwrap();

    assert_eq!(element.property::<u32>("startx"), 10);
    assert_eq!(element.property::<u32>("starty"), 20);
    assert_eq!(element.property::<u32>("endx"), 109);
    assert_eq!(element.property::<u32>("endy"), 69);
    assert!(element.property::<bool>("show-pointer"));
    assert!(!element.property::<bool>("use-damage"));
}

#[test]
fn invalid_and_overflowing_rectangles_are_rejected() {
    assert!(
        X11RecordingSource::for_rect(
            Rect {
                x: -1,
                y: 0,
                width: 10,
                height: 10,
            },
            true,
        )
        .is_err()
    );
    assert!(
        X11RecordingSource::for_rect(
            Rect {
                x: i32::MAX,
                y: 0,
                width: 2,
                height: 10,
            },
            true,
        )
        .is_err()
    );
}

#[test]
fn x11_source_records_a_short_webm() {
    if !["ximagesrc", "vp8enc", "webmmux"]
        .into_iter()
        .all(gstreamer_element_exists)
    {
        return;
    }
    if x11rb::connect(None).is_err() {
        eprintln!("X11 display unavailable; the Xvfb review job runs this test with a display");
        return;
    }
    let Ok(source) = X11RecordingSource::for_rect(
        Rect {
            x: 0,
            y: 0,
            width: 160,
            height: 120,
        },
        false,
    ) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("x11.webm");
    let pipeline = VideoPipeline::start(
        source.into_pipeline_source().unwrap(),
        &output,
        VideoPipelineConfig::default(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(300));

    let artifact = pipeline.stop().unwrap();

    assert_eq!((artifact.width, artifact.height), (160, 120));
    assert!(artifact.path.exists());
}

fn gstreamer_element_exists(element: &str) -> bool {
    std::process::Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
