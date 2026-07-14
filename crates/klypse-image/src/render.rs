use std::{
    f64::consts::{FRAC_PI_6, PI},
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use cairo::{Context, Format, ImageSurface, LineCap, LineJoin};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use pango::FontDescription;

use crate::{
    AnnotationDocument, ImageError, LayerKind, PixelRect, Rect, RedactionMode, Rgba as Color,
    Stroke, blur_region, pixelate_region,
};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default)]
pub struct Renderer {
    _private: (),
}

impl Renderer {
    pub fn render_to_rgba(
        &self,
        source: &[u8],
        document: &AnnotationDocument,
    ) -> Result<RgbaImage, ImageError> {
        document.validate()?;
        let source = image::load_from_memory(source)
            .map_err(|error| ImageError::Processing(error.to_string()))?
            .to_rgba8();
        if source.dimensions() != (document.original_width, document.original_height) {
            return Err(ImageError::InvalidDocument(
                "document dimensions do not match the source image".into(),
            ));
        }

        let crop = raster_crop(document)?;
        let mut output =
            image::imageops::crop_imm(&source, crop.x, crop.y, crop.width, crop.height).to_image();

        for layer in document.layers.iter().filter(|layer| layer.visible) {
            match &layer.kind {
                LayerKind::Redaction { rect, mode } => {
                    if let Some(region) = clipped_layer_rect(*rect, crop)? {
                        match mode {
                            RedactionMode::Pixelate { block_size } => {
                                pixelate_region(&mut output, region, *block_size)?;
                            }
                            RedactionMode::Blur { radius } => {
                                blur_region(&mut output, region, *radius)?;
                            }
                        }
                    }
                }
                kind => draw_vector_layer(&mut output, kind, crop)?,
            }
        }
        Ok(output)
    }

