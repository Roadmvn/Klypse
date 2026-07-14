use klypse_domain::KlypseError;
use klypse_platform::{BackendChoice, BackendSelector, CapabilityReport, DisplayProbe};

#[test]
fn wayland_requires_the_screenshot_portal() {
    let report = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some("wayland-0".into()),
        None,
        false,
        true,
        true,
    ));

    let error = BackendSelector::select_capture(&report).unwrap_err();

    assert!(matches!(error, KlypseError::UnavailableCapability(_)));
}

#[test]
fn x11_uses_the_direct_backend() {
    let report = CapabilityReport::from_probe(DisplayProbe::from_values(
        None,
        Some(":99".into()),
        false,
        false,
        false,
    ));

    assert_eq!(
        BackendSelector::select_capture(&report).unwrap(),
        BackendChoice::X11
    );
}

#[test]
fn wayland_never_falls_back_to_x11() {
    let report = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some("wayland-0".into()),
        Some(":0".into()),
        false,
        false,
        false,
    ));

    assert!(BackendSelector::select_capture(&report).is_err());
}
