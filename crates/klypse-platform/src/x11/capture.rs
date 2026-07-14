use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use image::{DynamicImage, ImageFormat as OutputImageFormat, RgbaImage};
use klypse_domain::{
    CaptureArtifact, CaptureBackend, CaptureKind, CaptureRequest, CaptureSelection, CaptureTarget,
    DisplayServer, KlypseError,
};
use tempfile::Builder;
use uuid::Uuid;
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt, ImageFormat, ImageOrder, Window},
    rust_connection::RustConnection,
};

use super::Rect;

pub struct X11CaptureBackend {
    connection: Arc<RustConnection>,
    screen_number: usize,
    output_directory: PathBuf,
}

impl X11CaptureBackend {
    pub fn connect(output_directory: impl AsRef<Path>) -> Result<Self, KlypseError> {
        let (connection, screen_number) = x11rb::connect(None)
            .map_err(|_| KlypseError::UnavailableCapability("X11 connection unavailable".into()))?;
        Self::with_connection(connection, screen_number, output_directory)
    }

    pub fn with_connection(
        connection: RustConnection,
        screen_number: usize,
        output_directory: impl AsRef<Path>,
    ) -> Result<Self, KlypseError> {
        let output_directory = output_directory.as_ref().to_path_buf();
        fs::create_dir_all(&output_directory)?;
        if connection.setup().roots.get(screen_number).is_none() {
            return Err(KlypseError::UnavailableCapability(
                "X11 screen is unavailable".into(),
            ));
        }
        Ok(Self {
            connection: Arc::new(connection),
            screen_number,
            output_directory,
        })
    }

    pub fn capture_sync(&self, request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError> {
        let rect = self.resolve_rect(request)?;
        self.capture_rect(rect)
    }

    pub fn capture_rect(&self, rect: Rect) -> Result<CaptureArtifact, KlypseError> {
        let screen = &self.connection.setup().roots[self.screen_number];
        validate_rect(rect, screen.width_in_pixels, screen.height_in_pixels)?;
        let x = i16::try_from(rect.x).map_err(|_| invalid_rect())?;
        let y = i16::try_from(rect.y).map_err(|_| invalid_rect())?;
        let width = u16::try_from(rect.width).map_err(|_| invalid_rect())?;
        let height = u16::try_from(rect.height).map_err(|_| invalid_rect())?;
        let reply = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                screen.root,
                x,
                y,
                width,
                height,
                u32::MAX,
            )
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?;
        let setup = self.connection.setup();
        let format = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == reply.depth)
            .ok_or_else(|| KlypseError::Media("X11 pixmap format is unavailable".into()))?;
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.visual_id == screen.root_visual)
            .ok_or_else(|| KlypseError::Media("X11 root visual is unavailable".into()))?;
        let rgba = convert_pixels(
            &reply.data,
            rect.width,
            rect.height,
            format.bits_per_pixel,
            format.scanline_pad,
            setup.image_byte_order,
            (visual.red_mask, visual.green_mask, visual.blue_mask),
        )?;
        let image = RgbaImage::from_raw(rect.width, rect.height, rgba)
            .ok_or_else(|| KlypseError::Media("X11 image dimensions are invalid".into()))?;
        let mut temporary = Builder::new()
            .prefix("klypse-x11-")
            .suffix(".png")
            .tempfile_in(&self.output_directory)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            DynamicImage::ImageRgba8(image)
                .write_to(&mut writer, OutputImageFormat::Png)
                .map_err(|error| KlypseError::Media(error.to_string()))?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        let (_file, path) = temporary
            .keep()
            .map_err(|error| KlypseError::Io(error.error))?;

        Ok(CaptureArtifact {
            id: Uuid::new_v4(),
            kind: CaptureKind::Screenshot,
            path,
            width: rect.width,
            height: rect.height,
            duration: None,
            created_at: Utc::now(),
            backend: DisplayServer::X11,
        })
    }

    fn resolve_rect(&self, request: &CaptureRequest) -> Result<Rect, KlypseError> {
        let screen = &self.connection.setup().roots[self.screen_number];
        match (request.target, request.selection) {
            (CaptureTarget::Screen, _) => Ok(Rect {
                x: 0,
                y: 0,
                width: u32::from(screen.width_in_pixels),
                height: u32::from(screen.height_in_pixels),
            }),
            (CaptureTarget::Area, CaptureSelection::Region(rect)) => Ok(Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            }),
            (CaptureTarget::Window, CaptureSelection::X11Window(window)) => {
                self.window_rect(window)
            }
            (CaptureTarget::Window, CaptureSelection::Automatic) => self
                .active_window()
                .and_then(|window| self.window_rect(window)),
            (CaptureTarget::ActiveWindow, _) => self
                .active_window()
                .and_then(|window| self.window_rect(window)),
            _ => Err(KlypseError::UnavailableCapability(
                "X11 capture target requires an explicit selection".into(),
            )),
        }
    }

    fn active_window(&self) -> Result<Window, KlypseError> {
        let root = self.connection.setup().roots[self.screen_number].root;
        let atom = self
            .connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?
            .atom;
        self.connection
            .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?
            .value32()
            .and_then(|mut values| values.next())
            .ok_or_else(|| {
                KlypseError::UnavailableCapability("X11 active window is unavailable".into())
            })
    }

    fn window_rect(&self, window: Window) -> Result<Rect, KlypseError> {
        let root = self.connection.setup().roots[self.screen_number].root;
        let geometry = self
            .connection
            .get_geometry(window)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?;
        let translated = self
            .connection
            .translate_coordinates(window, root, 0, 0)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?;
        Ok(Rect {
            x: i32::from(translated.dst_x),
            y: i32::from(translated.dst_y),
            width: u32::from(geometry.width),
            height: u32::from(geometry.height),
        })
    }
}

