use image::{Rgba, RgbaImage};

use crate::{ImageError, PixelRect};

pub fn pixelate_region(
    image: &mut RgbaImage,
    region: PixelRect,
    block_size: u32,
) -> Result<(), ImageError> {
    validate_region(image, region)?;
    if block_size == 0 {
        return Err(ImageError::Processing(
            "pixelation block size must be positive".into(),
        ));
    }

    let right = region.x + region.width;
    let bottom = region.y + region.height;
    for block_y in (region.y..bottom).step_by(block_size as usize) {
        for block_x in (region.x..right).step_by(block_size as usize) {
            let block_right = block_x.saturating_add(block_size).min(right);
            let block_bottom = block_y.saturating_add(block_size).min(bottom);
            let average = average_premultiplied(image, block_x, block_y, block_right, block_bottom);
            for y in block_y..block_bottom {
                for x in block_x..block_right {
                    image.put_pixel(x, y, average);
                }
            }
        }
    }
    Ok(())
}

pub fn blur_region(
    image: &mut RgbaImage,
    region: PixelRect,
    radius: u32,
) -> Result<(), ImageError> {
    validate_region(image, region)?;
    if radius == 0 {
        return Err(ImageError::Processing(
            "blur radius must be positive".into(),
        ));
    }

    let width = region.width as usize;
    let height = region.height as usize;
    let mut source = Vec::with_capacity(width * height);
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            source.push(to_premultiplied(*image.get_pixel(x, y)));
        }
    }

    let mut horizontal = vec![[0_u32; 4]; source.len()];
    for y in 0..height {
        for x in 0..width {
            let start = x.saturating_sub(radius as usize);
            let end = x.saturating_add(radius as usize).min(width - 1);
            horizontal[y * width + x] =
                average_channels((start..=end).map(|sample_x| source[y * width + sample_x]));
        }
    }

    let mut output = vec![[0_u32; 4]; source.len()];
    for y in 0..height {
        for x in 0..width {
            let start = y.saturating_sub(radius as usize);
            let end = y.saturating_add(radius as usize).min(height - 1);
            output[y * width + x] =
                average_channels((start..=end).map(|sample_y| horizontal[sample_y * width + x]));
        }
    }

    for y in 0..height {
        for x in 0..width {
            image.put_pixel(
                region.x + x as u32,
                region.y + y as u32,
                from_premultiplied(output[y * width + x]),
            );
        }
    }
    Ok(())
}

fn validate_region(image: &RgbaImage, region: PixelRect) -> Result<(), ImageError> {
    let right = region
        .x
        .checked_add(region.width)
        .ok_or_else(|| ImageError::Processing("redaction rectangle overflow".into()))?;
    let bottom = region
        .y
        .checked_add(region.height)
        .ok_or_else(|| ImageError::Processing("redaction rectangle overflow".into()))?;
    if right > image.width() || bottom > image.height() {
        return Err(ImageError::Processing(
            "redaction rectangle exceeds image bounds".into(),
        ));
    }
    Ok(())
}

fn average_premultiplied(
    image: &RgbaImage,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> Rgba<u8> {
    let average = average_channels(
        (top..bottom)
            .flat_map(|y| (left..right).map(move |x| to_premultiplied(*image.get_pixel(x, y)))),
    );
    from_premultiplied(average)
}

fn average_channels<I>(pixels: I) -> [u32; 4]
where
    I: Iterator<Item = [u32; 4]>,
{
    let mut sums = [0_u64; 4];
    let mut count = 0_u64;
    for pixel in pixels {
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
        count += 1;
    }
    std::array::from_fn(|channel| ((sums[channel] + count / 2) / count) as u32)
}

fn to_premultiplied(pixel: Rgba<u8>) -> [u32; 4] {
    let alpha = u32::from(pixel[3]);
    [
        (u32::from(pixel[0]) * alpha + 127) / 255,
        (u32::from(pixel[1]) * alpha + 127) / 255,
        (u32::from(pixel[2]) * alpha + 127) / 255,
        alpha,
    ]
}

fn from_premultiplied(pixel: [u32; 4]) -> Rgba<u8> {
    let alpha = pixel[3].min(255);
    let unpremultiply = |channel: u32| {
        (channel * 255 + alpha / 2)
            .checked_div(alpha)
            .unwrap_or(0)
            .min(255) as u8
    };
    Rgba([
        unpremultiply(pixel[0]),
        unpremultiply(pixel[1]),
        unpremultiply(pixel[2]),
        alpha as u8,
    ])
}
