use klypse_app::cli::parse_from;
use klypse_domain::{AppCommand, CaptureKind, CaptureTarget};
use std::process::Command;

#[test]
fn parses_area_capture() {
    let command = parse_from(["klypse", "capture", "area"]).unwrap();

    assert!(
        matches!(command, AppCommand::Capture(request) if request.target == CaptureTarget::Area)
    );
}

#[test]
fn parses_active_window_capture() {
    let command = parse_from(["klypse", "capture", "active-window"]).unwrap();

    assert!(
        matches!(command, AppCommand::Capture(request) if request.target == CaptureTarget::ActiveWindow)
    );
}

#[test]
fn parses_video_and_gif_recording() {
    let video = parse_from(["klypse", "record", "video", "screen"]).unwrap();
    let gif = parse_from(["klypse", "record", "gif"]).unwrap();

    assert!(
        matches!(video, AppCommand::Record(request) if request.kind == CaptureKind::Video && request.target == CaptureTarget::Screen)
    );
    assert!(
        matches!(gif, AppCommand::Record(request) if request.kind == CaptureKind::Gif && request.target == CaptureTarget::Area)
    );
}

#[test]
fn parses_stop_and_explicit_open() {
    assert!(matches!(
        parse_from(["klypse", "stop"]).unwrap(),
        AppCommand::StopRecording
    ));
    assert!(matches!(
        parse_from(["klypse", "open"]).unwrap(),
        AppCommand::Open
    ));
}

#[test]
fn defaults_to_open() {
    assert!(matches!(parse_from(["klypse"]).unwrap(), AppCommand::Open));
}

#[test]
fn rejects_unknown_commands() {
    assert!(parse_from(["klypse", "upload"]).is_err());
}

#[test]
fn help_and_version_are_successful_process_commands() {
    for argument in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_klypse"))
            .arg(argument)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{argument} exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