#[async_trait::async_trait]
impl CaptureBackend for X11CaptureBackend {
    async fn capture(&self, request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError> {
        self.capture_sync(request)
    }
}

pub fn bgra_to_rgba(pixels: &[u8]) -> Result<Vec<u8>, KlypseError> {
    if !pixels.len().is_multiple_of(4) {
        return Err(KlypseError::Media(
            "X11 pixel buffer contains a partial BGRA pixel".into(),
        ));
    }
    let mut rgba = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    Ok(rgba)
}

fn convert_pixels(
    data: &[u8],
    width: u32,
    height: u32,
    bits_per_pixel: u8,
    scanline_pad: u8,
    byte_order: ImageOrder,
    masks: (u32, u32, u32),
) -> Result<Vec<u8>, KlypseError> {
    let bytes_per_pixel = usize::from(bits_per_pixel.div_ceil(8));
    if !(2..=4).contains(&bytes_per_pixel) || scanline_pad == 0 {
        return Err(KlypseError::Media("unsupported X11 pixel format".into()));
    }
    let row_bits = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(usize::from(bits_per_pixel)))
        .ok_or_else(|| KlypseError::Media("X11 row size overflow".into()))?;
    let padding = usize::from(scanline_pad);
    let row_bytes = row_bits
        .div_ceil(padding)
        .checked_mul(padding / 8)
        .ok_or_else(|| KlypseError::Media("X11 row size overflow".into()))?;
    let required = row_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| KlypseError::Media("X11 image size overflow".into()))?;
    if data.len() < required {
        return Err(KlypseError::Media(
            "X11 pixel buffer is shorter than its geometry".into(),
        ));
    }
    let capacity = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(height as usize))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| KlypseError::Media("X11 image size overflow".into()))?;
    let mut rgba = Vec::with_capacity(capacity);
    for row in 0..height as usize {
        let row_start = row * row_bytes;
        for column in 0..width as usize {
            let start = row_start + column * bytes_per_pixel;
            let bytes = &data[start..start + bytes_per_pixel];
            let pixel = if byte_order == ImageOrder::LSB_FIRST {
                bytes
                    .iter()
                    .enumerate()
                    .fold(0_u32, |value, (index, byte)| {
                        value | (u32::from(*byte) << (index * 8))
                    })
            } else {
                bytes
                    .iter()
                    .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
            };
            rgba.extend_from_slice(&[
                masked_channel(pixel, masks.0),
                masked_channel(pixel, masks.1),
                masked_channel(pixel, masks.2),
                255,
            ]);
        }
    }
    Ok(rgba)
}

fn masked_channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    let value = (pixel & mask) >> shift;
    ((u64::from(value) * 255) / u64::from(maximum)) as u8
}

fn validate_rect(rect: Rect, root_width: u16, root_height: u16) -> Result<(), KlypseError> {
    let right = i64::from(rect.x) + i64::from(rect.width);
    let bottom = i64::from(rect.y) + i64::from(rect.height);
    if rect.width == 0
        || rect.height == 0
        || rect.x < 0
        || rect.y < 0
        || right > i64::from(root_width)
        || bottom > i64::from(root_height)
    {
        return Err(invalid_rect());
    }
    Ok(())
}

fn invalid_rect() -> KlypseError {
    KlypseError::UnavailableCapability("X11 capture rectangle is outside the root window".into())
}

fn x11_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::UnavailableCapability(format!("X11 request failed: {error}"))
}
