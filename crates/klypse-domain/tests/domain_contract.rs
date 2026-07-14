use std::time::Duration;

use klypse_domain::{CaptureKind, CaptureSelection, CaptureTarget, HotkeyAction, RecordingRequest};

#[test]
fn gif_request_rejects_more_than_thirty_seconds() {
    let request = RecordingRequest::new(
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(Duration::from_secs(31)),
    );

    assert!(request.is_err());
}

#[test]
fn gif_request_accepts_the_thirty_second_limit() {
    let request = RecordingRequest::new(
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(Duration::from_secs(30)),
    )
    .expect("the documented GIF duration limit should be accepted");

    assert_eq!(request.selection, CaptureSelection::Automatic);
}

#[test]
fn recording_request_rejects_screenshot_and_zero_duration() {
    assert!(RecordingRequest::new(CaptureKind::Screenshot, CaptureTarget::Screen, None).is_err());
    assert!(
        RecordingRequest::new(
            CaptureKind::Video,
            CaptureTarget::Screen,
            Some(Duration::ZERO),
        )
        .is_err()
    );
}

#[test]
fn capture_target_round_trips_as_kebab_case_json() {
    let json = serde_json::to_string(&CaptureTarget::ActiveWindow).unwrap();

    assert_eq!(json, "\"active-window\"");
    assert_eq!(
        serde_json::from_str::<CaptureTarget>(&json).unwrap(),
        CaptureTarget::ActiveWindow
    );
}

#[test]
fn hotkey_action_ids_are_stable() {
    assert_eq!(HotkeyAction::CaptureArea.id(), "capture-area");
    assert_eq!(HotkeyAction::StopRecording.id(), "stop-recording");
}
