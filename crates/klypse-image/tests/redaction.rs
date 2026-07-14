use image::{Rgba, RgbaImage};
use klypse_image::{PixelRect, blur_region, pixelate_region};

#[test]
fn pixelation_replaces_each_block_with_one_color() {
    let mut image = RgbaImage::from_fn(16, 16, |x, y| {
        Rgba([(x * 12) as u8, (y * 12) as u8, (x + y) as u8, 255])
    });

    pixelate_region(&mut image, PixelRect::new(0, 0, 16, 16).unwrap(), 8).unwrap();

    assert_eq!(image.get_pixel(0, 0), image.get_pixel(7, 7));
    assert_ne!(image.get_pixel(0, 0), image.get_pixel(8, 8));
}

#[test]
fn blur_clamps_reads_and_writes_to_the_redaction_rectangle() {
    let mut image = RgbaImage::from_pixel(12, 8, Rgba([255, 0, 0, 255]));
    for y in 2..6 {
        for x in 3..9 {
            let color = if x < 6 {
                Rgba([0, 0, 255, 255])
            } else {
                Rgba([0, 255, 0, 255])
            };
            image.put_pixel(x, y, color);
        }
    }

    blur_region(&mut image, PixelRect::new(3, 2, 6, 4).unwrap(), 2).unwrap();

    assert_eq!(image.get_pixel(2, 2), &Rgba([255, 0, 0, 255]));
    assert_eq!(image.get_pixel(9, 5), &Rgba([255, 0, 0, 255]));
    assert_eq!(image.get_pixel(3, 3)[0], 0);
    assert_eq!(image.get_pixel(8, 3)[0], 0);
    assert!(image.get_pixel(5, 3)[1] > 0);
    assert!(image.get_pixel(6, 3)[2] > 0);
}

#[test]
fn redaction_rejects_zero_strength_and_out_of_bounds_regions() {
    let mut image = RgbaImage::new(10, 10);
    let inside = PixelRect::new(0, 0, 10, 10).unwrap();
    let outside = PixelRect::new(8, 8, 4, 4).unwrap();

    assert!(pixelate_region(&mut image, inside, 0).is_err());
    assert!(blur_region(&mut image, inside, 0).is_err());
    assert!(pixelate_region(&mut image, outside, 4).is_err());
}
