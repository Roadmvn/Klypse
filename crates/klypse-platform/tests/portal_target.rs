use klypse_domain::CaptureTarget;
use klypse_platform::{AvailableTargetSet, PortalSelection, PortalTarget, map_portal_target};

#[test]
fn unsupported_active_window_uses_interactive_picker() {
    let supported = AvailableTargetSet::SCREEN | AvailableTargetSet::WINDOW;

    let selection = map_portal_target(CaptureTarget::ActiveWindow, supported);

    assert_eq!(selection, PortalSelection::InteractiveWithoutTarget);
}

#[test]
fn supported_area_requests_area_directly() {
    let selection = map_portal_target(CaptureTarget::Area, AvailableTargetSet::AREA);

    assert_eq!(selection, PortalSelection::Target(PortalTarget::Area));
}

#[test]
fn unsupported_explicit_target_falls_back_to_interactive_picker() {
    let selection = map_portal_target(CaptureTarget::Window, AvailableTargetSet::SCREEN);

    assert_eq!(selection, PortalSelection::InteractiveWithoutTarget);
}
