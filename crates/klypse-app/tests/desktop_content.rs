use chrono::Utc;
use gtk::gdk::prelude::*;
use klypse_app::desktop::{
    clipboard::{capture_content_provider, image_provider},
    notification::notification_body,
};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_storage::CaptureRecord;
use uuid::Uuid;

struct CaptureFixture {
    _directory: tempfile::TempDir,
    record: CaptureRecord,
}

impl CaptureFixture {
    fn screenshot() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Klypse capture.png");
        image::RgbaImage::new(8, 6).save(&path).unwrap();
        Self::new(directory, path, CaptureKind::Screenshot)
    }

    fn video() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Klypse video.webm");
        std::fs::write(&path, b"video fixture").unwrap();
        Self::new(directory, path, CaptureKind::Video)
    }

    fn new(directory: tempfile::TempDir, path: std::path::PathBuf, kind: CaptureKind) -> Self {
        Self {
            _directory: directory,
            record: CaptureRecord {
                id: Uuid::new_v4(),
                kind,
                path,
                original_path: None,
                thumbnail_path: None,
                created_at: Utc::now(),
                width: 8,
                height: 6,
                duration: None,
                file_size: 0,
                target: CaptureTarget::Area,
                backend: DisplayServer::X11,
                annotation_json: None,
            },
        }
    }
}

#[test]
fn content_providers_offer_formats_for_their_capture_kind() {
    if gtk::init().is_err() {
        // The focused desktop test runs under Xvfb; allow the full headless
        // workspace suite to keep exercising the non-display code paths.
        return;
    }
    let screenshot = CaptureFixture::screenshot();

    let provider = capture_content_provider(&screenshot.record).unwrap();
    let formats = provider.formats();
    assert!(formats.contain_mime_type("image/png"));
    assert!(formats.contain_mime_type("text/uri-list"));

    let video = CaptureFixture::video();
    let provider = capture_content_provider(&video.record).unwrap();
    let formats = provider.formats();
    assert!(formats.contain_mime_type("text/uri-list"));
    assert!(!formats.contain_mime_type("image/png"));

    clipboard_offers_the_picture_without_advertising_the_file();
}

fn clipboard_offers_the_picture_without_advertising_the_file() {
    // Keep all GTK assertions on the same initialization thread, including
    // when Rust's test harness runs each test on a separate OS thread.
    let screenshot = CaptureFixture::screenshot();

    let formats = image_provider(&screenshot.record.path).unwrap().formats();
    assert!(formats.contain_mime_type("image/png"));
    // Chromium based browsers discard every image format as soon as a file is
    // advertised, which leaves nothing to paste into a page.
    assert!(!formats.contain_mime_type("text/uri-list"));
}

#[test]
fn notification_exposes_only_the_safe_file_name() {
    let fixture = CaptureFixture::screenshot();

    assert_eq!(notification_body(&fixture.record), "Klypse capture.png");
    assert!(!notification_body(&fixture.record).contains("/tmp/"));
}
