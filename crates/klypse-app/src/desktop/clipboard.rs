use std::path::Path;

use gtk::{gdk, gio, glib, prelude::*};
use klypse_domain::{CaptureKind, KlypseError};
use klypse_image::{AnnotationDocument, Renderer};
use klypse_storage::CaptureRecord;

pub fn capture_content_provider(
    record: &CaptureRecord,
) -> Result<gdk::ContentProvider, KlypseError> {
    let file = file_provider(&record.path);
    if record.kind == CaptureKind::Video {
        return Ok(file);
    }

    let texture = gdk::Texture::from_filename(&record.path)
        .map_err(|error| KlypseError::Media(error.to_string()))?;
    let pixels = gdk::ContentProvider::for_value(&texture.to_value());
    let png = gdk::ContentProvider::for_bytes("image/png", &texture.save_to_png_bytes());
    Ok(gdk::ContentProvider::new_union(&[pixels, png, file]))
}

pub fn copy_record(record: &CaptureRecord) -> Result<(), KlypseError> {
    if record.kind == CaptureKind::Video {
        return set_clipboard_content(file_provider(&record.path));
    }
    set_clipboard_content(image_provider(&record.path)?)
}

pub fn copy_static_image(path: &Path) -> Result<(), KlypseError> {
    set_clipboard_content(image_provider(path)?)
}

/// Builds the clipboard payload for a picture: image formats only.
///
/// Advertising the file next to the image breaks pasting into Chromium based
/// browsers. As soon as they see `text/uri-list` they drop every other format
/// and hand the page a bare file name, so the picture never arrives. Dragging
/// still needs the file, which is why `capture_content_provider` keeps it.
pub fn image_provider(path: &Path) -> Result<gdk::ContentProvider, KlypseError> {
    let texture =
        gdk::Texture::from_filename(path).map_err(|error| KlypseError::Media(error.to_string()))?;
    let pixels = gdk::ContentProvider::for_value(&texture.to_value());
    let png = gdk::ContentProvider::for_bytes("image/png", &texture.save_to_png_bytes());
    Ok(gdk::ContentProvider::new_union(&[pixels, png]))
}

pub fn copy_flattened_image(
    source: &[u8],
    document: &AnnotationDocument,
) -> Result<(), KlypseError> {
    let rendered = Renderer::default()
        .render_to_rgba(source, document)
        .map_err(|error| KlypseError::Media(error.to_string()))?;
    let width = rendered.width();
    let height = rendered.height();
    let bytes = glib::Bytes::from_owned(rendered.into_raw());
    let texture = gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        width as usize * 4,
    );
    let pixels = gdk::ContentProvider::for_value(&texture.to_value());
    let png = gdk::ContentProvider::for_bytes("image/png", &texture.save_to_png_bytes());
    set_clipboard_content(gdk::ContentProvider::new_union(&[pixels, png]))
}

pub fn copy_file_uri(path: &Path) -> Result<(), KlypseError> {
    set_clipboard_content(file_provider(path))
}

fn file_provider(path: &Path) -> gdk::ContentProvider {
    let file = gio::File::for_path(path);
    let uri = format!("{}\r\n", file.uri());
    let files = gdk::FileList::from_array(&[file]);
    let typed = gdk::ContentProvider::for_value(&files.to_value());
    let uri = gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from_owned(uri.into_bytes()),
    );
    gdk::ContentProvider::new_union(&[typed, uri])
}

fn set_clipboard_content(provider: gdk::ContentProvider) -> Result<(), KlypseError> {
    let display = gdk::Display::default().ok_or_else(|| {
        KlypseError::UnavailableCapability("clipboard display is unavailable".into())
    })?;
    display
        .clipboard()
        .set_content(Some(&provider))
        .map_err(|error| KlypseError::UnavailableCapability(error.to_string()))
}
