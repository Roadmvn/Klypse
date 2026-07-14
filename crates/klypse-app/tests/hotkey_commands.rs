use klypse_app::application::command_for_hotkey;
use klypse_domain::{AppCommand, CaptureKind, CaptureTarget, HotkeyAction};

#[test]
fn hotkey_actions_map_to_the_same_commands_as_the_cli() {
    assert!(matches!(
        command_for_hotkey(HotkeyAction::CaptureArea).unwrap(),
        AppCommand::Capture(request) if request.target == CaptureTarget::Area
    ));
    assert!(matches!(
        command_for_hotkey(HotkeyAction::CaptureWindow).unwrap(),
        AppCommand::Capture(request) if request.target == CaptureTarget::ActiveWindow
    ));
    assert!(matches!(
        command_for_hotkey(HotkeyAction::RecordGif).unwrap(),
        AppCommand::Record(request)
            if request.kind == CaptureKind::Gif && request.target == CaptureTarget::Area
    ));
    assert!(matches!(
        command_for_hotkey(HotkeyAction::StopRecording).unwrap(),
        AppCommand::StopRecording
    ));
}
