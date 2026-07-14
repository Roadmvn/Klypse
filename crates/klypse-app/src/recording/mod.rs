mod backend;
mod controller;

pub use backend::DesktopRecordingBackend;
pub use controller::{
    RECOVERY_MARKER_NAME, RecordingController, RecordingEffects, RecordingStage, RecordingUiState,
};
