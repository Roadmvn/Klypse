use serde::{Deserialize, Serialize};

use crate::ImageError;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Result<Self, ImageError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(ImageError::InvalidGeometry(
                "point coordinates must be finite",
            ));
        }
        Ok(Self { x, y })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, ImageError> {
        if ![x, y, width, height].into_iter().all(f64::is_finite) {
            return Err(ImageError::InvalidGeometry(
                "rectangle values must be finite",
            ));
        }
        if width <= 0.0 || height <= 0.0 {
            return Err(ImageError::InvalidGeometry(
                "rectangle dimensions must be positive",
            ));
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn from_points(start: Point, end: Point) -> Result<Self, ImageError> {
        Self::new(
            start.x.min(end.x),
            start.y.min(end.y),
            (end.x - start.x).abs(),
            (end.y - start.y).abs(),
        )
    }

    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn contains_rect(self, other: Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    pub fn validate(self) -> Result<(), ImageError> {
        Self::new(self.x, self.y, self.width, self.height).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::InvalidGeometry(
                "pixel rectangle dimensions must be positive",
            ));
        }
        x.checked_add(width)
            .and_then(|_| y.checked_add(height))
            .ok_or(ImageError::InvalidGeometry("pixel rectangle overflow"))?;
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportTransform {
    scale: f64,
    offset: Point,
}

impl ViewportTransform {
    pub fn new(scale: f64, offset: Point) -> Result<Self, ImageError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(ImageError::InvalidGeometry(
                "viewport scale must be positive and finite",
            ));
        }
        Point::new(offset.x, offset.y)?;
        Ok(Self { scale, offset })
    }

    pub fn image_to_view(self, point: Point) -> Point {
        Point {
            x: point.x * self.scale + self.offset.x,
            y: point.y * self.scale + self.offset.y,
        }
    }

    pub fn view_to_image(self, point: Point) -> Point {
        Point {
            x: (point.x - self.offset.x) / self.scale,
            y: (point.y - self.offset.y) / self.scale,
        }
    }

    pub const fn scale(self) -> f64 {
        self.scale
    }
}
