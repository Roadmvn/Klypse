use klypse_domain::DisplayServer;
use klypse_platform::{CapabilityReport, DisplayProbe};

#[test]
fn report_does_not_claim_wayland_without_a_socket() {
    let probe = DisplayProbe::from_values(None, Some(":99".into()), false, false, false);
    let report = CapabilityReport::from_probe(probe);

    assert_eq!(report.display, DisplayServer::X11);
    assert_eq!(report.display_name(), "X11");
    assert!(report.static_capture.available);
    assert!(!report.video_recording.available);
}

#[test]
fn wayland_capture_requires_the_desktop_portal() {
    let unavailable = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some("wayland-0".into()),
        None,
        false,
        true,
        true,
    ));
    let available = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some("wayland-0".into()),
        None,
        true,
        true,
        true,
    ));

    assert!(!unavailable.static_capture.available);
    assert!(available.static_capture.available);
    assert!(available.video_recording.available);
    assert!(available.global_shortcuts.available);
}

#[test]
fn report_never_exposes_display_environment_values() {
    let secret_like_display = "wayland-private-value";
    let report = CapabilityReport::from_probe(DisplayProbe::from_values(
        Some(secret_like_display.into()),
        None,
        false,
        false,
        false,
    ));

    for detail in [
        &report.static_capture.detail,
        &report.video_recording.detail,
        &report.gif_recording.detail,
        &report.global_shortcuts.detail,
    ] {
        assert!(!detail.contains(secret_like_display));
    }
}
