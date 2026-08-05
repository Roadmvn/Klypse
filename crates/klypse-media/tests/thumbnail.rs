use image::{DynamicImage, Rgba, RgbaImage};
use klypse_media::Thumbnailer;

struct ImageFixture {
    _directory: tempfile::TempDir,
    source: std::path::PathBuf,
}

impl ImageFixture {
    fn png(width: u32, height: u32) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        RgbaImage::from_pixel(width, height, Rgba([12, 34, 56, 255]))
            .save(&source)
            .unwrap();
        Self {
            _directory: directory,
            source,
        }
    }
}

#[test]
fn thumbnail_preserves_aspect_ratio_and_bounds() {
    let fixture = ImageFixture::png(1200, 600);
    let destination = fixture._directory.path().join("thumb.png");

    let info = Thumbnailer::new(256)
        .generate(&fixture.source, &destination)
        .unwrap();

    assert_eq!((info.width, info.height), (256, 128));
    assert_eq!(info.path, destination);
    assert!(destination.exists());
}

#[test]
fn small_images_are_not_enlarged() {
    let fixture = ImageFixture::png(80, 40);
    let destination = fixture._directory.path().join("thumb.png");

    let info = Thumbnailer::new(256)
        .generate(&fixture.source, &destination)
        .unwrap();

    assert_eq!((info.width, info.height), (80, 40));
}

#[test]
fn thumbnail_can_be_generated_atomically_from_an_image_in_memory() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("thumb.png");
    let image =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(1_200, 600, Rgba([12, 34, 56, 255])));

    let info = Thumbnailer::new(256)
        .generate_from_image(image, &destination)
        .unwrap();

    assert_eq!((info.width, info.height), (256, 128));
    assert_eq!(info.path, destination);
    let generated = image::open(&destination).unwrap();
    assert_eq!((generated.width(), generated.height()), (256, 128));
}

#[test]
fn invalid_in_memory_thumbnail_request_keeps_an_existing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("thumb.png");
    std::fs::write(&destination, b"existing thumbnail").unwrap();
    let image = DynamicImage::ImageRgba8(RgbaImage::new(80, 40));

    assert!(
        Thumbnailer::new(0)
            .generate_from_image(image, &destination)
            .is_err()
    );
    assert_eq!(std::fs::read(destination).unwrap(), b"existing thumbnail");
}

#[test]
fn zero_maximum_edge_and_invalid_input_are_rejected_atomically() {
    let fixture = ImageFixture::png(80, 40);
    let zero_destination = fixture._directory.path().join("zero.png");
    let invalid_source = fixture._directory.path().join("invalid.png");
    let invalid_destination = fixture._directory.path().join("invalid-thumb.png");
    std::fs::write(&invalid_source, b"not an image").unwrap();

    assert!(
        Thumbnailer::new(0)
            .generate(&fixture.source, &zero_destination)
            .is_err()
    );
    assert!(
        Thumbnailer::new(256)
            .generate(&invalid_source, &invalid_destination)
            .is_err()
    );
    assert!(!zero_destination.exists());
    assert!(!invalid_destination.exists());
}

#[test]
fn webm_thumbnail_uses_the_first_decoded_frame_when_plugins_exist() {
    if ![
        "videotestsrc",
        "vp8enc",
        "webmmux",
        "uridecodebin",
        "appsink",
    ]
    .into_iter()
    .all(gstreamer_element_exists)
    {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("fixture.webm");
    let destination = directory.path().join("thumb.png");
    let status = std::process::Command::new("gst-launch-1.0")
        .args([
            "-q",
            "videotestsrc",
            "num-buffers=2",
            "pattern=black",
            "!",
            "video/x-raw,width=320,height=180,framerate=1/1",
            "!",
            "vp8enc",
            "deadline=1",
            "!",
            "webmmux",
            "!",
            "filesink",
        ])
        .arg(format!("location={}", source.display()))
        .status()
        .unwrap();
    assert!(status.success());

    let info = Thumbnailer::new(160)
        .generate(&source, &destination)
        .unwrap();

    assert_eq!((info.width, info.height), (160, 90));
    assert!(destination.exists());
}

fn gstreamer_element_exists(element: &str) -> bool {
    std::process::Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
