use std::path::Path;

use gtk::{gdk, gio, glib, prelude::*};
use klypse_domain::{CaptureKind, KlypseError};
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
    set_clipboard_content(capture_content_provider(record)?)
}

pub fn copy_static_image(path: &Path) -> Result<(), KlypseError> {
    let texture =
        gdk::Texture::from_filename(path).map_err(|error| KlypseError::Media(error.to_string()))?;
    let pixels = gdk::ContentProvider::for_value(&texture.to_value());
    let png = gdk::ContentProvider::for_bytes("image/png", &texture.save_to_png_bytes());
    let file = file_provider(path);
    set_clipboard_content(gdk::ContentProvider::new_union(&[pixels, png, file]))
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