    pub fn render_to_png(
        &self,
        source: &[u8],
        document: &AnnotationDocument,
        destination: &Path,
    ) -> Result<(), ImageError> {
        let rendered = self.render_to_rgba(source, document)?;
        let temporary = temporary_path(destination);
        let result = (|| {
            DynamicImage::ImageRgba8(rendered)
                .save_with_format(&temporary, ImageFormat::Png)
                .map_err(|error| ImageError::Processing(error.to_string()))?;
            File::open(&temporary)?.sync_all()?;
            fs::rename(&temporary, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn raster_crop(document: &AnnotationDocument) -> Result<PixelRect, ImageError> {
    match document.crop {
        Some(crop) => rect_to_pixels(crop),
        None => PixelRect::new(0, 0, document.original_width, document.original_height),
    }
}

fn rect_to_pixels(rect: Rect) -> Result<PixelRect, ImageError> {
    let left = rect.x.floor().max(0.0) as u32;
    let top = rect.y.floor().max(0.0) as u32;
    let right = rect.right().ceil().max(0.0) as u32;
    let bottom = rect.bottom().ceil().max(0.0) as u32;
    PixelRect::new(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    )
}

fn clipped_layer_rect(rect: Rect, crop: PixelRect) -> Result<Option<PixelRect>, ImageError> {
    let layer = rect_to_pixels(rect)?;
    let crop_right = crop.x + crop.width;
    let crop_bottom = crop.y + crop.height;
    let layer_right = layer.x + layer.width;
    let layer_bottom = layer.y + layer.height;
    let left = layer.x.max(crop.x);
    let top = layer.y.max(crop.y);
    let right = layer_right.min(crop_right);
    let bottom = layer_bottom.min(crop_bottom);
    if left >= right || top >= bottom {
        return Ok(None);
    }
    PixelRect::new(left - crop.x, top - crop.y, right - left, bottom - top).map(Some)
}

fn draw_vector_layer(
    image: &mut RgbaImage,
    layer: &LayerKind,
    crop: PixelRect,
) -> Result<(), ImageError> {
    let mut surface = rgba_to_surface(image)?;
    let context = Context::new(&surface).map_err(processing_error)?;
    context.translate(-f64::from(crop.x), -f64::from(crop.y));
    context.set_line_cap(LineCap::Round);
    context.set_line_join(LineJoin::Round);

    match layer {
        LayerKind::Rectangle { rect, stroke, fill } => {
            context.rectangle(rect.x, rect.y, rect.width, rect.height);
            fill_and_stroke(&context, *fill, *stroke)?;
        }
        LayerKind::Ellipse { rect, stroke, fill } => {
            context.save().map_err(processing_error)?;
            context.translate(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            context.scale(rect.width / 2.0, rect.height / 2.0);
            context.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
            context.restore().map_err(processing_error)?;
            fill_and_stroke(&context, *fill, *stroke)?;
        }
        LayerKind::Line { start, end, stroke } => {
            set_stroke(&context, *stroke);
            context.move_to(start.x, start.y);
            context.line_to(end.x, end.y);
            context.stroke().map_err(processing_error)?;
        }
        LayerKind::Arrow {
            start,
            end,
            stroke,
            head_length,
        } => draw_arrow(&context, *start, *end, *stroke, *head_length)?,
        LayerKind::Text {
            origin,
            text,
            font,
            size,
            color,
        } => {
            set_source(&context, *color);
            let layout = pangocairo::functions::create_layout(&context);
            let mut description = FontDescription::from_string(font);
            description.set_absolute_size(*size * f64::from(pango::SCALE));
            layout.set_font_description(Some(&description));
            layout.set_text(text);
            context.move_to(origin.x, origin.y);
            pangocairo::functions::show_layout(&context, &layout);
        }
        LayerKind::Freehand { points, stroke } => {
            set_stroke(&context, *stroke);
            context.move_to(points[0].x, points[0].y);
            for point in &points[1..] {
                context.line_to(point.x, point.y);
            }
            if points.len() == 1 {
                context.line_to(points[0].x + f64::EPSILON, points[0].y);
            }
            context.stroke().map_err(processing_error)?;
        }
        LayerKind::Redaction { .. } => unreachable!("redaction is rasterized separately"),
    }

    drop(context);
    surface.flush();
    *image = surface_to_rgba(&mut surface)?;
    Ok(())
}

fn fill_and_stroke(
    context: &Context,
    fill: Option<Color>,
    stroke: Stroke,
) -> Result<(), ImageError> {
    if let Some(fill) = fill {
        set_source(context, fill);
        context.fill_preserve().map_err(processing_error)?;
    }
    set_stroke(context, stroke);
    context.stroke().map_err(processing_error)
}

fn draw_arrow(
    context: &Context,
    start: crate::Point,
    end: crate::Point,
    stroke: Stroke,
    head_length: f64,
) -> Result<(), ImageError> {
    set_stroke(context, stroke);
    context.move_to(start.x, start.y);
    context.line_to(end.x, end.y);
    context.stroke().map_err(processing_error)?;

    let angle = (end.y - start.y).atan2(end.x - start.x);
    context.move_to(end.x, end.y);
    context.line_to(
        end.x - head_length * (angle - FRAC_PI_6).cos(),
        end.y - head_length * (angle - FRAC_PI_6).sin(),
    );
    context.line_to(
        end.x - head_length * (angle + FRAC_PI_6).cos(),
        end.y - head_length * (angle + FRAC_PI_6).sin(),
    );
    context.close_path();
    set_source(context, stroke.color);
    context.fill().map_err(processing_error)
}

fn set_stroke(context: &Context, stroke: Stroke) {
    set_source(context, stroke.color);
    context.set_line_width(stroke.width);
}

fn set_source(context: &Context, color: Color) {
    context.set_source_rgba(color.red, color.green, color.blue, color.alpha);
}

fn rgba_to_surface(image: &RgbaImage) -> Result<ImageSurface, ImageError> {
    let width = i32::try_from(image.width())
        .map_err(|_| ImageError::Processing("image is too wide for Cairo".into()))?;
    let height = i32::try_from(image.height())
        .map_err(|_| ImageError::Processing("image is too tall for Cairo".into()))?;
    let mut surface =
        ImageSurface::create(Format::ARgb32, width, height).map_err(processing_error)?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface
            .data()
            .map_err(|error| ImageError::Processing(error.to_string()))?;
        for (x, y, pixel) in image.enumerate_pixels() {
            let alpha = u32::from(pixel[3]);
            let red = (u32::from(pixel[0]) * alpha + 127) / 255;
            let green = (u32::from(pixel[1]) * alpha + 127) / 255;
            let blue = (u32::from(pixel[2]) * alpha + 127) / 255;
            let native = ((alpha << 24) | (red << 16) | (green << 8) | blue).to_ne_bytes();
            let offset = y as usize * stride + x as usize * 4;
            data[offset..offset + 4].copy_from_slice(&native);
        }
    }
    surface.mark_dirty();
    Ok(surface)
}

fn surface_to_rgba(surface: &mut ImageSurface) -> Result<RgbaImage, ImageError> {
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let stride = surface.stride() as usize;
    let data = surface
        .data()
        .map_err(|error| ImageError::Processing(error.to_string()))?;
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let offset = y as usize * stride + x as usize * 4;
            let native =
                u32::from_ne_bytes(data[offset..offset + 4].try_into().expect("four bytes"));
            let alpha = (native >> 24) & 0xff;
            let red = unpremultiply((native >> 16) & 0xff, alpha);
            let green = unpremultiply((native >> 8) & 0xff, alpha);
            let blue = unpremultiply(native & 0xff, alpha);
            image.put_pixel(x, y, Rgba([red, green, blue, alpha as u8]));
        }
    }
    Ok(image)
}

fn unpremultiply(channel: u32, alpha: u32) -> u8 {
    (channel * 255 + alpha / 2)
        .checked_div(alpha)
        .unwrap_or(0)
        .min(255) as u8
}

fn temporary_path(destination: &Path) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("klypse.png");
    destination.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), sequence))
}

fn processing_error(error: cairo::Error) -> ImageError {
    ImageError::Processing(error.to_string())
}
