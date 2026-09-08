use std::{fs, path::PathBuf};

use chrono::Utc;
use gtk::{gdk, gio, glib, prelude::*};
use klypse_app::desktop::clipboard::{capture_content_provider, copy_record};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_image::{AnnotationDocument, Layer, LayerKind, Rect, RedactionMode, Renderer};
use klypse_storage::CaptureRecord;
use uuid::Uuid;

fn provider_bytes(provider: &gdk::ContentProvider, mime: &str) -> Vec<u8> {
    let stream = gio::MemoryOutputStream::new_resizable();
    glib::MainContext::default()
        .block_on(provider.write_mime_type_future(mime, &stream, glib::Priority::DEFAULT))
        .unwrap();
    stream.close(gio::Cancellable::NONE).unwrap();
    stream.steal_as_bytes().as_ref().to_vec()
}

fn assert_pixels(provider: &gdk::ContentProvider, expected: &image::RgbaImage) {
    let png = provider_bytes(provider, "image/png");
    assert_eq!(&image::load_from_memory(&png).unwrap().to_rgba8(), expected);
    let texture = provider
        .value(gdk::Texture::static_type())
        .unwrap()
        .get::<gdk::Texture>()
        .unwrap();
    assert_eq!(
        &image::load_from_memory(&texture.save_to_png_bytes())
            .unwrap()
            .to_rgba8(),
        expected
    );
}

fn shared_file(provider: &gdk::ContentProvider) -> PathBuf {
    let bytes = provider_bytes(provider, "text/uri-list");
    let uri = std::str::from_utf8(&bytes).unwrap();
    assert!(uri.ends_with("\r\n"));
    let file = gio::File::for_uri(uri.trim_end());
    let file_list = provider
        .value(gdk::FileList::static_type())
        .unwrap()
        .get::<gdk::FileList>()
        .unwrap();
    let files = file_list.files();
    assert_eq!(files.len(), 1);
    assert!(files[0].equal(&file));
    file.path().unwrap()
}

