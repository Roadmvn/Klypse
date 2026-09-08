use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use gtk::{gdk, gio, glib, prelude::*};
use klypse_domain::{CaptureKind, KlypseError};
use klypse_image::{AnnotationDocument, Renderer};
use klypse_storage::CaptureRecord;

pub fn capture_content_provider(
    record: &CaptureRecord,
) -> Result<gdk::ContentProvider, KlypseError> {
    validate_annotation_kind(record)?;
    if record.kind == CaptureKind::Video {
        return Ok(file_provider(&record.path));
    }

    let texture = record_texture(record)?;
    let png = texture.save_to_png_bytes();
    // File managers and browsers can prefer either file format to the pixels.
    // Every advertised representation must contain exactly the saved edits.
    let shared_path = if record.annotation_json.is_some() {
        persist_shared_png(&png)?
    } else {
        record.path.clone()
    };
    Ok(gdk::ContentProvider::new_union(&[
        texture_provider_with_png(&texture, &png),
        file_provider(&shared_path),
    ]))
}

pub fn copy_record(record: &CaptureRecord) -> Result<(), KlypseError> {
    validate_annotation_kind(record)?;
    if record.kind == CaptureKind::Video {
        return set_clipboard_content(file_provider(&record.path));
    }
    set_clipboard_content(texture_provider(&record_texture(record)?))
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
    Ok(texture_provider(&texture))
}

fn texture_provider(texture: &gdk::Texture) -> gdk::ContentProvider {
    texture_provider_with_png(texture, &texture.save_to_png_bytes())
}

fn texture_provider_with_png(texture: &gdk::Texture, png: &glib::Bytes) -> gdk::ContentProvider {
    let pixels = gdk::ContentProvider::for_value(&texture.to_value());
    let png = gdk::ContentProvider::for_bytes("image/png", png);
    gdk::ContentProvider::new_union(&[pixels, png])
}

pub fn copy_flattened_image(
    source: &[u8],
    document: &AnnotationDocument,
) -> Result<(), KlypseError> {
    set_clipboard_content(texture_provider(&flattened_texture(source, document)?))
}

fn validate_annotation_kind(record: &CaptureRecord) -> Result<(), KlypseError> {
    if record.annotation_json.is_some() && record.kind != CaptureKind::Screenshot {
        return Err(KlypseError::InvalidRequest(
            "saved annotations are only supported for screenshots".into(),
        ));
    }
    Ok(())
}

fn record_texture(record: &CaptureRecord) -> Result<gdk::Texture, KlypseError> {
    match record.annotation_json.as_deref() {
        Some(annotation) => {
            let document: AnnotationDocument = serde_json::from_str(annotation)
                .map_err(|error| KlypseError::Media(error.to_string()))?;
            let source = fs::read(&record.path)?;
            // A broken document must stop sharing, never expose the original.
            flattened_texture(&source, &document)
        }
        None => gdk::Texture::from_filename(&record.path)
            .map_err(|error| KlypseError::Media(error.to_string())),
    }
}

fn flattened_texture(
    source: &[u8],
    document: &AnnotationDocument,
) -> Result<gdk::Texture, KlypseError> {
    let rendered = Renderer::default()
        .render_to_rgba(source, document)
        .map_err(|error| KlypseError::Media(error.to_string()))?;
    let width = i32::try_from(rendered.width())
        .map_err(|_| KlypseError::Media("image is too wide to share".into()))?;
    let height = i32::try_from(rendered.height())
        .map_err(|_| KlypseError::Media("image is too tall to share".into()))?;
    let bytes = glib::Bytes::from_owned(rendered.into_raw());
    Ok(gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        width as usize * 4,
    )
    .upcast())
}

fn persist_shared_png(png: &[u8]) -> Result<PathBuf, KlypseError> {
    let directory = glib::user_cache_dir().join("klypse/shared");
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    prune_shared_pngs(&directory);
    let mut file = tempfile::Builder::new()
        .prefix("Klypse-edited-")
        .suffix(".png")
        .tempfile_in(directory)?;
    file.write_all(png)?;
    file.as_file().sync_all()?;
    // Receivers can open a URI after GTK releases the drag provider or after
    // Klypse exits. Keep this immutable snapshot for seven days, instead of
    // tying its lifetime to a TempPath or overwriting it on the next drag.
    let (_, path) = file.keep().map_err(|error| KlypseError::Io(error.error))?;
    Ok(path)
}

fn prune_shared_pngs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("Klypse-edited-") || !name.ends_with(".png") {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .filter(|metadata| metadata.is_file())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(7 * 24 * 60 * 60));
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
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
