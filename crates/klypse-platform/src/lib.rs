mod capabilities;
pub mod portal;
mod selector;
pub mod x11;

pub use capabilities::{CapabilityReport, CapabilityStatus, DisplayProbe};
pub use portal::{
    AvailableTargetSet, PortalCaptureBackend, PortalCaptureClient, PortalClientError,
    PortalSelection, PortalTarget, map_portal_target,
};
pub use selector::{BackendChoice, BackendSelector};
pub use x11::{Rect, X11CaptureBackend, bgra_to_rgba, normalize_selection};

pub const CRATE_NAME: &str = "klypse-platform";
