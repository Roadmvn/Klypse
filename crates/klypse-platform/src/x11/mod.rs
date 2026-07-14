mod capture;
mod geometry;

pub use capture::{X11CaptureBackend, bgra_to_rgba};
pub use geometry::{Rect, normalize_selection};
