use klypse_domain::{HotkeyAction, KlypseError};
use klypse_platform::{
    HotkeyBinding, PortalHotkeyClientError, map_portal_hotkey_error, portal_shortcut_specs,
    validate_portal_bindings,
};

#[test]
fn portal_specs_keep_stable_ids_descriptions_and_triggers() {
    let bindings = [
        HotkeyBinding::new(HotkeyAction::CaptureArea, "<Primary>Print"),
        HotkeyBinding::new(HotkeyAction::RecordGif, "<Primary><Shift>Print"),
    ];

    let specs = portal_shortcut_specs(&bindings);

    assert_eq!(specs[0].id, "capture-area");
    assert_eq!(specs[0].description, "Capture area");
    assert_eq!(specs[0].preferred_trigger, "CTRL+Print");
    assert_eq!(specs[1].id, "record-gif");
    assert_eq!(specs[1].description, "Record GIF");
    assert_eq!(specs[1].preferred_trigger, "CTRL+SHIFT+Print");
}

#[test]
fn empty_portal_response_requests_the_cli_fallback() {
    let requested = [HotkeyBinding::new(HotkeyAction::CaptureScreen, "Print")];

    assert!(validate_portal_bindings(&requested, &[]).is_err());
    assert!(validate_portal_bindings(&requested, &["capture-screen".to_owned()]).is_ok());
}

#[test]
fn portal_cancellation_is_not_reported_as_a_failure() {
    assert!(matches!(
        map_portal_hotkey_error(PortalHotkeyClientError::Cancelled),
        KlypseError::Cancelled
    ));
    assert!(matches!(
        map_portal_hotkey_error(PortalHotkeyClientError::PermissionDenied("denied".into())),
        KlypseError::PermissionDenied(_)
    ));
}
