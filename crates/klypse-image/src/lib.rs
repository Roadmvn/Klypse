mod document;
mod error;
mod geometry;
mod history;
mod redaction;
mod render;

pub use document::{
    AnnotationDocument, DOCUMENT_VERSION, Layer, LayerKind, RedactionMode, Rgba, Stroke,
};
pub use error::ImageError;
pub use geometry::{PixelRect, Point, Rect, ViewportTransform};
pub use history::{DEFAULT_HISTORY_LIMIT, DocumentCommand, EditHistory};
pub use redaction::{blur_region, pixelate_region};
pub use render::Renderer;

pub const CRATE_NAME: &str = "klypse-image";
