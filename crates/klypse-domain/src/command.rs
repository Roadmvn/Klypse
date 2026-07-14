use crate::{CaptureRequest, RecordingRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    CaptureArea,
    CaptureScreen,
    CaptureWindow,
    RecordVideo,
    RecordGif,
    StopRecording,
}

impl HotkeyAction {
    pub const fn id(self) -> &'static str {
        match self {
            Self::CaptureArea => "capture-area",
            Self::CaptureScreen => "capture-screen",
            Self::CaptureWindow => "capture-window",
            Self::RecordVideo => "record-video",
            Self::RecordGif => "record-gif",
            Self::StopRecording => "stop-recording",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Open,
    Capture(CaptureRequest),
    Record(RecordingRequest),
    StopRecording,
}
