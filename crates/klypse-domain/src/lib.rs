mod backend;
mod capture;
mod command;
mod error;

pub use backend::{CaptureBackend, HotkeyBackend, RecordingBackend};
pub use capture::{
    CaptureArtifact, CaptureKind, CaptureRequest, CaptureSelection, CaptureTarget, DisplayServer,
    PixelRect, RecordingRequest,
};
pub use command::{AppCommand, HotkeyAction};
pub use error::KlypseError;

pub const CRATE_NAME: &str = "klypse-domain";
