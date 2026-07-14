mod capabilities;
mod selector;
pub mod x11;

pub use capabilities::{CapabilityReport, CapabilityStatus, DisplayProbe};
pub use selector::{BackendChoice, BackendSelector};
pub use x11::{Rect, X11CaptureBackend, bgra_to_rgba, normalize_selection};

pub const CRATE_NAME: &str = "klypse-platform";
