mod document;
mod error;
mod geometry;
mod history;

pub use document::{
    AnnotationDocument, DOCUMENT_VERSION, Layer, LayerKind, RedactionMode, Rgba, Stroke,
};
pub use error::ImageError;
pub use geometry::{PixelRect, Point, Rect, ViewportTransform};
pub use history::{DEFAULT_HISTORY_LIMIT, DocumentCommand, EditHistory};

pub const CRATE_NAME: &str = "klypse-image";
