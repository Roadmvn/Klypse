use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video::VideoInfo;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};
use tempfile::NamedTempFile;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("the maximum thumbnail edge must be greater than zero")]
    InvalidMaximumEdge,
    #[error("invalid recording configuration or transition: {0}")]
    InvalidRecording(String),
    #[error("recording finalization failed: {0}")]
    Finalization(String),
    #[error("unsupported thumbnail source: {0}")]
    UnsupportedSource(PathBuf),
    #[error("GStreamer error: {0}")]
    Gstreamer(String),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailInfo {
    pub width: u32,
    pub height: u32,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Thumbnailer {
    max_edge: u32,
}

impl Thumbnailer {
    pub const fn new(max_edge: u32) -> Self {
        Self { max_edge }
    }

    pub fn generate(
        &self,
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<ThumbnailInfo, MediaError> {
        if self.max_edge == 0 {
            return Err(MediaError::InvalidMaximumEdge);
        }
        let source = source.as_ref();
        let destination = destination.as_ref();
        let image = match source
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png" | "gif" | "jpg" | "jpeg" | "webp") => load_oriented_image(source)?,
            Some("webm") => decode_webm_frame(source)?,
            _ => return Err(MediaError::UnsupportedSource(source.to_path_buf())),
        };
        self.write_resized(image, destination)
    }

    fn write_resized(
        &self,
        image: DynamicImage,
        destination: &Path,
    ) -> Result<ThumbnailInfo, MediaError> {
        let resized = if image.width() <= self.max_edge && image.height() <= self.max_edge {
            image
        } else {
            image.resize(self.max_edge, self.max_edge, FilterType::Lanczos3)
        };
        let parent = destination.parent().ok_or_else(|| {
            MediaError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "thumbnail destination has no parent directory",
            ))
        })?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            resized.write_to(&mut writer, ImageFormat::Png)?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        temporary
            .persist(destination)
            .map_err(|error| MediaError::Io(error.error))?;
        fs::File::open(parent)?.sync_all()?;

        Ok(ThumbnailInfo {
            width: resized.width(),
            height: resized.height(),
            path: destination.to_path_buf(),
        })
    }
}

fn load_oriented_image(source: &Path) -> Result<DynamicImage, MediaError> {
    let mut decoder = ImageReader::open(source)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn decode_webm_frame(source: &Path) -> Result<DynamicImage, MediaError> {
    gst::init().map_err(|error| MediaError::Gstreamer(error.to_string()))?;
    let canonical_source = source.canonicalize()?;
    let uri = gst::glib::filename_to_uri(&canonical_source, None)
        .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
    let uri_argument = format!("uri={uri}");
    let pipeline = gst::parse::launchv(&[
        "uridecodebin",
        &uri_argument,
        "!",
        "videoconvert",
        "!",
        "video/x-raw,format=RGBA",
        "!",
        "appsink",
        "name=klypse-thumbnail-sink",
        "sync=false",
        "max-buffers=1",
        "drop=true",
    ])
    .map_err(|error| MediaError::Gstreamer(error.to_string()))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| MediaError::Gstreamer("thumbnail pipeline has an invalid type".into()))?;
    let appsink = pipeline
        .by_name("klypse-thumbnail-sink")
        .ok_or_else(|| MediaError::Gstreamer("thumbnail appsink was not created".into()))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| MediaError::Gstreamer("thumbnail sink has an invalid type".into()))?;

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
    let decoded = (|| {
        let sample = appsink
            .try_pull_sample(gst::ClockTime::from_seconds(5))
            .ok_or_else(|| {
                MediaError::Gstreamer("timed out while decoding the first video frame".into())
            })?;
        let caps = sample
            .caps()
            .ok_or_else(|| MediaError::Gstreamer("decoded frame has no caps".into()))?;
        let info =
            VideoInfo::from_caps(caps).map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| MediaError::Gstreamer("decoded frame has no buffer".into()))?;
        let map = buffer
            .map_readable()
            .map_err(|error| MediaError::Gstreamer(error.to_string()))?;
        let stride = usize::try_from(info.stride()[0])
            .map_err(|_| MediaError::Gstreamer("decoded frame has a negative stride".into()))?;
        let width = info.width();
        let height = info.height();
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| MediaError::Gstreamer("decoded frame is too wide".into()))?;
        let mut pixels = Vec::with_capacity(
            row_bytes
                .checked_mul(height as usize)
                .ok_or_else(|| MediaError::Gstreamer("decoded frame is too large".into()))?,
        );
        for row in 0..height as usize {
            let start = row
                .checked_mul(stride)
                .ok_or_else(|| MediaError::Gstreamer("decoded frame offset overflow".into()))?;
            let end = start
                .checked_add(row_bytes)
                .ok_or_else(|| MediaError::Gstreamer("decoded frame offset overflow".into()))?;
            let source_row = map.as_slice().get(start..end).ok_or_else(|| {
                MediaError::Gstreamer("decoded frame buffer is shorter than its caps".into())
            })?;
            pixels.extend_from_slice(source_row);
        }
        let image = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| MediaError::Gstreamer("decoded frame dimensions are invalid".into()))?;
        Ok(DynamicImage::ImageRgba8(image))
    })();
    let stop_result = pipeline
        .set_state(gst::State::Null)
        .map_err(|error| MediaError::Gstreamer(error.to_string()));
    stop_result?;
    decoded
}
