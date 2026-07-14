use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{ImageError, Point, Rect};

pub const DOCUMENT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Rgba {
    pub fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Result<Self, ImageError> {
        let value = Self {
            red,
            green,
            blue,
            alpha,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(self) -> Result<(), ImageError> {
        if [self.red, self.green, self.blue, self.alpha]
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        {
            Ok(())
        } else {
            Err(ImageError::InvalidDocument(
                "colors must contain finite channels from zero to one".into(),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Rgba,
    pub width: f64,
}

impl Stroke {
    pub fn new(color: Rgba, width: f64) -> Result<Self, ImageError> {
        let value = Self { color, width };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(self) -> Result<(), ImageError> {
        self.color.validate()?;
        if self.width.is_finite() && self.width > 0.0 {
            Ok(())
        } else {
            Err(ImageError::InvalidDocument(
                "stroke width must be positive and finite".into(),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RedactionMode {
    Pixelate { block_size: u32 },
    Blur { radius: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LayerKind {
    Rectangle {
        rect: Rect,
        stroke: Stroke,
        fill: Option<Rgba>,
    },
    Ellipse {
        rect: Rect,
        stroke: Stroke,
        fill: Option<Rgba>,
    },
    Line {
        start: Point,
        end: Point,
        stroke: Stroke,
    },
    Arrow {
        start: Point,
        end: Point,
        stroke: Stroke,
        head_length: f64,
    },
    Text {
        origin: Point,
        text: String,
        font: String,
        size: f64,
        color: Rgba,
    },
    Freehand {
        points: Vec<Point>,
        stroke: Stroke,
    },
    Redaction {
        rect: Rect,
        mode: RedactionMode,
    },
}

impl LayerKind {
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Rectangle { rect, .. }
            | Self::Ellipse { rect, .. }
            | Self::Redaction { rect, .. } => *rect,
            Self::Line { start, end, .. } | Self::Arrow { start, end, .. } => {
                bounds_for_points(&[*start, *end])
            }
            Self::Text {
                origin, text, size, ..
            } => Rect {
                x: origin.x,
                y: origin.y,
                width: (*size * text.chars().count().max(1) as f64 * 0.6).max(1.0),
                height: (*size).max(1.0),
            },
            Self::Freehand { points, .. } => bounds_for_points(points),
        }
    }

    fn validate(&self) -> Result<(), ImageError> {
        match self {
            Self::Rectangle { rect, stroke, fill } | Self::Ellipse { rect, stroke, fill } => {
                rect.validate()?;
                stroke.validate()?;
                if let Some(fill) = fill {
                    fill.validate()?;
                }
            }
            Self::Line { start, end, stroke } => {
                validate_points(&[*start, *end])?;
                stroke.validate()?;
            }
            Self::Arrow {
                start,
                end,
                stroke,
                head_length,
            } => {
                validate_points(&[*start, *end])?;
                stroke.validate()?;
                if !head_length.is_finite() || *head_length <= 0.0 {
                    return Err(ImageError::InvalidDocument(
                        "arrow head length must be positive and finite".into(),
                    ));
                }
            }
            Self::Text {
                origin,
                text,
                font,
                size,
                color,
            } => {
                validate_points(&[*origin])?;
                color.validate()?;
                if text.trim().is_empty() || font.trim().is_empty() {
                    return Err(ImageError::InvalidDocument(
                        "text and font cannot be empty".into(),
                    ));
                }
                if !size.is_finite() || *size <= 0.0 {
                    return Err(ImageError::InvalidDocument(
                        "text size must be positive and finite".into(),
                    ));
                }
            }
            Self::Freehand { points, stroke } => {
                if points.is_empty() {
                    return Err(ImageError::InvalidDocument(
                        "freehand paths cannot be empty".into(),
                    ));
                }
                validate_points(points)?;
                stroke.validate()?;
            }
            Self::Redaction { rect, mode } => {
                rect.validate()?;
                let valid = match mode {
                    RedactionMode::Pixelate { block_size } => *block_size > 0,
                    RedactionMode::Blur { radius } => *radius > 0,
                };
                if !valid {
                    return Err(ImageError::InvalidDocument(
                        "redaction strength must be positive".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub visible: bool,
    pub kind: LayerKind,
}

impl Layer {
    pub fn new(id: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            id: id.into(),
            visible: true,
            kind,
        }
    }

    pub fn bounds(&self) -> Rect {
        self.kind.bounds()
    }

    fn validate(&self) -> Result<(), ImageError> {
        if self.id.trim().is_empty() {
            return Err(ImageError::InvalidDocument(
                "layer identifiers cannot be empty".into(),
            ));
        }
        self.kind.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationDocument {
    pub version: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub crop: Option<Rect>,
    pub layers: Vec<Layer>,
}

impl AnnotationDocument {
    pub fn new(original_width: u32, original_height: u32) -> Result<Self, ImageError> {
        let document = Self {
            version: DOCUMENT_VERSION,
            original_width,
            original_height,
            crop: None,
            layers: Vec::new(),
        };
        document.validate()?;
        Ok(document)
    }

    pub fn with_crop(mut self, crop: Rect) -> Result<Self, ImageError> {
        self.crop = Some(crop);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ImageError> {
        if self.version != DOCUMENT_VERSION {
            return Err(ImageError::InvalidDocument(format!(
                "unsupported document version {}",
                self.version
            )));
        }
        if self.original_width == 0 || self.original_height == 0 {
            return Err(ImageError::InvalidDocument(
                "original dimensions must be positive".into(),
            ));
        }
        let image = Rect::new(
            0.0,
            0.0,
            f64::from(self.original_width),
            f64::from(self.original_height),
        )?;
        if let Some(crop) = self.crop {
            crop.validate()?;
            if !image.contains_rect(crop) {
                return Err(ImageError::InvalidDocument(
                    "crop must stay within the original image".into(),
                ));
            }
        }
        let mut ids = HashSet::new();
        for layer in &self.layers {
            layer.validate()?;
            if !ids.insert(layer.id.as_str()) {
                return Err(ImageError::InvalidDocument(
                    "layer identifiers must be unique".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct UncheckedDocument {
    version: u32,
    original_width: u32,
    original_height: u32,
    crop: Option<Rect>,
    layers: Vec<Layer>,
}

impl<'de> Deserialize<'de> for AnnotationDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = UncheckedDocument::deserialize(deserializer)?;
        let document = Self {
            version: raw.version,
            original_width: raw.original_width,
            original_height: raw.original_height,
            crop: raw.crop,
            layers: raw.layers,
        };
        document.validate().map_err(serde::de::Error::custom)?;
        Ok(document)
    }
}

fn validate_points(points: &[Point]) -> Result<(), ImageError> {
    if points
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite())
    {
        Ok(())
    } else {
        Err(ImageError::InvalidDocument(
            "layer coordinates must be finite".into(),
        ))
    }
}

fn bounds_for_points(points: &[Point]) -> Rect {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(f64::EPSILON),
        height: (max_y - min_y).max(f64::EPSILON),
    }
}