// One GTK test keeps all display-bound checks on GTK's initialization thread.
// Run this target under Xvfb; the full headless workspace run can skip it.
#[test]
fn sharing_uses_saved_edits_for_every_format_and_rejects_broken_documents() {
    if gtk::init().is_err() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("original screenshot.png");
    let source_image = image::RgbaImage::from_fn(12, 10, |x, y| {
        image::Rgba([
            (x * 10) as u8,
            (y * 20) as u8,
            (((x + y) % 2) * 240) as u8,
            255,
        ])
    });
    source_image.save(&source_path).unwrap();
    let original_bytes = fs::read(&source_path).unwrap();
    let mut document = AnnotationDocument::new(12, 10)
        .unwrap()
        .with_crop(Rect::new(2.0, 2.0, 8.0, 6.0).unwrap())
        .unwrap();
    document.layers.push(Layer::new(
        "redaction",
        LayerKind::Redaction {
            rect: Rect::new(2.0, 2.0, 4.0, 4.0).unwrap(),
            mode: RedactionMode::Pixelate { block_size: 2 },
        },
    ));
    document.layers.push(Layer::new(
        "mark",
        LayerKind::Rectangle {
            rect: Rect::new(7.0, 5.0, 2.0, 2.0).unwrap(),
            stroke: klypse_image::Stroke::new(
                klypse_image::Rgba::new(1.0, 0.0, 0.0, 1.0).unwrap(),
                1.0,
            )
            .unwrap(),
            fill: Some(klypse_image::Rgba::new(1.0, 0.0, 0.0, 1.0).unwrap()),
        },
    ));
    let mut record = CaptureRecord {
        id: Uuid::new_v4(),
        kind: CaptureKind::Screenshot,
        path: source_path.clone(),
        original_path: None,
        thumbnail_path: None,
        created_at: Utc::now(),
        width: 12,
        height: 10,
        duration: None,
        file_size: original_bytes.len() as u64,
        target: CaptureTarget::Area,
        backend: DisplayServer::X11,
        annotation_json: Some(serde_json::to_string(&document).unwrap()),
    };
    let expected = Renderer::default()
        .render_to_rgba(&original_bytes, &document)
        .unwrap();
    assert_eq!(expected.dimensions(), (8, 6));
    assert_eq!(expected.get_pixel(0, 0), &image::Rgba([25, 50, 120, 255]));
    assert_eq!(expected.get_pixel(6, 4), &image::Rgba([255, 0, 0, 255]));

    copy_record(&record).unwrap();
    let clipboard = gdk::Display::default().unwrap().clipboard();
    let copied = clipboard.content().unwrap();
    assert_pixels(&copied, &expected);
    assert!(!copied.formats().contain_mime_type("text/uri-list"));

    let drag = capture_content_provider(&record).unwrap();
    assert_pixels(&drag, &expected);
    let first_shared_path = shared_file(&drag);
    assert_ne!(first_shared_path, source_path);
    assert_eq!(
        image::open(&first_shared_path).unwrap().to_rgba8(),
        expected
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&first_shared_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    drop(drag);
    assert!(
        first_shared_path.is_file(),
        "a receiver may open the URI after drag-end"
    );

    // A subsequent edit/share must not replace the snapshot an earlier receiver
    // is still consuming. Use blur for the second rendering path as well.
    document.layers[0].kind = LayerKind::Redaction {
        rect: Rect::new(2.0, 2.0, 4.0, 4.0).unwrap(),
        mode: RedactionMode::Blur { radius: 2 },
    };
    record.annotation_json = Some(serde_json::to_string(&document).unwrap());
    let blurred = Renderer::default()
        .render_to_rgba(&original_bytes, &document)
        .unwrap();
    let next_drag = capture_content_provider(&record).unwrap();
    assert_pixels(&next_drag, &blurred);
    let next_shared_path = shared_file(&next_drag);
    assert_ne!(next_shared_path, first_shared_path);
    assert_eq!(
        image::open(&first_shared_path).unwrap().to_rgba8(),
        expected
    );
    assert_eq!(image::open(&next_shared_path).unwrap().to_rgba8(), blurred);
    assert_eq!(fs::read(&source_path).unwrap(), original_bytes);

    for annotation in [
        "{malformed".to_owned(),
        serde_json::to_string(&AnnotationDocument::new(1, 1).unwrap()).unwrap(),
    ] {
        record.annotation_json = Some(annotation);
        assert!(capture_content_provider(&record).is_err());
        assert!(copy_record(&record).is_err());
        assert_pixels(&clipboard.content().unwrap(), &expected);
    }
    record.annotation_json = Some(serde_json::to_string(&document).unwrap());
    record.path = directory.path().join("missing.png");
    assert!(capture_content_provider(&record).is_err());
    assert!(copy_record(&record).is_err());
    record.path = source_path.clone();

    // Existing unannotated image, GIF and video behavior stays intact.
    record.annotation_json = None;
    let plain = capture_content_provider(&record).unwrap();
    assert_pixels(&plain, &source_image);
    assert_eq!(shared_file(&plain), source_path);

    record.kind = CaptureKind::Gif;
    record.path = directory.path().join("animation.gif");
    image::DynamicImage::ImageRgba8(source_image.clone())
        .save(&record.path)
        .unwrap();
    let gif = capture_content_provider(&record).unwrap();
    assert!(gif.formats().contain_mime_type("image/png"));
    assert_eq!(shared_file(&gif), record.path);
    copy_record(&record).unwrap();
    assert!(
        !clipboard
            .content()
            .unwrap()
            .formats()
            .contain_mime_type("text/uri-list")
    );

    record.kind = CaptureKind::Video;
    record.path = directory.path().join("video.webm");
    fs::write(&record.path, b"video fixture").unwrap();
    let video = capture_content_provider(&record).unwrap();
    assert!(!video.formats().contain_mime_type("image/png"));
    assert_eq!(shared_file(&video), record.path);
    copy_record(&record).unwrap();
    assert_eq!(shared_file(&clipboard.content().unwrap()), record.path);
    fs::remove_file(first_shared_path).unwrap();
    fs::remove_file(next_shared_path).unwrap();
}
