use klypse_domain::{DisplayServer, HotkeyAction};
use klypse_platform::{
    CapabilityReport, DisplayProbe, HotkeyBinding, HotkeyManager, HotkeyMode, X11HotkeyBackend,
    cli_fallback_commands, parse_accelerator,
};

fn report(wayland: bool, x11: bool, portal: bool) -> CapabilityReport {
    CapabilityReport::from_probe(DisplayProbe::from_values(
        wayland.then(|| "wayland-0".to_owned()),
        x11.then(|| ":99".to_owned()),
        portal,
        false,
        false,
    ))
}

#[test]
fn x11_hotkey_registers_and_unregisters() {
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let (sender, _receiver) = async_channel::unbounded();
    let mut backend = X11HotkeyBackend::start(
        &[HotkeyBinding::new(HotkeyAction::CaptureScreen, "Print")],
        sender,
    )
    .unwrap();

    backend.stop().unwrap();
}

#[test]
fn manager_selects_x11_portal_or_cli_fallback() {
    assert_eq!(
        HotkeyManager::select(&report(false, true, false)),
        HotkeyMode::X11
    );
    assert_eq!(
        HotkeyManager::select(&report(true, false, true)),
        HotkeyMode::Portal
    );
    assert_eq!(
        HotkeyManager::select(&report(true, false, false)),
        HotkeyMode::DesktopCliFallback
    );
    assert_eq!(report(true, false, true).display, DisplayServer::Wayland);
}

#[test]
fn default_accelerators_are_parsed_without_losing_modifiers() {
    let area = parse_accelerator("<Primary>Print").unwrap();
    assert!(area.control);
    assert!(!area.shift);
    assert_eq!(area.key, "Print");

    let gif = parse_accelerator("<Primary><Shift>Print").unwrap();
    assert!(gif.control);
    assert!(gif.shift);
    assert_eq!(gif.key, "Print");
    assert!(parse_accelerator("").is_err());
}

#[test]
fn fallback_commands_are_stable_and_complete() {
    assert_eq!(
        cli_fallback_commands(),
        [
            (HotkeyAction::CaptureArea, "klypse capture area"),
            (HotkeyAction::CaptureScreen, "klypse capture screen"),
            (HotkeyAction::CaptureWindow, "klypse capture active-window"),
            (HotkeyAction::RecordVideo, "klypse record video screen"),
            (HotkeyAction::RecordGif, "klypse record gif area"),
            (HotkeyAction::StopRecording, "klypse stop"),
        ]
    );
}
