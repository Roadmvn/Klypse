mod capabilities;
mod selector;

pub use capabilities::{CapabilityReport, CapabilityStatus, DisplayProbe};
pub use selector::{BackendChoice, BackendSelector};

pub const CRATE_NAME: &str = "klypse-platform";
