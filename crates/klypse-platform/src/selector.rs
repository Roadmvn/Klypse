use klypse_domain::{DisplayServer, KlypseError};

use crate::CapabilityReport;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendChoice {
    X11,
    Portal,
}

pub struct BackendSelector;

impl BackendSelector {
    pub fn select_capture(report: &CapabilityReport) -> Result<BackendChoice, KlypseError> {
        if !report.static_capture.available {
            return Err(KlypseError::UnavailableCapability(
                report.static_capture.detail.clone(),
            ));
        }

        match report.display {
            DisplayServer::X11 => Ok(BackendChoice::X11),
            DisplayServer::Wayland => Ok(BackendChoice::Portal),
        }
    }
}
